use std::fmt::Display;

use agentnotify_domain::Timestamp;

use crate::ChannelError;

#[derive(
    Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, serde::Deserialize, serde::Serialize,
)]
#[serde(transparent)]
pub struct LoginSessionId(String);

impl LoginSessionId {
    pub fn new(value: impl Into<String>) -> Result<Self, ChannelError> {
        let value = value.into();
        if value.trim().is_empty() || value.trim() != value {
            return Err(ChannelError::permanent(
                "invalid_login_session",
                "登录会话标识无效",
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for LoginSessionId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum LoginSessionState {
    Preparing,
    QrReady,
    WaitingScan,
    NeedVerifyCode,
    WaitingFirstInbound,
    Paired,
    Expired,
    Blocked,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BeginLoginRequest {
    pub account_key: String,
}

impl BeginLoginRequest {
    pub fn new(account_key: impl Into<String>) -> Self {
        Self {
            account_key: account_key.into(),
        }
    }
}

/// 只存在内存中的渠道登录会话；二维码载荷不得写入磁盘。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoginSession {
    id: LoginSessionId,
    account_key: String,
    account_id: Option<String>,
    state: LoginSessionState,
    qr_payload: Option<String>,
    created_at: Timestamp,
    error: Option<agentnotify_domain::SafeError>,
}

impl LoginSession {
    pub fn new(
        id: LoginSessionId,
        account_key: impl Into<String>,
        state: LoginSessionState,
        created_at: Timestamp,
    ) -> Result<Self, ChannelError> {
        let account_key = account_key.into();
        if account_key.trim().is_empty() {
            return Err(ChannelError::permanent(
                "invalid_login_account",
                "登录账号标识不能为空",
            ));
        }
        Ok(Self {
            id,
            account_key,
            account_id: None,
            state,
            qr_payload: None,
            created_at,
            error: None,
        })
    }

    pub fn id(&self) -> &LoginSessionId {
        &self.id
    }

    pub fn account_key(&self) -> &str {
        &self.account_key
    }

    /// 登录确认并持久化账号后才会出现，未绑定时保持为空。
    pub fn account_id(&self) -> Option<&str> {
        self.account_id.as_deref()
    }

    pub fn state(&self) -> LoginSessionState {
        self.state
    }

    pub fn qr_payload(&self) -> Option<&str> {
        self.qr_payload.as_deref()
    }

    pub fn created_at(&self) -> Timestamp {
        self.created_at
    }

    pub fn error(&self) -> Option<&agentnotify_domain::SafeError> {
        self.error.as_ref()
    }

    pub fn with_account_id(mut self, account_id: impl Into<String>) -> Self {
        self.account_id = Some(account_id.into());
        self
    }

    pub fn with_qr_payload(mut self, qr_payload: impl Into<String>) -> Self {
        self.qr_payload = Some(qr_payload.into());
        self
    }
}

/// 渠道登录使用独立接口，避免普通发送适配器被迫实现交互流程。
#[async_trait::async_trait]
pub trait ChannelLoginAdapter: Send + Sync {
    async fn begin_login(&self, request: BeginLoginRequest) -> Result<LoginSession, ChannelError>;

    async fn submit_login_code(
        &self,
        session_id: &LoginSessionId,
        code: &str,
    ) -> Result<LoginSession, ChannelError>;

    async fn cancel_login(&self, session_id: &LoginSessionId) -> Result<(), ChannelError>;
}
