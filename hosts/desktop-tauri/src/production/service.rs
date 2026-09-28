use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use agentnotify_agent_sdk::AgentEventEnvelope;
use agentnotify_application::{ChannelAccountStore, IngestResult, StatusStore};
use agentnotify_channel_clawbot::CLAWBOT_CHANNEL_ID;
use agentnotify_channel_sdk::{BeginLoginRequest, ChannelLoginAdapter, LoginSessionId};
use agentnotify_domain::{
    AgentId, ChannelAccountId, DeliveryId, DeliveryState, NotificationId, RequestId, Timestamp,
};
use agentnotify_storage_sqlite::{NotificationQuery, SqliteStore};
use tauri::{AppHandle, Wry};

use super::agent_driver::ProductionAgentDriver;

use super::app_exit::{ProductionAppExitRequester, spawn_graceful_exit};
use super::events::map_delivery_state;
use super::mapping::{
    install_result_from_report, map_delivery_view_record, map_login_session_dto,
    map_migration_snapshot, map_notification_record, sanitize_account_config,
    update_state_for_error,
};
use super::orc_handler::{OrcCommandHandler, load_harness_templates};
use super::orc_notify::ProductionOrcPresenter;
use super::runtime::ProductionRuntimeCoordinator;
use super::settings::ProductionSettingsStore;
use crate::bridge::commands::{ChannelCommands, HostCommandService};
use crate::bridge::dto::*;
use crate::bridge::error::CommandError;
use crate::lifecycle::{
    autostart::{TauriCurrentUserAutostart, set_autostart},
    tray::sync_tray_paused,
};
use crate::update::{SystemInstallerLauncher, UpdateChannel, UpdateService};

// 命令域子模块（2026-09 从单文件拆分，纯搬移）：
// `HostCommandService` 各域 impl 片段分布在本模块子树内，共享父模块导入。
mod channels;
mod diagnostics;
mod notifications;
mod settings;
mod update;
pub struct ProductionHostCommandService {
    app: Option<AppHandle<Wry>>,
    runtime: Arc<ProductionRuntimeCoordinator>,
    store: Arc<SqliteStore>,
    settings: ProductionSettingsStore,
    updates: Arc<UpdateService>,
    orchestration: OrcCommandHandler,
}
impl ProductionHostCommandService {
    pub async fn new(
        app: Option<AppHandle<Wry>>,
        runtime: Arc<ProductionRuntimeCoordinator>,
        store: Arc<SqliteStore>,
        settings: ProductionSettingsStore,
        config_dir: &Path,
        updates: Arc<UpdateService>,
        // 是否装配派活链路（AgentDriver）：生产 true 用信封唤醒真实 Agent；headless/测试 false（纯状态推进）。
        enable_agent_driver: bool,
    ) -> Self {
        let orchestration = OrcCommandHandler::with_selector(
            None, // 动态模式：按 settings 实时解析 enabled（默认开启）+ workflow，无需重启。
            store.clone(),
            settings.clone(),
            load_harness_templates(config_dir),
            Some(Arc::new(ProductionOrcPresenter::new(
                settings.clone(),
                runtime.target_provider(),
                runtime.channel_registry(),
                Some(store.clone()),
            ))),
            // P2 派活：推进/创建任务时把信封交给工作流配置的真实 Agent；生产才有真实驱动。
            if enable_agent_driver {
                Some(Arc::new(ProductionAgentDriver::new(
                    runtime.agent_registry(),
                )))
            } else {
                None
            },
        );
        Self {
            app,
            runtime,
            store,
            settings,
            updates,
            orchestration,
        }
    }

