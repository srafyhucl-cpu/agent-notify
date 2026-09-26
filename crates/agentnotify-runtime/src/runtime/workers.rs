//! 运行时后台 worker：渠道入站消费、投递、状态刷新与致命故障监督，
//! 另含装配层共用的快照构建与错误归类助手。

use std::{sync::Arc, time::Duration};

use agentnotify_application::{
    ChannelAccountStore, DeliveryError, DeliveryService, ReplyError, ReplyService, StatusError,
    StatusOverview, StatusService, StoreError,
};
use agentnotify_channel_sdk::{ChannelAccount, ChannelError, ChannelRegistry, InboundEmitter};
use agentnotify_domain::InboundMessage;
use agentnotify_storage_sqlite::SqliteStore;
use tokio::sync::{mpsc, watch};

use crate::migration::{MigrationSnapshot, MigrationState};
use crate::{ComponentFailure, ComponentState, RuntimeState, Supervisor};

use super::error::RuntimeError;
use super::{DiagnosticItem, DiagnosticLevel, RuntimeSnapshot, SnapshotMetadata};

pub(super) async fn enabled_accounts(
    store: Arc<SqliteStore>,
    channels: Arc<ChannelRegistry>,
) -> Result<
    Vec<(
        Arc<dyn agentnotify_channel_sdk::ChannelAdapter>,
        ChannelAccount,
    )>,
    RuntimeError,
> {
    let account_store: Arc<dyn ChannelAccountStore> = store;
    let mut accounts = Vec::new();
    for adapter in channels.all() {
        let descriptor = adapter.descriptor();
        for account in account_store.list(&descriptor.id).await? {
            if account.enabled {
                accounts.push((adapter.clone(), account));
            }
        }
    }
    Ok(accounts)
}

pub(super) async fn run_channel(
    name: String,
    adapter: Arc<dyn agentnotify_channel_sdk::ChannelAdapter>,
    account: ChannelAccount,
    emit: InboundEmitter,
    mut cancel: watch::Receiver<bool>,
    poll_interval: Duration,
) -> Result<(), ComponentFailure> {
    let task = adapter
        .start(account, emit)
        .await
        .map_err(|error| channel_failure(error, &name))?;
    loop {
        if *cancel.borrow() {
            return task
                .shutdown()
                .await
                .map_err(|error| channel_failure(error, &name));
        }
        tokio::select! {
            changed = cancel.changed() => {
                if changed.is_ok() {
                    return task.shutdown().await.map_err(|error| channel_failure(error, &name));
                }
                return Ok(());
            }
            _ = tokio::time::sleep(poll_interval) => {
                if task.is_finished() {
                    return task.shutdown().await.map_err(|error| channel_failure(error, &name));
                }
            }
        }
    }
}

pub(super) async fn run_inbound_consumer(
    mut receiver: mpsc::Receiver<InboundMessage>,
    reply: Arc<ReplyService>,
    mut cancel: watch::Receiver<bool>,
) -> Result<(), ComponentFailure> {
    loop {
        if *cancel.borrow() {
            return Ok(());
        }
        tokio::select! {
            message = receiver.recv() => {
                let Some(message) = message else {
                    return Ok(());
                };
                if let Err(error) = reply.handle(message).await {
                    if matches!(&error, ReplyError::Store(store_error) if store_error_is_fatal(store_error)) {
                        return Err(ComponentFailure::new(
                            error.code(),
                            error.message(),
                            true,
                        ));
                    }
                    tracing::warn!(code = error.code(), "处理入站回复失败");
                }
            }
            changed = cancel.changed() => {
                if changed.is_err() || *cancel.borrow() {
                    return Ok(());
                }
            }
        }
    }
}

