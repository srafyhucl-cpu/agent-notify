use std::sync::Arc;
use std::time::Duration;

use agentnotify_agent_sdk::AgentEventEnvelope;
use agentnotify_application::{ChannelAccountStore, IngestResult, StatusStore};
use agentnotify_channel_clawbot::CLAWBOT_CHANNEL_ID;
use agentnotify_channel_sdk::{BeginLoginRequest, ChannelLoginAdapter, LoginSessionId};
use agentnotify_domain::{
    AgentId, ChannelAccountId, DeliveryId, DeliveryState, NotificationId, RequestId, Timestamp,
};
use agentnotify_storage_sqlite::{
    DeliveryViewRecord, NotificationQuery, NotificationRecord, SqliteStore,
};
use tauri::{AppHandle, Wry};

use super::events::{map_delivery_state, map_login_session_state};
use super::runtime::ProductionRuntimeCoordinator;
use super::settings::ProductionSettingsStore;
use crate::bridge::commands::HostCommandService;
use crate::bridge::dto::*;
use crate::bridge::error::CommandError;

pub struct ProductionHostCommandService {
    app: Option<AppHandle<Wry>>,
    runtime: Arc<ProductionRuntimeCoordinator>,
    store: Arc<SqliteStore>,
    settings: ProductionSettingsStore,
}

impl ProductionHostCommandService {
    pub fn new(
        app: Option<AppHandle<Wry>>,
        runtime: Arc<ProductionRuntimeCoordinator>,
        store: Arc<SqliteStore>,
        settings: ProductionSettingsStore,
    ) -> Self {
        Self {
            app,
            runtime,
            store,
            settings,
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

        self.store
            .upsert_agent_config(&payload.agent_id, new_enabled, &new_config)
            .await
            .map_err(|e| CommandError::new("agent_config_save_failed", e.to_string()))?;

        // 重新构建 runtime
        self.runtime.start_or_restart().await?;

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
            let _ = channel.logout(account.clone()).await;
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
        let timeout = Duration::from_secs(5);

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
            tokio::time::sleep(Duration::from_millis(150)).await;
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

        // 重新排队 outbox 并获取最新投递视图
        let record = self
            .store
            .requeue_delivery(id)
            .await
            .map_err(|e| CommandError::new(e.code(), format!("重试投递失败：{e}")))?;

        Ok(map_delivery_view_record(record))
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

        self.settings.save_settings(&payload).await?;
        self.runtime
            .set_outbox_paused(payload.notifications_paused)
            .await?;

        // 仅在关键运行时配置发生变化时重启 runtime
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
        let _ = self.runtime.set_outbox_paused(true).await;
        let _ = self.store.wal_checkpoint_truncate().await;

        if let Some(app) = &self.app {
            let app_clone = app.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(Duration::from_millis(100)).await;
                app_clone.exit(0);
            });
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
        let current_version = match self.runtime.current_snapshot().await {
            Some(s) => s.app_version,
            None => env!("CARGO_PKG_VERSION").to_string(),
        };

        Ok(UpdateStatusDto {
            current_version,
            available_version: None,
            state: UpdateStateDto::Unsupported,
            signed: false,
            preview: false,
            message: "当前版本暂不支持自动检查更新".into(),
            checked_at: Some(Timestamp::now_utc().to_rfc3339()),
        })
    }
}

impl ProductionHostCommandService {
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

fn sanitize_account_config(config: &serde_json::Value) -> serde_json::Value {
    if let serde_json::Value::Object(map) = config {
        let mut safe_map = serde_json::Map::new();
        for (k, v) in map {
            if !k.to_lowercase().contains("token")
                && !k.to_lowercase().contains("secret")
                && !k.to_lowercase().contains("password")
            {
                safe_map.insert(k.clone(), v.clone());
            }
        }
        serde_json::Value::Object(safe_map)
    } else {
        config.clone()
    }
}

fn map_login_session_dto(session: &agentnotify_channel_sdk::LoginSession) -> LoginSessionDto {
    LoginSessionDto {
        id: session.id().as_str().to_string(),
        account_id: session.account_id().map(str::to_owned),
        account_key: session.account_key().to_string(),
        state: map_login_session_state(session.state()),
        qr_payload: session.qr_payload().map(str::to_owned),
        created_at: session.created_at().to_rfc3339(),
        message: session.error().map(|e| e.message().to_string()),
        error: session.error().map(|e| SafeErrorDto {
            code: e.code().to_string(),
            message: e.message().to_string(),
        }),
    }
}

fn map_delivery_view_record(record: DeliveryViewRecord) -> DeliveryDto {
    DeliveryDto {
        id: record.delivery.id().to_string(),
        notification_id: record.delivery.notification_id().to_string(),
        channel_id: record.delivery.channel_id().to_string(),
        account_id: record.delivery.account_id().to_string(),
        state: map_delivery_state(record.delivery.state()),
        external_message_id: record.delivery.external_message_id().map(|m| m.to_string()),
        error: record.delivery.error().map(|e| SafeErrorDto {
            code: e.code().to_string(),
            message: e.message().to_string(),
        }),
        retryable: record.delivery.can_retry(),
        updated_at: record.updated_at.to_rfc3339(),
    }
}

fn map_notification_record(record: NotificationRecord) -> NotificationSummaryDto {
    NotificationSummaryDto {
        id: record.notification.id.to_string(),
        agent_id: record.notification.agent_id.to_string(),
        session_id: record.notification.session_id.map(|s| s.to_string()),
        session_title: record.notification.session_title,
        title: record.notification.title,
        preview: record.notification.body.chars().take(100).collect(),
        occurred_at: record.notification.occurred_at.to_rfc3339(),
        delivery_states: record
            .delivery_states
            .into_iter()
            .map(map_delivery_state)
            .collect(),
    }
}

fn map_migration_snapshot(snapshot: &agentnotify_runtime::MigrationSnapshot) -> LegacyMigrationDto {
    LegacyMigrationDto {
        state: match snapshot.state {
            agentnotify_runtime::MigrationState::NotConfigured => MigrationStateDto::NotConfigured,
            agentnotify_runtime::MigrationState::NotDetected => MigrationStateDto::NotDetected,
            agentnotify_runtime::MigrationState::Completed => MigrationStateDto::Completed,
            agentnotify_runtime::MigrationState::Partial => MigrationStateDto::Partial,
            agentnotify_runtime::MigrationState::Required => MigrationStateDto::Required,
        },
        source_detected: snapshot.source_detected,
        report_file: snapshot.report_file.clone(),
        report: snapshot.report.as_ref().map(|r| MigrationReportDto {
            imported_at: r.imported_at.clone(),
            source_file_count: r.source_file_count as u32,
            settings_imported: r.settings_imported as u32,
            agent_configs_imported: r.agent_configs_imported as u32,
            accounts_imported: r.accounts_imported as u32,
            notifications_imported: r.notifications_imported as u32,
            deliveries_imported: r.deliveries_imported as u32,
            routes_imported: r.routes_imported as u32,
            claims_imported: r.claims_imported as u32,
            skipped_records: r.skipped_records as u32,
            warnings: r
                .warnings
                .iter()
                .map(|w| MigrationWarningDto {
                    code: w.code.clone(),
                    file: w.file.clone(),
                    record: w.record,
                })
                .collect(),
        }),
        error: snapshot.error.as_ref().map(|f| MigrationIssueDto {
            code: f.code.clone(),
            message: f.message.clone(),
            file: f.file.clone(),
            field: f.field.clone(),
        }),
    }
}
