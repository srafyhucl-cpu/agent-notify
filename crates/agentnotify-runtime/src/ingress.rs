use std::{path::Path, path::PathBuf, sync::Arc, time::Duration};

use agentnotify_application::{IngestError, IngestService};
use agentnotify_ingress::{Spool, SpoolLimits};
use tokio::sync::watch;

use crate::{
    ComponentFailure, RuntimeError, SharedAgentEventFilter, SharedAgentEventObserver,
    filter::allow_notification, observer::notify_observer,
};

const DRAIN_BATCH_SIZE: usize = 100;
const SPOOL_MAX_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);

pub(crate) async fn drain_before_start(
    spool_dir: Option<&Path>,
    ingest: Arc<IngestService>,
    observer: Option<SharedAgentEventObserver>,
    filter: Option<SharedAgentEventFilter>,
) -> Result<(), RuntimeError> {
    let Some(spool_dir) = spool_dir else {
        return Ok(());
    };
    drain_all(spool_dir, ingest, observer, filter).await
}

/// 运行期周期重放：把「管道即时投递失败、落盘等待」的事件补送（不必等下次启动重放）。
/// 单次重放失败只告警、下一周期继续（旁路语义：不把组件打挂，也不影响在线链路）。
pub(crate) async fn run_spool_replay(
    spool_dir: Option<PathBuf>,
    ingest: Arc<IngestService>,
    observer: Option<SharedAgentEventObserver>,
    filter: Option<SharedAgentEventFilter>,
    mut cancel: watch::Receiver<bool>,
    interval: Duration,
) -> Result<(), ComponentFailure> {
    let Some(spool_dir) = spool_dir else {
        return Ok(());
    };
    loop {
        if *cancel.borrow() {
            return Ok(());
        }
        tokio::select! {
            changed = cancel.changed() => {
                if changed.is_err() || *cancel.borrow() {
                    return Ok(());
                }
            }
            _ = tokio::time::sleep(interval) => {
                if let Err(error) = drain_all(&spool_dir, ingest.clone(), observer.clone(), filter.clone()).await {
                    tracing::warn!(code = error.code(), "spool 周期重放失败（下个周期重试）");
                }
            }
        }
    }
}

async fn drain_all(
    spool_dir: &Path,
    ingest: Arc<IngestService>,
    observer: Option<SharedAgentEventObserver>,
    filter: Option<SharedAgentEventFilter>,
) -> Result<(), RuntimeError> {
    let spool = Spool::open(spool_dir, SpoolLimits::default()).map_err(RuntimeError::from)?;
    spool
        .cleanup_expired(SPOOL_MAX_AGE)
        .map_err(RuntimeError::from)?;

    loop {
        let entries = spool
            .drain_batch(DRAIN_BATCH_SIZE)
            .map_err(RuntimeError::from)?;
        if entries.is_empty() {
            return Ok(());
        }

        for entry in entries {
            match &entry.event {
                Err(error) => {
                    tracing::warn!(code = error.code(), "隔离无效的离线入口事件");
                    spool
                        .quarantine(&entry, error.code())
                        .map_err(RuntimeError::from)?;
                }
                Ok(envelope) => {
                    // 过滤：编排会话的原始完成事件不建通知/投递，但仍回调观察者；条目照常 ack。
                    if !allow_notification(filter.as_ref(), envelope) {
                        tracing::debug!(
                            request_id = %envelope.request_id,
                            "事件按过滤器跳过 ingest（仍回调观察者）"
                        );
                        notify_observer(observer.as_ref(), envelope).await;
                        spool.ack(&entry).map_err(RuntimeError::from)?;
                        continue;
                    }
                    match ingest.ingest(envelope.clone()).await {
                        Ok(_) => {
                            notify_observer(observer.as_ref(), envelope).await;
                            spool.ack(&entry).map_err(RuntimeError::from)?;
                        }
                        Err(error) if permanent_ingest_error(&error) => {
                            tracing::warn!(code = error.code(), "隔离无法处理的离线入口事件");
                            spool
                                .quarantine(&entry, error.code())
                                .map_err(RuntimeError::from)?;
                        }
                        Err(error) => return Err(RuntimeError::Ingest(error)),
                    }
                }
            }
        }
    }
}

fn permanent_ingest_error(error: &IngestError) -> bool {
    matches!(
        error,
        IngestError::AgentNotRegistered { .. }
            | IngestError::InvalidNotification
            | IngestError::Agent(
                agentnotify_agent_sdk::AgentError::InvalidInput
                    | agentnotify_agent_sdk::AgentError::InvalidEvent
                    | agentnotify_agent_sdk::AgentError::UnsupportedCapability
                    | agentnotify_agent_sdk::AgentError::Ignored(_)
            )
    )
}
