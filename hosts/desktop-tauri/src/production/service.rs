use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use agentnotify_agent_sdk::AgentEventEnvelope;
use agentnotify_application::{ChannelAccountStore, IngestResult, StatusStore};
use agentnotify_channel_clawbot::CLAWBOT_CHANNEL_ID;
use agentnotify_channel_sdk::{BeginLoginRequest, ChannelLoginAdapter, LoginSessionId};
use agentnotify_domain::{
    AgentId, AgentSessionId, ChannelAccountId, DeliveryId, DeliveryState, NotificationId,
    RequestId, Timestamp,
};
use agentnotify_orchestration::{
    MessageKind, NotifyMode, OrcError, OrcStore, OrcTask, StepOutcome, TaskState, TemplateResolver,
    TransitionAction, Workflow,
};
use agentnotify_storage_sqlite::{NotificationQuery, SqliteStore};
use tauri::{AppHandle, Wry};

use super::agent_driver::{AgentDriver, ProductionAgentDriver};

use super::app_exit::{ProductionAppExitRequester, spawn_graceful_exit};
use super::events::map_delivery_state;
use super::mapping::{
    install_result_from_report, map_delivery_view_record, map_login_session_dto,
    map_migration_snapshot, map_notification_record, sanitize_account_config,
    update_state_for_error,
};
use super::orc_notify::{
    OrcClusterPresenter, ProductionOrcPresenter, failure_body, progress_body,
    render_cluster_message, should_notify,
};
use super::orc_wechat_route::state_cn;
use super::runtime::ProductionRuntimeCoordinator;
use super::settings::ProductionSettingsStore;
use crate::bridge::commands::HostCommandService;
use crate::bridge::dto::*;
use crate::bridge::error::CommandError;
use crate::lifecycle::{
    autostart::{TauriCurrentUserAutostart, set_autostart},
    tray::sync_tray_paused,
};
use crate::update::{SystemInstallerLauncher, UpdateChannel, UpdateService};

