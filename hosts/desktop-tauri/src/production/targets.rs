use std::sync::Arc;

use agentnotify_agent_sdk::AgentRegistry;
use agentnotify_application::{
    AgentNotificationConfig, ChannelAccountStore, DeliveryTarget, NotificationPolicy, QuietHours,
    ReplyConfig, ReplyTarget, SecretStore,
};
use agentnotify_channel_clawbot::{CLAWBOT_CHANNEL_ID, ClawBotAccount, ClawBotCredentials};
use agentnotify_channel_sdk::ChannelRegistry;
use agentnotify_domain::ChannelId;
use agentnotify_runtime::{ResolvedRuntimeTargets, RuntimeTargetError, RuntimeTargetProvider};
use agentnotify_storage_sqlite::SqliteStore;
use time::Duration as TimeDuration;

use super::settings::ProductionSettingsStore;

pub struct ProductionTargetProvider {
    store: Arc<SqliteStore>,
    settings: ProductionSettingsStore,
    secret_store: Arc<dyn SecretStore>,
    agent_registry: Arc<AgentRegistry>,
    _channel_registry: Arc<ChannelRegistry>,
}

impl ProductionTargetProvider {
    pub fn new(
        store: Arc<SqliteStore>,
        settings: ProductionSettingsStore,
        secret_store: Arc<dyn SecretStore>,
        agent_registry: Arc<AgentRegistry>,
        channel_registry: Arc<ChannelRegistry>,
    ) -> Self {
        Self {
            store,
            settings,
            secret_store,
            agent_registry,
            _channel_registry: channel_registry,
        }
    }
}

#[async_trait::async_trait]
impl RuntimeTargetProvider for ProductionTargetProvider {
    async fn resolve(&self) -> Result<ResolvedRuntimeTargets, RuntimeTargetError> {
        let settings_dto =
            self.settings.load_settings().await.map_err(|error| {
                RuntimeTargetError::new("settings_load_failed", error.message())
            })?;

        // 1. 构建通知策略
        let agent_configs = self.store.agent_configs().await.map_err(|error| {
            RuntimeTargetError::new("agent_configs_load_failed", error.message())
        })?;

        let utc_offset_minutes = 8 * 60;

        let global_quiet_hours = settings_dto.quiet_hours.as_ref().and_then(|qh| {
            if !qh.enabled {
                return None;
            }
            let (sh, sm) = parse_hh_mm(&qh.start)?;
            let (eh, em) = parse_hh_mm(&qh.end)?;
            Some(QuietHours {
                start_minute: sh * 60 + sm,
                end_minute: eh * 60 + em,
                utc_offset_minutes,
            })
        });

        let global_cooldown = if settings_dto.cooldown_seconds > 0 {
            Some(TimeDuration::seconds(i64::from(
                settings_dto.cooldown_seconds,
            )))
        } else {
            None
        };

        let mut policy = NotificationPolicy::default();
        for agent in self.agent_registry.all() {
            let agent_id = agent.descriptor().id;
            let enabled = if let Some(record) = agent_configs.get(agent_id.as_str()) {
                record.enabled
            } else {
                // 没有显式配置的 Agent（如 OpenCode）保守启用
                true
            };

            policy = policy.with_agent(
                agent_id,
                AgentNotificationConfig {
                    enabled,
                    quiet_hours: global_quiet_hours,
                    cooldown: global_cooldown,
                },
            );
        }

        // 2. 构建投递和回复目标
        let clawbot_channel_id = ChannelId::new(CLAWBOT_CHANNEL_ID)
            .map_err(|error| RuntimeTargetError::new("channel_id_invalid", error.message()))?;

        let accounts = self
            .store
            .list(&clawbot_channel_id)
            .await
            .map_err(|error| {
                RuntimeTargetError::new("channel_accounts_load_failed", error.message())
            })?;

        let mut enabled_accounts = Vec::new();
        for account in accounts {
            if account.enabled {
                enabled_accounts.push(account);
            }
        }

        // 确定性排序：优先默认账号，其余按 ID 字典序升序
        let default_acc_id = settings_dto.default_channel_account_id.as_deref();
        enabled_accounts.sort_by(|a, b| {
            let a_is_default = default_acc_id == Some(a.id.as_str());
            let b_is_default = default_acc_id == Some(b.id.as_str());
            match (a_is_default, b_is_default) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.id.as_str().cmp(b.id.as_str()),
            }
        });

        let mut delivery_targets = Vec::new();
        let mut reply_targets = Vec::new();

        for account in enabled_accounts {
            let clawbot_account = match ClawBotAccount::from_channel_account(account.clone()) {
                Ok(acc) => acc,
                Err(error) => {
                    tracing::warn!(
                        account_id = %account.id,
                        %error,
                        "跳过无效的 ClawBot 账号"
                    );
                    continue;
                }
            };

            let _secret_ref = match clawbot_account.bot_token_secret_ref() {
                Ok(sr) => sr,
                Err(error) => {
                    tracing::warn!(
                        account_id = %account.id,
                        %error,
                        "获取 ClawBot 账号密钥引用失败"
                    );
                    continue;
                }
            };

            let secret_value = match self
                .secret_store
                .get(
                    &account.id,
                    agentnotify_application::ports::SecretKind::BotToken,
                )
                .await
            {
                Ok(val) => val,
                Err(error) => {
                    tracing::warn!(
                        account_id = %account.id,
                        %error,
                        "读取 ClawBot 账号凭据失败"
                    );
                    continue;
                }
            };

            let credentials = match ClawBotCredentials::from_secret(&secret_value) {
                Ok(creds) => creds,
                Err(error) => {
                    tracing::warn!(
                        account_id = %account.id,
                        %error,
                        "解析 ClawBot 账号凭据失败"
                    );
                    continue;
                }
            };

            let user_id = credentials.user_id.clone();
            if user_id.trim().is_empty() {
                tracing::warn!(
                    account_id = %account.id,
                    "ClawBot 账号未绑定 user_id，跳过目标装配"
                );
                continue;
            }

            delivery_targets.push(DeliveryTarget::new(account.clone(), user_id.clone()));

            if settings_dto.reply_enabled {
                if let Ok(reply_target) = ReplyTarget::new(account, user_id.clone(), user_id) {
                    reply_targets.push(reply_target);
                }
            }
        }

        let route_ttl = TimeDuration::seconds(i64::from(settings_dto.route_ttl_seconds));

        let reply_config = ReplyConfig {
            enabled: settings_dto.reply_enabled,
            send_confirmation: settings_dto.delivery_receipt_enabled,
            claim_ttl: route_ttl,
            confirmation_text: "已收到回复，正在处理中。".into(),
        };

        Ok(ResolvedRuntimeTargets {
            notification_policy: policy,
            delivery_targets,
            reply_targets,
            reply_config,
            reply_route_ttl: route_ttl,
        })
    }
}

fn parse_hh_mm(s: &str) -> Option<(u16, u16)> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 2 {
        return None;
    }
    let h: u16 = parts[0].parse().ok()?;
    let m: u16 = parts[1].parse().ok()?;
    if h < 24 && m < 60 { Some((h, m)) } else { None }
}
