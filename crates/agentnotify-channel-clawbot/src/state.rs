use std::fmt::{Debug, Formatter};

use agentnotify_application::SecretValue;
use agentnotify_channel_sdk::ChannelError;
use agentnotify_domain::Timestamp;

pub const DEFAULT_BASE_URL: &str = "https://ilinkai.weixin.qq.com";

/// 明文中不能落库的账号状态只保留平台 ID 尾部，用于登录后辨识账号。
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct ClawBotAccountState {
    pub bot_id_hint: String,
    pub user_id_hint: String,
    pub base_url: String,
    pub stale_at: Option<Timestamp>,
    pub session_established_at: Option<Timestamp>,
    #[serde(default)]
    pub session_alert_at: Option<Timestamp>,
}

impl ClawBotAccountState {
    pub fn new(bot_id_hint: String, user_id_hint: String) -> Self {
        Self {
            bot_id_hint,
            user_id_hint,
            base_url: DEFAULT_BASE_URL.into(),
            stale_at: None,
            session_established_at: None,
            session_alert_at: None,
        }
    }
}

/// ClawBot 游标单独保存，账号切换时不能沿用上一个账号的值。
#[derive(Clone, Debug, Default, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct ClawBotCursor {
    pub get_updates_buf: String,
}

impl ClawBotCursor {
    pub fn new(get_updates_buf: impl Into<String>) -> Self {
        Self {
            get_updates_buf: get_updates_buf.into(),
        }
    }
}

/// 账号登录结果中的敏感字段整体进入 Credential Manager 的 bot-token 条目。
#[derive(Clone, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct ClawBotCredentials {
    pub bot_token: String,
    pub bot_id: String,
    pub user_id: String,
    pub base_url: String,
}

impl ClawBotCredentials {
    pub fn new(
        bot_token: impl Into<String>,
        bot_id: impl Into<String>,
        user_id: impl Into<String>,
        base_url: impl Into<String>,
    ) -> Result<Self, ChannelError> {
        let value = Self {
            bot_token: bot_token.into(),
            bot_id: bot_id.into(),
            user_id: user_id.into(),
            base_url: base_url.into(),
        };
        value.validate()?;
        Ok(value)
    }

    pub fn bot_token(&self) -> &str {
        &self.bot_token
    }

    pub fn bot_id(&self) -> &str {
        &self.bot_id
    }

    pub fn user_id(&self) -> &str {
        &self.user_id
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn validate(&self) -> Result<(), ChannelError> {
        validate_required("bot_token", &self.bot_token)?;
        validate_required("bot_id", &self.bot_id)?;
        validate_required("user_id", &self.user_id)?;
        validate_required("base_url", &self.base_url)?;
        Ok(())
    }

    pub(crate) fn to_secret(&self) -> Result<SecretValue, ChannelError> {
        self.validate()?;
        SecretValue::new(serde_json::to_string(self).map_err(|_| {
            ChannelError::permanent(
                "clawbot_credentials_encode_failed",
                "ClawBot 登录凭据编码失败",
            )
        })?)
        .map_err(|_| {
            ChannelError::permanent(
                "clawbot_credentials_encode_failed",
                "ClawBot 登录凭据编码失败",
            )
        })
    }

    pub(crate) fn from_secret(value: &SecretValue) -> Result<Self, ChannelError> {
        let credentials =
            serde_json::from_str::<Self>(value.expose()).map_err(|_| invalid_credentials())?;
        credentials.validate()?;
        Ok(credentials)
    }
}

impl Debug for ClawBotCredentials {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ClawBotCredentials")
            .field("bot_token", &"[REDACTED]")
            .field("bot_id", &"[REDACTED]")
            .field("user_id", &"[REDACTED]")
            .field("base_url", &self.base_url)
            .finish()
    }
}

/// context token 与绑定的平台用户 ID 必须成组读取。
#[derive(Clone, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct ClawBotContext {
    pub context_token: String,
    pub user_id: String,
}

impl ClawBotContext {
    pub fn new(
        context_token: impl Into<String>,
        user_id: impl Into<String>,
    ) -> Result<Self, ChannelError> {
        let value = Self {
            context_token: context_token.into(),
            user_id: user_id.into(),
        };
        value.validate()?;
        Ok(value)
    }

    pub fn context_token(&self) -> &str {
        &self.context_token
    }

    pub fn user_id(&self) -> &str {
        &self.user_id
    }

    pub fn validate(&self) -> Result<(), ChannelError> {
        validate_required("context_token", &self.context_token)?;
        validate_required("user_id", &self.user_id)?;
        Ok(())
    }

    pub(crate) fn to_secret(&self) -> Result<SecretValue, ChannelError> {
        self.validate()?;
        SecretValue::new(serde_json::to_string(self).map_err(|_| {
            ChannelError::permanent("clawbot_context_encode_failed", "ClawBot 会话凭据编码失败")
        })?)
        .map_err(|_| {
            ChannelError::permanent("clawbot_context_encode_failed", "ClawBot 会话凭据编码失败")
        })
    }

    pub(crate) fn from_secret(value: &SecretValue) -> Result<Self, ChannelError> {
        let context =
            serde_json::from_str::<Self>(value.expose()).map_err(|_| invalid_context())?;
        context.validate()?;
        Ok(context)
    }
}

impl Debug for ClawBotContext {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ClawBotContext")
            .field("context_token", &"[REDACTED]")
            .field("user_id", &"[REDACTED]")
            .finish()
    }
}

fn validate_required(field: &str, value: &str) -> Result<(), ChannelError> {
    if value.trim().is_empty() {
        return Err(ChannelError::permanent(
            "clawbot_invalid_credentials",
            format!("ClawBot 凭据字段 {field} 不能为空"),
        ));
    }
    Ok(())
}

fn invalid_credentials() -> ChannelError {
    ChannelError::permanent(
        "clawbot_invalid_credentials",
        "ClawBot 登录凭据损坏，请重新登录",
    )
}

fn invalid_context() -> ChannelError {
    ChannelError::permanent(
        "clawbot_invalid_context",
        "ClawBot 会话凭据损坏，请重新发送一条消息",
    )
}
