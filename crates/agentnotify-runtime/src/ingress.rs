use std::{path::Path, sync::Arc, time::Duration};

use agentnotify_application::{IngestError, IngestService};
use agentnotify_ingress::{Spool, SpoolLimits};

use crate::RuntimeError;

const DRAIN_BATCH_SIZE: usize = 100;
const SPOOL_MAX_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);

pub(crate) async fn drain_before_start(
    spool_dir: Option<&Path>,
    ingest: Arc<IngestService>,
) -> Result<(), RuntimeError> {
    let Some(spool_dir) = spool_dir else {
        return Ok(());
    };
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
                Ok(envelope) => match ingest.ingest(envelope.clone()).await {
                    Ok(_) => {
                        spool.ack(&entry).map_err(RuntimeError::from)?;
                    }
                    Err(error) if permanent_ingest_error(&error) => {
                        tracing::warn!(code = error.code(), "隔离无法处理的离线入口事件");
                        spool
                            .quarantine(&entry, error.code())
                            .map_err(RuntimeError::from)?;
                    }
                    Err(error) => return Err(RuntimeError::Ingest(error)),
                },
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
