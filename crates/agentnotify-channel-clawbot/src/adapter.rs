use std::sync::Arc;

use agentnotify_application::{SecretError, SecretKind, SecretStore};
use agentnotify_channel_sdk::{
    ChannelAccount, ChannelAdapter, ChannelCapabilities, ChannelDescriptor, ChannelError,
    ChannelHealth, ChannelTask, DeliveryReceipt, InboundEmitter, OutboundMessage,
};
use agentnotify_domain::{ChannelAccountId, SafeError};

use crate::{
    account::ClawBotAccount,
    descriptor::{MAX_TEXT_BYTES, capabilities, descriptor},
    state::{ClawBotContext, ClawBotCredentials},
};

/// 第一阶段只实现账号与安全凭据边界；发送和长轮询由后续任务接入。
#[derive(Clone)]
pub struct ClawBotChannel {
    secrets: Arc<dyn SecretStore>,
}

impl ClawBotChannel {
    pub fn new(secrets: Arc<dyn SecretStore>) -> Self {
        Self { secrets }
    }

    pub async fn save_credentials(
        &self,
        account_id: &ChannelAccountId,
        credentials: &ClawBotCredentials,
    ) -> Result<(), ChannelError> {
        self.secrets
            .set(account_id, SecretKind::BotToken, credentials.to_secret()?)
            .await
            .map_err(secret_store_error)
    }

    pub async fn load_credentials(
        &self,
        account_id: &ChannelAccountId,
    ) -> Result<ClawBotCredentials, ChannelError> {
        let secret = self
            .secrets
            .get(account_id, SecretKind::BotToken)
            .await
            .map_err(secret_store_error)?;
        ClawBotCredentials::from_secret(&secret)
    }

    pub async fn save_context(
        &self,
        account_id: &ChannelAccountId,
        context: &ClawBotContext,
    ) -> Result<(), ChannelError> {
        self.secrets
            .set(account_id, SecretKind::ContextToken, context.to_secret()?)
            .await
            .map_err(secret_store_error)
    }

    pub async fn load_context(
        &self,
        account_id: &ChannelAccountId,
    ) -> Result<ClawBotContext, ChannelError> {
        let secret = self
            .secrets
            .get(account_id, SecretKind::ContextToken)
            .await
            .map_err(secret_store_error)?;
        ClawBotContext::from_secret(&secret)
    }

    async fn delete_secrets(&self, account_id: &ChannelAccountId) -> Result<(), ChannelError> {
        let bot_token = self.secrets.delete(account_id, SecretKind::BotToken).await;
        let context_token = self
            .secrets
            .delete(account_id, SecretKind::ContextToken)
            .await;

        match (bot_token, context_token) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(error), _) | (_, Err(error)) => Err(secret_store_error(error)),
        }
    }
}

#[async_trait::async_trait]
impl ChannelAdapter for ClawBotChannel {
    fn descriptor(&self) -> ChannelDescriptor {
        descriptor()
    }

    fn capabilities(&self) -> ChannelCapabilities {
        capabilities()
    }

    async fn start(
        &self,
        _account: ChannelAccount,
        _emit: InboundEmitter,
    ) -> Result<ChannelTask, ChannelError> {
        Ok(ChannelTask::completed())
    }

    async fn send(
        &self,
        _account: ChannelAccount,
        message: OutboundMessage,
    ) -> Result<DeliveryReceipt, ChannelError> {
        validate_outbound(&message)?;
        Err(ChannelError::permanent(
            "clawbot_send_not_implemented",
            "ClawBot 发送能力尚未接入",
        ))
    }

    async fn inspect(&self, account: ChannelAccount) -> ChannelHealth {
        let account = match ClawBotAccount::from_channel_account(account) {
            Ok(account) => account,
            Err(error) => {
                return ChannelHealth::unavailable(safe_error(error.code(), error.message()));
            }
        };
        match self.load_credentials(account.id()).await {
            Ok(_) => {
                if account.state().stale_at.is_some() {
                    ChannelHealth::stale(safe_error(
                        "clawbot_session_stale",
                        "ClawBot 会话已失效，请重新扫码或发送消息恢复",
                    ))
                } else {
                    ChannelHealth::healthy()
                }
            }
            Err(error) => ChannelHealth::unavailable(safe_error(error.code(), error.message())),
        }
    }

    async fn logout(&self, account: ChannelAccount) -> Result<(), ChannelError> {
        let account = ClawBotAccount::from_channel_account(account)?;
        self.delete_secrets(account.id()).await
    }
}

fn validate_outbound(message: &OutboundMessage) -> Result<(), ChannelError> {
    if message.text.trim().is_empty() {
        return Err(ChannelError::permanent(
            "clawbot_empty_text",
            "ClawBot 消息正文不能为空",
        ));
    }
    if message.text.len() > MAX_TEXT_BYTES {
        return Err(ChannelError::permanent(
            "clawbot_text_too_large",
            "ClawBot 消息正文超过 32 KiB 上限",
        ));
    }
    Ok(())
}

fn secret_store_error(error: SecretError) -> ChannelError {
    let code = if error.code() == "secret_not_found" {
        "clawbot_secret_not_found"
    } else {
        "clawbot_secret_store_failed"
    };
    ChannelError::permanent(code, error.message())
}

fn safe_error(code: &str, message: &str) -> SafeError {
    SafeError::new(code, message).expect("ClawBot 安全错误常量必须有效")
}