    pub fn runtime(&self) -> Arc<ProductionRuntimeCoordinator> {
        self.runtime.clone()
    }
}
#[async_trait::async_trait]
impl HostCommandService for ProductionHostCommandService {
    async fn get_snapshot(
        &self,
        _payload: EmptyPayload,
    ) -> Result<RuntimeSnapshotDto, CommandError> {
        let snapshot = match self.runtime.current_snapshot().await {
            Some(s) => s,
            None => self.runtime.start_or_restart().await?,
        };

        let agents = self.list_agents(EmptyPayload {}).await?;
        let channel_list = self.list_channel_accounts(EmptyPayload {}).await?;
        let mut all_accounts = Vec::new();
        for ch in channel_list.channels {
            all_accounts.extend(ch.accounts);
        }

        let recent_delivery_records = self.store.recent_deliveries(10).await.unwrap_or_default();
        let recent_deliveries = recent_delivery_records
            .into_iter()
            .map(map_delivery_view_record)
            .collect();

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

        let summary = RuntimeSummaryDto {
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
        let diagnostics = snapshot
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

        Ok(RuntimeSnapshotDto {
            runtime: summary,
            overview: SnapshotOverviewDto {
                storage,
                agents,
                channels: all_accounts,
                recent_deliveries,
            },
            components,
            diagnostics,
            migration,
        })
    }
    async fn list_agents(&self, _payload: EmptyPayload) -> Result<Vec<AgentDto>, CommandError> {
        let configs = self
            .store
            .agent_configs()
            .await
            .map_err(|e| CommandError::new("agent_configs_query_failed", e.to_string()))?;

        let adapters = self.runtime.agent_registry().all();
        let mut result = Vec::new();

        for adapter in adapters {
            let desc = adapter.descriptor();
            let caps = adapter.capabilities();
            let agent_id_str = desc.id.to_string();
            let record = configs.get(&agent_id_str);

            // 未配置时默认保守启用
            let enabled = record.map(|r| r.enabled).unwrap_or(true);
            let config = record
                .map(|r| r.config.clone())
                .unwrap_or(serde_json::Value::Null);

            let health = adapter.inspect().await;
            let available = health.available;
            let detail = health.detail.map(|d| SafeErrorDto {
                code: d.code().to_string(),
                message: d.message().to_string(),
            });

            result.push(AgentDto {
                id: agent_id_str,
                display_name: desc.display_name,
                description: desc.description,
                config_schema: desc.config_schema,
                capabilities: AgentCapabilitiesDto {
                    notify: caps.notify,
                    resume: caps.resume,
                    session_title: caps.session_title,
                    hook_installer: caps.hook_installer,
                    reply_window: caps.reply_window,
                },
                enabled,
                config,
                health: AgentHealthDto { available, detail },
            });
        }

        Ok(result)
    }

    async fn update_agent_config(
        &self,
        payload: UpdateAgentConfigPayload,
    ) -> Result<AgentDto, CommandError> {
        let configs = self
            .store
            .agent_configs()
            .await
            .map_err(|e| CommandError::new("agent_configs_query_failed", e.to_string()))?;

        let existing = configs.get(&payload.agent_id);
        let current_enabled = existing.map(|r| r.enabled).unwrap_or(true);
        let current_config = existing
            .map(|r| r.config.clone())
            .unwrap_or(serde_json::json!({}));

        let new_enabled = payload.enabled.unwrap_or(current_enabled);
        let new_config = payload.config.unwrap_or(current_config);

        // 校验通过才落库并重建注册表：无效配置会在这里报错，数据库保持原样
        self.runtime
            .update_agent_config(&payload.agent_id, new_enabled, &new_config)
            .await?;

        let agents = self.list_agents(EmptyPayload {}).await?;
        agents
            .into_iter()
            .find(|a| a.id == payload.agent_id)
            .ok_or_else(|| CommandError::new("agent_not_found", "找不到已更新的 Agent"))
    }
    async fn create_orc_task(
        &self,
        payload: CreateOrcTaskPayload,
    ) -> Result<OrcTaskDto, CommandError> {
        self.orchestration.create(payload).await
    }

    async fn list_orc_tasks(
        &self,
        _payload: EmptyPayload,
    ) -> Result<Vec<OrcTaskDto>, CommandError> {
        self.orchestration.list().await
    }

    async fn advance_orc_task(
        &self,
        payload: AdvanceOrcTaskPayload,
    ) -> Result<OrcTaskDto, CommandError> {
        self.orchestration.advance(payload).await
    }

    async fn mark_blocked_orc_task(
        &self,
        payload: MarkBlockedOrcTaskPayload,
    ) -> Result<OrcTaskDto, CommandError> {
        self.orchestration.mark_blocked(payload).await
    }

    async fn recover_blocked_orc_task(
        &self,
        payload: OrcTaskIdPayload,
    ) -> Result<OrcTaskDto, CommandError> {
        self.orchestration.recover_blocked(payload).await
    }

    async fn start_orc_task(&self, payload: OrcTaskIdPayload) -> Result<OrcTaskDto, CommandError> {
        self.orchestration.start(payload).await
    }

    async fn get_current_orc_workflow(
        &self,
        _payload: EmptyPayload,
    ) -> Result<CurrentOrcWorkflowDto, CommandError> {
        self.orchestration.current_workflow().await
    }
}