pub(super) async fn run_outbox_worker(
    delivery: Arc<DeliveryService>,
    mut outbox_pause: watch::Receiver<bool>,
    mut cancel: watch::Receiver<bool>,
    idle_delay: Duration,
) -> Result<(), ComponentFailure> {
    loop {
        if *cancel.borrow() {
            return Ok(());
        }
        if *outbox_pause.borrow() {
            tokio::select! {
                changed = outbox_pause.changed() => {
                    if changed.is_err() {
                        return Ok(());
                    }
                }
                changed = cancel.changed() => {
                    if changed.is_err() || *cancel.borrow() {
                        return Ok(());
                    }
                }
            }
            continue;
        }
        match delivery.process_next().await {
            Ok(_) => {}
            Err(error) => {
                if delivery_error_is_fatal(&error) {
                    return Err(ComponentFailure::new(error.code(), error.message(), true));
                }
                tracing::warn!(code = error.code(), "Outbox worker 处理失败");
                tokio::select! {
                    changed = cancel.changed() => {
                        if changed.is_err() || *cancel.borrow() {
                            return Ok(());
                        }
                    }
                    _ = tokio::time::sleep(idle_delay) => {}
                }
                continue;
            }
        }
        tokio::select! {
            changed = cancel.changed() => {
                if changed.is_err() || *cancel.borrow() {
                    return Ok(());
                }
            }
            _ = tokio::time::sleep(idle_delay) => {}
        }
    }
}

pub(super) async fn run_status_refresher(
    status: Arc<StatusService>,
    supervisor: Supervisor,
    metadata: SnapshotMetadata,
    sender: watch::Sender<RuntimeSnapshot>,
    mut cancel: watch::Receiver<bool>,
    interval: Duration,
) -> Result<(), ComponentFailure> {
    loop {
        if *cancel.borrow() {
            return Ok(());
        }
        match status.snapshot().await {
            Ok(overview) => {
                let state = supervisor.runtime_state();
                let _ = sender.send(build_snapshot(
                    &metadata.app_version,
                    &metadata.platform,
                    state,
                    overview,
                    &supervisor,
                    &metadata.migration,
                ));
            }
            Err(error) => {
                if status_error_is_fatal(&error) {
                    return Err(ComponentFailure::new(error.code(), error.message(), true));
                }
                tracing::warn!(code = error.code(), "刷新运行状态失败");
            }
        }
        tokio::select! {
            changed = cancel.changed() => {
                if changed.is_err() || *cancel.borrow() {
                    return Ok(());
                }
            }
            _ = tokio::time::sleep(interval) => {}
        }
    }
}

pub(super) async fn run_fatal_monitor(
    supervisor: Supervisor,
    cancel: watch::Receiver<bool>,
    cancel_sender: watch::Sender<bool>,
) -> Result<(), ComponentFailure> {
    let mut fatal = supervisor.subscribe_fatal_error();
    let mut cancel = cancel;
    loop {
        if *cancel.borrow() {
            return Ok(());
        }
        tokio::select! {
            changed = fatal.changed() => {
                if changed.is_err() {
                    return Ok(());
                }
                if fatal.borrow().is_some() {
                    let _ = cancel_sender.send(true);
                    return Ok(());
                }
            }
            changed = cancel.changed() => {
                if changed.is_err() {
                    return Ok(());
                }
            }
        }
    }
}

pub(super) fn build_snapshot(
    app_version: &str,
    platform: &str,
    state: RuntimeState,
    overview: StatusOverview,
    supervisor: &Supervisor,
    migration: &MigrationSnapshot,
) -> RuntimeSnapshot {
    let components = supervisor.components();
    let failed_components = components
        .iter()
        .filter(|component| component.state == ComponentState::Failed)
        .count();
    let diagnostics = vec![
        DiagnosticItem {
            code: "storage".into(),
            level: if overview.storage.recent_error.is_some() {
                DiagnosticLevel::Warning
            } else {
                DiagnosticLevel::Ok
            },
            message: if overview.storage.recent_error.is_some() {
                "数据库可读写，但最近存在一次失败记录".into()
            } else {
                "数据库可读写".into()
            },
        },
        DiagnosticItem {
            code: "components".into(),
            level: if failed_components == 0 {
                DiagnosticLevel::Ok
            } else {
                DiagnosticLevel::Warning
            },
            message: if failed_components == 0 {
                "后台组件运行正常".into()
            } else {
                "部分后台组件已停止，其他组件仍在运行".into()
            },
        },
        migration_diagnostic(migration),
    ];
    RuntimeSnapshot {
        app_version: app_version.into(),
        platform: platform.into(),
        state,
        overview,
        components,
        diagnostics,
        migration: migration.clone(),
    }
}

