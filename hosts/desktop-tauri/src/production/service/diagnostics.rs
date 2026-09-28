//! 诊断命令域（`HostCommandService` 的 `diagnostics` 片段，2026-09 从 service.rs 拆分）。

use super::*;
use crate::bridge::commands::DiagnosticsCommands;

#[async_trait::async_trait]
impl DiagnosticsCommands for ProductionHostCommandService {
    async fn get_diagnostics(
        &self,
        _payload: EmptyPayload,
    ) -> Result<DiagnosticsDto, CommandError> {
        let snapshot = match self.runtime.current_snapshot().await {
            Some(s) => s,
            None => self.runtime.start_or_restart().await?,
        };

        let storage_snapshot = StatusStore::snapshot(&*self.store)
            .await
            .unwrap_or_default();
        let storage = StorageStatusDto {
            notification_count: storage_snapshot.notification_count as u32,
            delivery_count: storage_snapshot.delivery_count as u32,
            pending_outbox_count: storage_snapshot.pending_outbox_count as u32,
            recent_error: storage_snapshot.recent_error.map(|e| SafeErrorDto {
                code: e.code().to_string(),
                message: e.message().to_string(),
            }),
        };

        let paused = self.runtime.is_outbox_paused().await;
        let lifecycle_state = if snapshot.migration.state
            == agentnotify_runtime::MigrationState::Required
        {
            RuntimeLifecycleStateDto::MigrationRequired
        } else if paused {
            RuntimeLifecycleStateDto::Paused
        } else {
            match snapshot.state {
                agentnotify_runtime::RuntimeState::Starting => RuntimeLifecycleStateDto::Starting,
                agentnotify_runtime::RuntimeState::Running
                | agentnotify_runtime::RuntimeState::Degraded => RuntimeLifecycleStateDto::Running,
                agentnotify_runtime::RuntimeState::Stopping => RuntimeLifecycleStateDto::Stopping,
                agentnotify_runtime::RuntimeState::Stopped => RuntimeLifecycleStateDto::Stopped,
                agentnotify_runtime::RuntimeState::Failed => RuntimeLifecycleStateDto::Failed,
                agentnotify_runtime::RuntimeState::MigrationRequired => {
                    RuntimeLifecycleStateDto::MigrationRequired
                }
            }
        };

        let runtime_summary = RuntimeSummaryDto {
            app_version: snapshot.app_version.clone(),
            platform: snapshot.platform.clone(),
            state: lifecycle_state,
            paused,
        };

        let components = snapshot
            .components
            .into_iter()
            .map(|c| ComponentDto {
                name: c.name.to_string(),
                state: match c.state {
                    agentnotify_runtime::ComponentState::Starting => ComponentStateDto::Starting,
                    agentnotify_runtime::ComponentState::Running => ComponentStateDto::Running,
                    agentnotify_runtime::ComponentState::Stopping
                    | agentnotify_runtime::ComponentState::Stopped => ComponentStateDto::Stopped,
                    agentnotify_runtime::ComponentState::Failed => ComponentStateDto::Failed,
                    agentnotify_runtime::ComponentState::Unknown => ComponentStateDto::Failed,
                },
                detail: c.last_error.map(|d| SafeErrorDto {
                    code: d.code().to_string(),
                    message: d.message().to_string(),
                }),
            })
            .collect();

        let now_str = Timestamp::now_utc().to_rfc3339();
        let items = snapshot
            .diagnostics
            .into_iter()
            .map(|d| DiagnosticItemDto {
                code: d.code,
                level: match d.level {
                    agentnotify_runtime::DiagnosticLevel::Ok => DiagnosticLevelDto::Normal,
                    agentnotify_runtime::DiagnosticLevel::Warning => DiagnosticLevelDto::Waiting,
                    agentnotify_runtime::DiagnosticLevel::Error => DiagnosticLevelDto::Error,
                },
                message: d.message,
                checked_at: now_str.clone(),
                action: None,
            })
            .collect();

        let migration = map_migration_snapshot(&snapshot.migration);

        Ok(DiagnosticsDto {
            generated_at: Timestamp::now_utc().to_rfc3339(),
            runtime: runtime_summary,
            storage,
            components,
            items,
            migration,
        })
    }

    async fn retry_legacy_migration(
        &self,
        _payload: EmptyPayload,
    ) -> Result<LegacyMigrationDto, CommandError> {
        let snapshot = self.runtime.retry_legacy_migration().await?;
        Ok(map_migration_snapshot(&snapshot.migration))
    }
}