/// 测试发送后等待 Delivery 落库并达到终态的上限。
const DELIVERY_FINAL_STATE_TIMEOUT: Duration = Duration::from_secs(5);
/// 等待 Delivery 终态期间的 SQLite 轮询间隔。
const DELIVERY_FINAL_STATE_POLL: Duration = Duration::from_millis(150);

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
        let orchestration = OrcCommandHandler::with_driver(
            orchestration_store(&store, &settings).await,
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

    async fn list_channel_accounts(
        &self,
        _payload: EmptyPayload,
    ) -> Result<ChannelListDto, CommandError> {
        let adapters = self.runtime.channel_registry().all();
        let mut channels = Vec::new();

        for adapter in adapters {
            let desc = adapter.descriptor();
            let caps = adapter.capabilities();
            let channel_id = desc.id.clone();

            let account_records =
                self.store.list(&channel_id).await.map_err(|e| {
                    CommandError::new("channel_accounts_query_failed", e.to_string())
                })?;

            let mut accounts = Vec::new();
            for account in account_records {
                let health = adapter.inspect(account.clone()).await;
                let health_dto = ChannelHealthDto {
                    available: health.available,
                    stale: health.stale,
                    detail: health.detail.map(|d| SafeErrorDto {
                        code: d.code().to_string(),
                        message: d.message().to_string(),
                    }),
                };

                // 脱敏账号配置
                let safe_config = sanitize_account_config(&account.config);

                accounts.push(ChannelAccountDto {
                    id: account.id.to_string(),
                    channel_id: account.channel_id.to_string(),
                    display_name: account.display_name,
                    enabled: account.enabled,
                    config: safe_config,
                    health: health_dto,
                    last_inbound_at: None,
                    last_delivery_at: None,
                });
            }

            channels.push(ChannelDto {
                id: channel_id.to_string(),
                display_name: desc.display_name,
                config_schema: desc.config_schema,
                capabilities: ChannelCapabilitiesDto {
                    send_text: caps.send_text,
                    receive: caps.receive,
                    reply_routing: caps.reply_routing,
                    edit_message: caps.edit_message,
                    attachments: caps.attachments,
                    markdown: caps.markdown,
                    max_text_bytes: caps.max_text_bytes.map(|v| v as u32),
                    inbound_modes: caps
                        .inbound_modes
                        .iter()
                        .map(|m| match m {
                            agentnotify_channel_sdk::InboundMode::LongPolling => {
                                "long_polling".into()
                            }
                            agentnotify_channel_sdk::InboundMode::WebSocket => "websocket".into(),
                            agentnotify_channel_sdk::InboundMode::Webhook => "webhook".into(),
                            agentnotify_channel_sdk::InboundMode::LocalEvent => {
                                "local_event".into()
                            }
                        })
                        .collect(),
                },
                accounts,
            });
        }

        Ok(ChannelListDto { channels })
    }

    async fn begin_channel_login(
        &self,
        payload: BeginChannelLoginPayload,
    ) -> Result<BeginChannelLoginResultDto, CommandError> {
        if payload.channel_id != CLAWBOT_CHANNEL_ID {
            return Err(CommandError::new(
                "channel_login_unsupported",
                format!("当前不支持渠道 {} 的扫码登录", payload.channel_id),
            ));
        }

        let req = BeginLoginRequest::new(payload.account_key);
        let session = self
            .runtime
            .login_adapter()
            .begin_login(req)
            .await
            .map_err(|error| {
                CommandError::new(error.code(), format!("发起渠道登录失败：{error}"))
            })?;

        let session_dto = map_login_session_dto(&session);
        Ok(BeginChannelLoginResultDto {
            channel_id: payload.channel_id,
            session: session_dto,
        })
    }

    async fn submit_channel_login_code(
        &self,
        payload: SubmitChannelLoginCodePayload,
    ) -> Result<LoginSessionDto, CommandError> {
        let session_id = LoginSessionId::new(payload.session_id).map_err(|error| {
            CommandError::new("invalid_session_id", format!("会话标识无效：{error}"))
        })?;

        let session = self
            .runtime
            .login_adapter()
            .submit_login_code(&session_id, &payload.code)
            .await
            .map_err(|error| {
                CommandError::new(error.code(), format!("提交登录验证码失败：{error}"))
            })?;

        Ok(map_login_session_dto(&session))
    }

    async fn logout_channel_account(
        &self,
        payload: ChannelAccountIdPayload,
    ) -> Result<MutationAcceptedDto, CommandError> {
        let account_id = ChannelAccountId::new(&payload.account_id)
            .map_err(|e| CommandError::new("invalid_account_id", e.to_string()))?;

        let account_opt = self
            .store
            .get(&account_id)
            .await
            .map_err(|e| CommandError::new("account_query_failed", e.to_string()))?;

        let Some(mut account) = account_opt else {
            return Err(CommandError::new("account_not_found", "找不到指定渠道账号"));
        };

        if let Some(channel) = self.runtime.channel_registry().get(&account.channel_id) {
            channel.logout(account.clone()).await.map_err(|error| {
                CommandError::new(error.code(), format!("渠道账号登出失败：{error}"))
            })?;
        }

        account.enabled = false;
        self.store
            .upsert(account)
            .await
            .map_err(|e| CommandError::new("account_save_failed", e.to_string()))?;

        self.runtime.start_or_restart().await?;

        Ok(MutationAcceptedDto {
            accepted: true,
            id: Some(payload.account_id),
        })
    }

    async fn enable_channel_account(
        &self,
        payload: ChannelAccountIdPayload,
    ) -> Result<ChannelAccountDto, CommandError> {
        self.set_channel_account_enabled(&payload.account_id, true)
            .await
    }

    async fn disable_channel_account(
        &self,
        payload: ChannelAccountIdPayload,
    ) -> Result<ChannelAccountDto, CommandError> {
        self.set_channel_account_enabled(&payload.account_id, false)
            .await
    }

    async fn send_test_notification(
        &self,
        payload: SendTestNotificationPayload,
    ) -> Result<TestNotificationResultDto, CommandError> {
        let title = payload.title.trim();
        let body = payload.body.trim();
        if title.is_empty() {
            return Err(CommandError::new("title_empty", "测试通知标题不能为空"));
        }
        if body.is_empty() {
            return Err(CommandError::new("body_empty", "测试通知内容不能为空"));
        }
        if payload.account_id.trim().is_empty() {
            return Err(CommandError::new(
                "account_empty",
                "请指定发送测试的目标账号",
            ));
        }

        let agent_id = AgentId::new("opencode").expect("固定有效标识");
        let request_id =
            RequestId::new(format!("req-test-{}", uuid::Uuid::new_v4())).expect("有效请求标识");

        let event_payload = serde_json::json!({
            "eventType": "session.completed",
            "sessionId": format!("test-session-{}", uuid::Uuid::new_v4()),
            "title": title,
            "body": body,
            "metadata": {
                "targetAccountId": payload.account_id
            }
        });

        let envelope = AgentEventEnvelope {
            request_id,
            agent_id,
            payload: event_payload,
        };

        let ingest_res = self.runtime.ingest(envelope).await?;
        let notification_id = match ingest_res {
            IngestResult::Queued { notification_id }
            | IngestResult::Duplicate { notification_id } => notification_id,
            IngestResult::Skipped { reason } => {
                return Err(CommandError::new(
                    reason.as_str(),
                    format!("测试通知被策略跳过：{}", reason.as_str()),
                ));
            }
        };

        // 轮询 SQLite 等待 Delivery 产生并达到终态或最多等待 5 秒
        let mut final_delivery = None;
        let start_time = std::time::Instant::now();
        let timeout = DELIVERY_FINAL_STATE_TIMEOUT;

        while start_time.elapsed() < timeout {
            let detail = self
                .store
                .notification_detail(notification_id.clone())
                .await
                .ok()
                .flatten();
            if let Some(detail_record) = detail {
                if let Some(delivery_record) = detail_record.deliveries.first() {
                    let state = delivery_record.delivery.state();
                    if state != DeliveryState::Pending {
                        final_delivery = Some(map_delivery_view_record(delivery_record.clone()));
                        break;
                    }
                }
            }
            tokio::time::sleep(DELIVERY_FINAL_STATE_POLL).await;
        }

        // 如果超时但产生了 delivery，也返回当前记录
        if final_delivery.is_none() {
            if let Ok(Some(detail)) = self
                .store
                .notification_detail(notification_id.clone())
                .await
            {
                if let Some(delivery_record) = detail.deliveries.first() {
                    final_delivery = Some(map_delivery_view_record(delivery_record.clone()));
                }
            }
        }

        Ok(TestNotificationResultDto {
            accepted: true,
            delivery: final_delivery,
        })
    }

    async fn list_notifications(
        &self,
        payload: NotificationFilterPayload,
    ) -> Result<NotificationListDto, CommandError> {
        let from_ts = payload
            .from
            .as_deref()
            .and_then(|s| Timestamp::parse_rfc3339(s).ok());
        let to_ts = payload
            .to
            .as_deref()
            .and_then(|s| Timestamp::parse_rfc3339(s).ok());

        let query = NotificationQuery {
            agent_id: payload.agent_id,
            channel_id: payload.channel_id,
            account_id: payload.account_id,
            delivery_state: payload.delivery_state.map(|s| match s {
                DeliveryStateDto::Pending => DeliveryState::Pending,
                DeliveryStateDto::Sent => DeliveryState::Sent,
                DeliveryStateDto::Failed => DeliveryState::Failed,
                DeliveryStateDto::Unknown => DeliveryState::Unknown,
                DeliveryStateDto::Skipped => DeliveryState::Skipped,
            }),
            from: from_ts,
            to: to_ts,
            search: payload.query,
            cursor: payload.cursor,
            limit: payload.limit,
        };

        let page = self
            .store
            .notification_page(query)
            .await
            .map_err(|e| CommandError::new("notification_query_failed", e.to_string()))?;

        let items = page
            .items
            .into_iter()
            .map(map_notification_record)
            .collect();

        Ok(NotificationListDto {
            items,
            total: page.total,
            next_cursor: page.next_cursor,
        })
    }

    async fn get_notification_detail(
        &self,
        payload: NotificationIdPayload,
    ) -> Result<NotificationDetailDto, CommandError> {
        let id = NotificationId::new(&payload.notification_id)
            .map_err(|e| CommandError::new("invalid_notification_id", e.to_string()))?;

        let record = self
            .store
            .notification_detail(id)
            .await
            .map_err(|e| CommandError::new("notification_detail_query_failed", e.to_string()))?
            .ok_or_else(|| CommandError::new("notification_not_found", "找不到指定通知记录"))?;

        let summary = NotificationSummaryDto {
            id: record.notification.id.to_string(),
            agent_id: record.notification.agent_id.to_string(),
            session_id: record
                .notification
                .session_id
                .as_ref()
                .map(|s| s.to_string()),
            session_title: record.notification.session_title.clone(),
            title: record.notification.title.clone(),
            preview: record.notification.body.chars().take(100).collect(),
            occurred_at: record.notification.occurred_at.to_rfc3339(),
            delivery_states: record
                .deliveries
                .iter()
                .map(|d| map_delivery_state(d.delivery.state()))
                .collect(),
        };

        let deliveries = record
            .deliveries
            .into_iter()
            .map(map_delivery_view_record)
            .collect();

        Ok(NotificationDetailDto {
            notification: summary,
            body: record.notification.body,
            metadata: record.notification.metadata.clone().into_inner(),
            deliveries,
            route_exists: record.route_exists,
        })
    }

    async fn retry_delivery(
        &self,
        payload: DeliveryIdPayload,
    ) -> Result<DeliveryDto, CommandError> {
        let id = DeliveryId::new(&payload.delivery_id)
            .map_err(|e| CommandError::new("invalid_delivery_id", e.to_string()))?;

        // 重新排队 outbox 并获取更新前记录
        let record = self
            .store
            .requeue_delivery(id)
            .await
            .map_err(|e| CommandError::new(e.code(), format!("重试投递失败：{e}")))?;

        // 重新查询该通知对应的最新投递记录
        let latest = self
            .store
            .notification_detail(record.delivery.notification_id().clone())
            .await
            .map_err(|e| CommandError::new(e.code(), format!("查询最新投递状态失败：{e}")))?
            .and_then(|detail| {
                detail.deliveries.into_iter().find(|item| {
                    item.delivery.channel_id() == record.delivery.channel_id()
                        && item.delivery.account_id() == record.delivery.account_id()
                })
            })
            .unwrap_or(record);

        Ok(map_delivery_view_record(latest))
    }

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

    async fn get_settings(&self, _payload: EmptyPayload) -> Result<SettingsDto, CommandError> {
        let mut settings = self.settings.load_settings().await?;
        settings.notifications_paused = self.runtime.is_outbox_paused().await;
        Ok(settings)
    }

    async fn update_settings(&self, payload: SettingsDto) -> Result<SettingsDto, CommandError> {
        let current = self.settings.load_settings().await?;

        // 1. 若当前用户自启动发生变化，调用系统自启动服务
        if let Some(app) = &self.app {
            if current.auto_start != payload.auto_start {
                let autostart = TauriCurrentUserAutostart::new(app.clone());
                set_autostart(&autostart, payload.auto_start)
                    .map_err(|e| CommandError::new(e.code(), e.to_string()))?;
            }
        }

        self.settings.save_settings(&payload).await?;
        self.runtime
            .set_outbox_paused(payload.notifications_paused)
            .await?;

        // 2. 若暂停状态变更，同步托盘菜单与事件
        if let Some(app) = &self.app {
            if current.notifications_paused != payload.notifications_paused {
                let _ = sync_tray_paused(app, payload.notifications_paused);
            }
        }

        // 3. 仅在关键运行时配置发生变化时重启 runtime
        let needs_runtime_restart = current.reply_enabled != payload.reply_enabled
            || current.delivery_receipt_enabled != payload.delivery_receipt_enabled
            || current.route_ttl_seconds != payload.route_ttl_seconds
            || current.quiet_hours != payload.quiet_hours
            || current.cooldown_seconds != payload.cooldown_seconds
            || current.default_channel_account_id != payload.default_channel_account_id;

        if needs_runtime_restart {
            self.runtime.start_or_restart().await?;
        }

        self.get_settings(EmptyPayload {}).await
    }

    async fn set_runtime_paused(
        &self,
        payload: SetRuntimePausedPayload,
    ) -> Result<RuntimeSummaryDto, CommandError> {
        let current_paused = self.runtime.is_outbox_paused().await;
        if current_paused == payload.paused {
            let snapshot = match self.runtime.current_snapshot().await {
                Some(s) => s,
                None => self.runtime.start_or_restart().await?,
            };
            return Ok(RuntimeSummaryDto {
                app_version: snapshot.app_version,
                platform: snapshot.platform,
                state: if payload.paused {
                    RuntimeLifecycleStateDto::Paused
                } else {
                    RuntimeLifecycleStateDto::Running
                },
                paused: payload.paused,
            });
        }

        // 先持久化到 settings 表
        self.settings
            .save_settings(&SettingsDto {
                notifications_paused: payload.paused,
                ..self.settings.load_settings().await?
            })
            .await?;

        // 再控制 Outbox 门控；若失败，尝试回滚设置
        if let Err(error) = self.runtime.set_outbox_paused(payload.paused).await {
            let _ = self
                .settings
                .save_settings(&SettingsDto {
                    notifications_paused: current_paused,
                    ..self.settings.load_settings().await?
                })
                .await;
            return Err(error);
        }

        // 同步托盘菜单与托盘事件
        if let Some(app) = &self.app {
            let _ = sync_tray_paused(app, payload.paused);
        }

        let snapshot = match self.runtime.current_snapshot().await {
            Some(s) => s,
            None => self.runtime.start_or_restart().await?,
        };

        Ok(RuntimeSummaryDto {
            app_version: snapshot.app_version,
            platform: snapshot.platform,
            state: if payload.paused {
                RuntimeLifecycleStateDto::Paused
            } else {
                RuntimeLifecycleStateDto::Running
            },
            paused: payload.paused,
        })
    }

    async fn quit_app(&self, _payload: EmptyPayload) -> Result<MutationAcceptedDto, CommandError> {
        if let Some(app) = &self.app {
            spawn_graceful_exit(
                app.clone(),
                self.runtime.clone(),
                self.store.clone(),
                Duration::ZERO,
            );
        } else {
            let _ = self.runtime.shutdown_runtime().await;
            let _ = self.store.wal_checkpoint_truncate().await;
        }

        Ok(MutationAcceptedDto {
            accepted: true,
            id: None,
        })
    }

    async fn get_update_status(
        &self,
        _payload: EmptyPayload,
    ) -> Result<UpdateStatusDto, CommandError> {
        let current_version = self.current_app_version().await;
        let channel = self.update_channel().await;
        let preview = channel.is_preview();
        let checked_at = Some(Timestamp::now_utc().to_rfc3339());

        Ok(
            match self.updates.check_latest(&current_version, channel).await {
                Ok(Some(release)) => UpdateStatusDto {
                    current_version,
                    available_version: Some(release.version.clone()),
                    state: UpdateStateDto::Available,
                    signed: false,
                    preview,
                    message: format!("发现新版本 v{}，可下载并安装。", release.version),
                    checked_at,
                },
                Ok(None) => UpdateStatusDto {
                    current_version,
                    available_version: None,
                    state: UpdateStateDto::UpToDate,
                    signed: false,
                    preview,
                    message: "当前已是最新版本。".into(),
                    checked_at,
                },
                Err(error) => UpdateStatusDto {
                    current_version,
                    available_version: None,
                    state: update_state_for_error(&error),
                    signed: false,
                    preview,
                    message: error.message().to_owned(),
                    checked_at,
                },
            },
        )
    }

    async fn install_update(
        &self,
        _payload: InstallUpdatePayload,
    ) -> Result<InstallUpdateResultDto, CommandError> {
        let current_version = self.current_app_version().await;
        let channel = self.update_channel().await;
        let preview = channel.is_preview();
        let install_root = current_install_root()?;
        let launcher = SystemInstallerLauncher;
        let exit = ProductionAppExitRequester::new(
            self.app.clone(),
            self.runtime.clone(),
            self.store.clone(),
        );

        Ok(
            match self
                .updates
                .install_latest(&current_version, channel, &install_root, &launcher, &exit)
                .await
            {
                Ok(report) => install_result_from_report(report),
                // 安装失败不抛异常：用 DTO 的 Failed 状态把中文原因交给界面展示。
                Err(error) => InstallUpdateResultDto {
                    state: update_state_for_error(&error),
                    message: error.message().to_owned(),
                    installed_version: None,
                    signed: false,
                    preview,
                },
            },
        )
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
}

impl ProductionHostCommandService {
    async fn current_app_version(&self) -> String {
        match self.runtime.current_snapshot().await {
            Some(snapshot) => snapshot.app_version,
            None => env!("CARGO_PKG_VERSION").to_string(),
        }
    }

    /// 更新通道来自设置；读取失败按正式通道处理（更保守，必须校验签名指纹）。
    async fn update_channel(&self) -> UpdateChannel {
        match self.settings.load_settings().await {
            Ok(settings) => match settings.update_channel {
                UpdateChannelDto::Stable => UpdateChannel::Stable,
                UpdateChannelDto::Beta => UpdateChannel::Beta,
            },
            Err(_) => UpdateChannel::Stable,
        }
    }

    async fn set_channel_account_enabled(
        &self,
        account_id_str: &str,
        enabled: bool,
    ) -> Result<ChannelAccountDto, CommandError> {
        let account_id = ChannelAccountId::new(account_id_str)
            .map_err(|e| CommandError::new("invalid_account_id", e.to_string()))?;

        let account_opt = self
            .store
            .get(&account_id)
            .await
            .map_err(|e| CommandError::new("account_query_failed", e.to_string()))?;

        let Some(mut account) = account_opt else {
            return Err(CommandError::new("account_not_found", "找不到指定渠道账号"));
        };

        account.enabled = enabled;
        self.store
            .upsert(account.clone())
            .await
            .map_err(|e| CommandError::new("account_save_failed", e.to_string()))?;

        self.runtime.start_or_restart().await?;

        let list = self.list_channel_accounts(EmptyPayload {}).await?;
        for channel in list.channels {
            for acc in channel.accounts {
                if acc.id == account_id_str {
                    return Ok(acc);
                }
            }
        }

        Err(CommandError::new("account_not_found", "更新后找不到账号"))
    }
}

/// 编排命令处理器（P1-1，B 方案接线）。
///
/// 持有可选的 [`OrcStore`]：`orchestration.enabled` 开启时才装配 SQLite 仓储（§8.5 默认关闭）；
/// 未启用时所有编排命令返回明确错误（`orchestration_disabled`），对既有功能零影响。
pub struct OrcCommandHandler {
    store: Option<OrcStore>,
    /// harness 模板解析器（P1-5）：用户模板优先、内置默认兜底；派活时由此生成任务信封。
    templates: TemplateResolver,
    /// 集群消息呈现（P1-4）：缺省不呈现（行为与 P1-3 一致）；注入后 advance/mark_blocked 按通知节奏外发。
    presenter: Option<Arc<dyn OrcClusterPresenter>>,
    /// 派活驱动器（P2）：缺省不派活（行为与 P1-3/1-4 一致）；注入后 create/advance
    /// 把当前 Step 的任务信封真正交给配置的 Agent。
    driver: Option<Arc<dyn AgentDriver>>,
}

/// 编排开关设置键（settings 表），默认关闭。
pub const KEY_ORCHESTRATION_ENABLED: &str = "orchestration.enabled";
/// 全局默认通知节奏设置键（settings 表，P1-4 §4.6）：缺失/非法回退 `final_only` 并告警。
pub const KEY_ORCHESTRATION_NOTIFY_MODE: &str = "orchestration.notify_mode";
/// 用户 harness 模板配置文件（`config_dir` 下，§4.3 / P1-5；缺失 = 内置默认兜底）。
pub const HARNESS_TEMPLATES_FILE: &str = "harness-templates.json";
const ORCHESTRATION_DISABLED_CODE: &str = "orchestration_disabled";
const ORCHESTRATION_DISABLED_MESSAGE: &str =
    "编排未启用：请在设置中启用 orchestration.enabled 后重启应用";
/// 当前步骤未配置 Agent（agent_hint 缺失）时的稳定错误码（P2 派活，写进 blocked 原因）。
const ORC_STEP_AGENT_MISSING: &str = "orc_step_agent_missing";
/// 派活信封会话 id 前缀：`task-<task_id>-step-<n>`（每个 (task, step) 一个稳定会话）。
const ORC_DISPATCH_SESSION_PREFIX: &str = "task";

impl OrcCommandHandler {
    /// 默认装配：仅内置默认信封模板（向后兼容）。
    pub fn new(store: Option<OrcStore>) -> Self {
        Self::with_templates(store, TemplateResolver::new())
    }

    /// 装配用户 harness 模板解析器（用户模板优先、内置默认兜底，§4.3 / P1-5）。
    pub fn with_templates(store: Option<OrcStore>, templates: TemplateResolver) -> Self {
        Self {
            store,
            templates,
            presenter: None,
            driver: None,
        }
    }

    /// 装配集群消息呈现（P1-4）：注入生产实现后，任务创建继承全局默认通知节奏，
    /// advance / mark_blocked 按 §4.6 决定是否外发微信。不注入 = 与 P1-3 行为一致。
    pub fn with_presenter(
        store: Option<OrcStore>,
        templates: TemplateResolver,
        presenter: Arc<dyn OrcClusterPresenter>,
    ) -> Self {
        Self {
            store,
            templates,
            presenter: Some(presenter),
            driver: None,
        }
    }

    /// 完整装配（P2 派活链路）：呈现层 + 派活驱动器。
    ///
    /// - `presenter=None` → 不推微信（与 P1-3 一致）；
    /// - `driver=None` → 只推进 + 呈现，不派活（与 P1-4 一致，向后兼容）。
    pub fn with_driver(
        store: Option<OrcStore>,
        templates: TemplateResolver,
        presenter: Option<Arc<dyn OrcClusterPresenter>>,
        driver: Option<Arc<dyn AgentDriver>>,
    ) -> Self {
        Self {
            store,
            templates,
            presenter,
            driver,
        }
    }

    /// 模板解析器访问：派活生成任务信封时用（用户模板优先、内置默认兜底）。
    pub fn templates(&self) -> &TemplateResolver {
        &self.templates
    }

    fn require_store(&self) -> Result<&OrcStore, CommandError> {
        self.store.as_ref().ok_or_else(|| {
            CommandError::new(ORCHESTRATION_DISABLED_CODE, ORCHESTRATION_DISABLED_MESSAGE)
        })
    }

    pub async fn create(&self, payload: CreateOrcTaskPayload) -> Result<OrcTaskDto, CommandError> {
        let store = self.require_store()?;
        let goal = payload.goal.trim();
        if goal.is_empty() {
            return Err(CommandError::new("orc_goal_empty", "任务目标不能为空"));
        }
        // P1-4：显式指定 → 任务级覆盖（校验失败明确报错）；未指定 → 继承全局默认
        // `orchestration.notify_mode`（缺失/非法回退 final_only 并告警，§4.6）。
        let notify_mode = match payload.notify_mode.as_deref() {
            Some(raw) => parse_notify_mode(Some(raw))?,
            None => self.global_default_notify_mode().await,
        };
        let task = store
            .create_task(goal, notify_mode)
            .await
            .map_err(orc_error)?;
        let dto = orc_task_to_dto(&task)?;
        // P2 派活：任务创建即唤醒第 1 步的 Agent（新会话开工，open=true）。
        // 派活是附加动作：失败只标记 blocked（§4.6 不自动重推）+ 呈现层推失败提醒，
        // 不影响已落库的创建结果与命令返回。
        self.dispatch_step(store, &task).await;
        Ok(dto)
    }

    /// 全局默认通知节奏：已装配呈现层时读设置（缺失/非法回退 final_only 并告警）；
    /// 未装配（向前兼容）按 final_only。
    async fn global_default_notify_mode(&self) -> NotifyMode {
        match &self.presenter {
            Some(presenter) => presenter.default_notify_mode().await,
            None => NotifyMode::FinalOnly,
        }
    }

    pub async fn list(&self) -> Result<Vec<OrcTaskDto>, CommandError> {
        let store = self.require_store()?;
        let tasks = store.list_tasks().await.map_err(orc_error)?;
        tasks.iter().map(orc_task_to_dto).collect()
    }

    pub async fn advance(
        &self,
        payload: AdvanceOrcTaskPayload,
    ) -> Result<OrcTaskDto, CommandError> {
        let store = self.require_store()?;
        let kind = parse_message_kind(payload.kind);
        let outcome = store
            .on_message(&payload.task_id, kind)
            .await
            .map_err(orc_error)?;
        let task = store.get_task(&payload.task_id).await.map_err(orc_error)?;
        let dto = orc_task_to_dto(&task)?;
        // P1-4 呈现层单一入口：推进后按任务通知节奏决定是否外发微信（失败不阻塞命令结果）。
        if let Some(presenter) = &self.presenter {
            self.present_advance(presenter, store, &task, &dto, kind, &outcome)
                .await;
        }
        // P2 派活：Advance/BackToWork/Recover 且任务未完成 → 把当前目标 Step 的信封
        // 交给该步配置的 Agent（失败只标记 blocked，不改变已落库的推进结果）。
        self.dispatch_current_step(store, &task, &outcome).await;
        Ok(dto)
    }

    /// P1-4 推进后呈现：按 `should_notify` 规则决定是否外发微信集群消息（§4.6）。
    /// 元数据损坏等呈现侧失败只告警，不影响已落库的推进结果。
    async fn present_advance(
        &self,
        presenter: &Arc<dyn OrcClusterPresenter>,
        store: &OrcStore,
        task: &OrcTask,
        dto: &OrcTaskDto,
        kind: MessageKind,
        outcome: &StepOutcome,
    ) {
        let mode = match task.notify_mode() {
            Ok(mode) => mode,
            Err(error) => {
                tracing::warn!(
                    task_id = %task.id(),
                    code = error.code.as_str(),
                    "读取任务通知节奏失败，跳过集群消息推送"
                );
                return;
            }
        };
        if !should_notify(mode, outcome) {
            return;
        }
        let total = store.workflow().max_order();
        let step = dto.current_step;
        // 正文里的"收到汇报的那一步"：Advance 后 current_step 已指向下一步，需回退一步。
        let reported_step = if outcome.action == TransitionAction::Advance {
            step.saturating_sub(1)
        } else {
            step
        };
        let body = progress_body(kind, outcome.action, reported_step);
        let text = render_cluster_message(task.id(), step, total, state_cn(&dto.state), &body);
        presenter.push(task.id(), text).await;
    }

    pub async fn mark_blocked(
        &self,
        payload: MarkBlockedOrcTaskPayload,
    ) -> Result<OrcTaskDto, CommandError> {
        let store = self.require_store()?;
        let reason = payload.reason.trim();
        if reason.is_empty() {
            return Err(CommandError::new(
                "orc_block_reason_empty",
                "阻塞原因不能为空（请写清哪一步失败、谁不可用、未送达）",
            ));
        }
        let task = store
            .mark_blocked(&payload.task_id, payload.step, reason)
            .await
            .map_err(orc_error)?;
        let dto = orc_task_to_dto(&task)?;
        // P1-4 失败提醒不受 notify_mode 限制：一律外发（§4.6：写清失败 Step/原因，不自动重推）。
        if let Some(presenter) = &self.presenter {
            self.present_blocked(presenter, store, task.id(), payload.step, reason)
                .await;
        }
        Ok(dto)
    }

    /// P1-4 失败提醒呈现：写清哪一步失败、原因，需人工处理（会话语义与命令 mark_blocked 一致）。
    async fn present_blocked(
        &self,
        presenter: &Arc<dyn OrcClusterPresenter>,
        store: &OrcStore,
        task_id: &str,
        step: u32,
        reason: &str,
    ) {
        let total = store.workflow().max_order();
        let text = render_cluster_message(
            task_id,
            step,
            total,
            state_cn(&OrcTaskStateDto::Failed),
            &failure_body(step, reason),
        );
        presenter.push(task_id, text).await;
    }

    /// P2 推进后自动派活：`outcome.action` 属于要干活的转移（Advance/BackToWork/Recover）
    /// 且任务未完成时，派活当前目标 Step 的 Agent；其余转移（Stay/WaitConfirm/Complete）不派活。
    ///
    /// 与呈现层同语义：派活成功/失败都不改变已落库的推进结果；失败只标记 blocked
    /// （§4.6 不自动重推），由 [`Self::present_blocked`] 推微信失败提醒，不阻塞命令返回。
    async fn dispatch_current_step(&self, store: &OrcStore, task: &OrcTask, outcome: &StepOutcome) {
        use TransitionAction::{Advance, BackToWork, Recover};
        if !matches!(outcome.action, Advance | BackToWork | Recover) {
            return;
        }
        if task.state() == TaskState::Completed {
            return;
        }
        self.dispatch_step(store, task).await;
    }

    /// P2 派活当前步骤：组装信封 → 交给该步配置的 Agent（Step 1 试图开新会话，后续步续聊）。
    ///
    /// - 未注入 driver → 保持 P1-3/1-4 行为（只推进 + 呈现，不派活，agent_hint 缺失也不报错）；
    /// - 步骤缺失（内部不一致）→ 记 error 日志后跳过，不阻塞推进、不标记阻塞；
    /// - `agent_hint` 缺失 → 明确错误码 `orc_step_agent_missing` 并自动 blocked
    ///   （写清「Step N 未配置 Agent」，用户可改工作流后恢复）。
    async fn dispatch_step(&self, store: &OrcStore, task: &OrcTask) {
        // 未注入 driver：保持 P1-3/1-4 行为（推进 + 呈现，不派活）。
        let Some(driver) = self.driver.as_ref() else {
            return;
        };
        let current_step = match task.current_step() {
            Ok(step) => step,
            Err(error) => {
                tracing::error!(
                    task_id = %task.id(),
                    code = error.code.as_str(),
                    "读取任务当前步骤失败，跳过派活"
                );
                return;
            }
        };
        let Some(step) = store.workflow().step(current_step) else {
            tracing::error!(
                task_id = %task.id(),
                step = current_step,
                "工作流缺少当前步骤（内部不一致），跳过派活"
            );
            return;
        };
        let Some(agent_hint) = step.agent_hint.as_deref() else {
            let reason = format!("Step {current_step} 未配置 Agent（agent_hint），无法派活");
            tracing::warn!(task_id = %task.id(), step = current_step, code = ORC_STEP_AGENT_MISSING, "{reason}");
            self.auto_blocked(store, task.id(), current_step, reason)
                .await;
            return;
        };
        let goal = match task.goal() {
            Ok(goal) => goal,
            Err(error) => {
                tracing::error!(
                    task_id = %task.id(),
                    code = error.code.as_str(),
                    "读取任务目标失败，跳过派活"
                );
                return;
            }
        };
        // 信封渲染（用户模板优先、内置默认兜底，§4.3 / P1-5）：渲染告警只记日志不阻断。
        let next_role = store
            .workflow()
            .next_step(current_step)
            .map(|next| next.role.as_str());
        let rendered = self
            .templates
            .render_envelope(store.workflow(), step, &goal, next_role);
        for warning in &rendered.warnings {
            tracing::warn!(
                task_id = %task.id(),
                kind = %warning.kind.as_str(),
                "{}",
                warning.message
            );
        }
        let targets = match dispatch_targets(task.id(), agent_hint, current_step) {
            Ok(targets) => targets,
            Err(reason) => {
                tracing::warn!(task_id = %task.id(), step = current_step, "{reason}");
                self.auto_blocked(store, task.id(), current_step, reason)
                    .await;
                return;
            }
        };
        let (agent_id, session_id) = targets;
        let open = current_step == 1;
        if let Err(error) = driver
            .dispatch(task.id(), &agent_id, &session_id, &rendered.text, open)
            .await
        {
            tracing::warn!(
                task_id = %task.id(),
                step = current_step,
                code = error.code(),
                "派活失败：{}",
                error.message()
            );
            self.auto_blocked(
                store,
                task.id(),
                current_step,
                format!("Step {current_step} 派活失败：{}", error.message()),
            )
            .await;
        }
    }

    /// P2 派活失败 → 自动 blocked（§4.6：不自动重推，等人工处理）并推微信失败提醒。
    /// 落库失败（如任务已终止）只记日志，不再改变推进结果。
    async fn auto_blocked(&self, store: &OrcStore, task_id: &str, step: u32, reason: String) {
        match store.mark_blocked(task_id, step, &reason).await {
            Ok(task) => {
                if let Some(presenter) = &self.presenter {
                    self.present_blocked(presenter, store, task.id(), step, &reason)
                        .await;
                }
            }
            Err(error) => {
                tracing::error!(
                    task_id,
                    step,
                    code = error.code.as_str(),
                    "派活失败后自动标记阻塞失败：{}",
                    error.message
                );
            }
        }
    }

    pub async fn recover_blocked(
        &self,
        payload: OrcTaskIdPayload,
    ) -> Result<OrcTaskDto, CommandError> {
        let store = self.require_store()?;
        let task = store
            .recover_blocked(&payload.task_id)
            .await
            .map_err(orc_error)?;
        orc_task_to_dto(&task)
    }
}

/// 读取 `orchestration.enabled`（默认关闭，§8.5）；开启时装配绑定预置工作流的 SQLite 仓储。
/// 设置读取失败按默认关闭处理（保守：编排是可选功能，不阻塞应用启动）。
pub async fn orchestration_store(
    store: &Arc<SqliteStore>,
    settings: &ProductionSettingsStore,
) -> Option<OrcStore> {
    let enabled = match settings.store().settings_entries().await {
        Ok(entries) => entries
            .get(KEY_ORCHESTRATION_ENABLED)
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        Err(error) => {
            tracing::warn!(%error, "读取编排开关失败，按默认关闭（orchestration.enabled=false）处理");
            false
        }
    };
    if !enabled {
        return None;
    }
    let workflow = Workflow::preset(false).expect("预置工作流必须有效");
    Some(OrcStore::with_repository(workflow, store.clone()))
}

/// 装配编排时加载用户 harness 模板配置（§4.3 / P1-5）。
///
/// 文件缺失 = 正常未配置（内置默认兜底，不告警）；读取/解析/单条损坏 →
/// 每条告警写清哪里失败、如何回退并记入日志（不静默），编排不会因用户配置坏而瘫痪。
pub fn load_harness_templates(config_dir: impl AsRef<Path>) -> TemplateResolver {
    let result =
        TemplateResolver::from_config_file(config_dir.as_ref().join(HARNESS_TEMPLATES_FILE));
    for warning in &result.warnings {
        tracing::warn!(
            kind = %warning.kind.as_str(),
            "{}",
            warning.message
        );
    }
    result.resolver
}

/// 通知节奏解析：缺省 final_only（只推最终汇报，§4.6）；未知取值明确报错，不猜测。
fn parse_notify_mode(raw: Option<&str>) -> Result<NotifyMode, CommandError> {
    match raw {
        None | Some("final_only") => Ok(NotifyMode::FinalOnly),
        Some("verbose") => Ok(NotifyMode::Verbose),
        Some(other) => Err(CommandError::new(
            "orc_notify_mode_invalid",
            format!("无效的通知节奏：{other}（可选 final_only / verbose）"),
        )),
    }
}

fn parse_message_kind(kind: OrcMessageKindDto) -> MessageKind {
    match kind {
        OrcMessageKindDto::Report => MessageKind::Report,
        OrcMessageKindDto::Instruction => MessageKind::Instruction,
        OrcMessageKindDto::Confirm => MessageKind::Confirm,
        OrcMessageKindDto::Question => MessageKind::Question,
        OrcMessageKindDto::Info => MessageKind::Info,
    }
}

/// 编排错误 → 命令错误：保留稳定错误码与中文用户消息。
fn orc_error(error: OrcError) -> CommandError {
    CommandError::new(error.code.as_str(), error.message)
}

/// P2 派活目标：由 `agent_hint` 解析 Agent id，并为 (task, step) 生成稳定会话 id
/// （`task-<task_id>-step-<n>`）。Step 1 用该会话开新会话，后续步 resume 同一会话。
/// 解析失败返回用户可读中文原因（不猜测兜底）。
fn dispatch_targets(
    task_id: &str,
    agent_hint: &str,
    step: u32,
) -> Result<(AgentId, AgentSessionId), String> {
    let agent_id = AgentId::new(agent_hint.to_string())
        .map_err(|_| format!("Step {step} 的 Agent 标识无效（{agent_hint}），无法派活"))?;
    let session_id = AgentSessionId::new(format!(
        "{ORC_DISPATCH_SESSION_PREFIX}-{task_id}-step-{step}"
    ))
    .map_err(|_| format!("Step {step} 的会话标识生成失败，无法派活"))?;
    Ok((agent_id, session_id))
}

/// 脱敏后的任务视图：只暴露任务上下文，不暴露内部元数据细节。
fn orc_task_to_dto(task: &OrcTask) -> Result<OrcTaskDto, CommandError> {
    let meta = task.meta().map_err(orc_error)?;
    Ok(OrcTaskDto {
        id: task.id().to_string(),
        workflow_id: meta.workflow_id,
        state: orc_task_state_dto(task.state()),
        current_step: meta.current_step,
        blocked_step: meta.blocked_step,
        block_reason: meta.block_reason,
        notify_mode: meta.notify_mode.as_str().to_string(),
        goal: meta.goal,
    })
}

/// A2A `TaskState` → 稳定 DTO 字符串（§8.3 映射表的桌面呈现侧）。
/// `TaskState` 带 `#[non_exhaustive]`：未来新增状态统一落到 `Unspecified`，保持契约稳定。
fn orc_task_state_dto(state: TaskState) -> OrcTaskStateDto {
    match state {
        TaskState::Unspecified => OrcTaskStateDto::Unspecified,
        TaskState::Submitted => OrcTaskStateDto::Submitted,
        TaskState::Working => OrcTaskStateDto::Working,
        TaskState::Completed => OrcTaskStateDto::Completed,
        TaskState::Failed => OrcTaskStateDto::Failed,
        TaskState::Canceled => OrcTaskStateDto::Canceled,
        TaskState::InputRequired => OrcTaskStateDto::InputRequired,
        TaskState::Rejected => OrcTaskStateDto::Rejected,
        TaskState::AuthRequired => OrcTaskStateDto::AuthRequired,
        _ => OrcTaskStateDto::Unspecified,
    }
}

/// 更新包要落回的安装目录就是当前程序所在目录（安装器路径用 /DIR= 锁定同一位置）。
fn current_install_root() -> Result<PathBuf, CommandError> {
    let executable = std::env::current_exe().map_err(|error| {
        CommandError::new(
            "update_install_dir_missing",
            format!("无法确定当前程序位置，无法安装更新：{error}"),
        )
    })?;
    executable
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| {
            CommandError::new(
                "update_install_dir_missing",
                format!("无法确定安装目录：{}", executable.display()),
            )
        })
}
