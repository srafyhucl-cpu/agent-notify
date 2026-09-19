use std::sync::Arc;

use agentnotify_application::{ChannelAccountStore, SecretError, SecretKind, SecretStore};
use agentnotify_channel_sdk::{
    ChannelAccount, ChannelAdapter, ChannelCapabilities, ChannelDescriptor, ChannelError,
    ChannelHealth, ChannelTask, DeliveryReceipt, InboundEmitter, OutboundMessage,
};
use agentnotify_domain::{ChannelAccountId, SafeError};

use crate::{
    account::ClawBotAccount,
    descriptor::{MAX_TEXT_BYTES, capabilities, descriptor},
    send::{ClawBotHttpSendTransport, ClawBotSendTransport, send_outbound},
    session::{ClawBotHttpSessionTransport, ClawBotSessionTransport, run_session_task},
    state::{ClawBotContext, ClawBotCredentials},
};

/// ClawBot 账号、安全凭据与可靠出站边界；长轮询由后续任务接入。
#[derive(Clone)]
pub struct ClawBotChannel {
    secrets: Arc<dyn SecretStore>,
    accounts: Option<Arc<dyn ChannelAccountStore>>,
    sender: Arc<dyn ClawBotSendTransport>,
    session: Arc<dyn ClawBotSessionTransport>,
}

impl ClawBotChannel {
    pub fn new(secrets: Arc<dyn SecretStore>) -> Self {
        Self::with_send_transport(secrets, Arc::new(ClawBotHttpSendTransport::new()))
    }

    pub fn with_send_transport(
        secrets: Arc<dyn SecretStore>,
        sender: Arc<dyn ClawBotSendTransport>,
    ) -> Self {
        Self {
            secrets,
            accounts: None,
            sender,
            session: Arc::new(ClawBotHttpSessionTransport::new()),
        }
    }

    pub fn with_account_store(mut self, accounts: Arc<dyn ChannelAccountStore>) -> Self {
        self.accounts = Some(accounts);
        self
    }

    pub fn with_session_transport(mut self, session: Arc<dyn ClawBotSessionTransport>) -> Self {
        self.session = session;
        self
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
        account: ChannelAccount,
        emit: InboundEmitter,
    ) -> Result<ChannelTask, ChannelError> {
        let accounts = self.accounts.clone().ok_or_else(|| {
            ChannelError::permanent(
                "clawbot_account_store_missing",
                "ClawBot 渠道缺少账号存储，无法启动长轮询",
            )
        })?;
        let secrets = self.secrets.clone();
        let session = self.session.clone();
        let (cancel, cancel_receiver) = tokio::sync::watch::channel(false);
        let handle = tokio::spawn(async move {
            run_session_task(secrets, accounts, session, account, emit, cancel_receiver).await
        });
        Ok(ChannelTask::new(handle, cancel))
    }

    async fn send(
        &self,
        account: ChannelAccount,
        message: OutboundMessage,
    ) -> Result<DeliveryReceipt, ChannelError> {
        validate_outbound(&message)?;
        send_outbound(
            self.secrets.as_ref(),
            self.accounts.as_deref(),
            self.sender.as_ref(),
            account,
            message,
        )
        .await
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