fn migration_diagnostic(migration: &MigrationSnapshot) -> DiagnosticItem {
    let (level, message) = match migration.state {
        MigrationState::NotConfigured => (DiagnosticLevel::Ok, "旧数据迁移未配置".into()),
        MigrationState::NotDetected => (DiagnosticLevel::Ok, "未发现旧版 Agent-notify 数据".into()),
        MigrationState::Completed => (DiagnosticLevel::Ok, "旧数据迁移已完成".into()),
        MigrationState::Partial => (
            DiagnosticLevel::Warning,
            format!(
                "旧数据已导入，跳过 {} 条损坏记录",
                migration
                    .report
                    .as_ref()
                    .map(|report| report.skipped_records)
                    .unwrap_or_default()
            ),
        ),
        MigrationState::Required => (
            DiagnosticLevel::Error,
            migration
                .error
                .as_ref()
                .map(|error| error.message.clone())
                .unwrap_or_else(|| "旧数据迁移未完成，当前处于只读诊断模式".into()),
        ),
    };
    DiagnosticItem {
        code: "legacy-migration".into(),
        level,
        message,
    }
}

fn channel_failure(error: ChannelError, _name: &str) -> ComponentFailure {
    ComponentFailure::new(error.code(), error.message(), false)
}

fn delivery_error_is_fatal(error: &DeliveryError) -> bool {
    matches!(error, DeliveryError::Store(store_error) if store_error_is_fatal(store_error))
}

fn status_error_is_fatal(error: &StatusError) -> bool {
    matches!(error, StatusError::Store(store_error) if store_error_is_fatal(store_error))
}

fn store_error_is_fatal(error: &StoreError) -> bool {
    matches!(
        error.code(),
        "sqlite_error" | "store_corrupted" | "store_unavailable" | "migration_checksum_mismatch"
    )
}

pub(super) fn map_status_error(error: StatusError) -> RuntimeError {
    match error {
        StatusError::Store(error) => RuntimeError::Store(error),
    }
}

#[cfg(test)]
mod tests {
    use super::build_snapshot;
    use crate::migration::MigrationSnapshot;
    use crate::{ComponentState, DiagnosticLevel, RuntimeState, Supervisor};
    use agentnotify_application::StatusOverview;

    /// 回归：CI 上 `production_contract` 曾偶发「状态 Running 但组件列表为空」。
    /// 唯一来源是组件表锁中毒被静默吞掉（PR #15 之前的 `unwrap_or_default()` 返回空表），
    /// 这里把「中毒后快照仍报告组件」钉死在快照层，避免以后重构把恢复逻辑弄丢。
    #[test]
    fn poisoned_component_lock_still_reports_components_in_snapshot() {
        let supervisor = Supervisor::new();
        supervisor.set_state("delivery.outbox", ComponentState::Running);
        supervisor.poison_components_lock();

        let snapshot = build_snapshot(
            "2.0.7-test",
            "windows",
            RuntimeState::Running,
            StatusOverview::default(),
            &supervisor,
            &MigrationSnapshot::not_configured(),
        );

        assert!(
            !snapshot.components.is_empty(),
            "锁中毒后组件表不能被清空，否则诊断页会显示成「没有任何后台组件」"
        );
        let components_item = snapshot
            .diagnostics
            .iter()
            .find(|item| item.code == "components")
            .expect("快照必须包含组件诊断项");
        assert_eq!(
            components_item.level,
            DiagnosticLevel::Ok,
            "组件表可读时诊断项必须是 Ok，而不是把异常吞掉"
        );
    }
}
