//! 渠道账号命令域（{HostCommandService} 的 `channels` 片段，2026-09 从 service.rs 拆分）。
//!
//! 子模块经 `use super::*` 共享父模块导入；`impl` 块与 `service.rs` 主体同属
//! `ProductionHostCommandService`，拆分只搬移不改行为。

use super::*;
use crate::bridge::commands::ChannelCommands;

/// 测试发送后等待 Delivery 落库并达到终态的上限。
const DELIVERY_FINAL_STATE_TIMEOUT: Duration = Duration::from_secs(5);
/// 等待 Delivery 终态期间的 SQLite 轮询间隔。
const DELIVERY_FINAL_STATE_POLL: Duration = Duration::from_millis(150);

#[async_trait::async_trait]
impl ChannelCommands for ProductionHostCommandService {
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
