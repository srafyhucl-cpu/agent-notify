use std::fmt::Display;

use agentnotify_domain::{ChannelAccountId, ChannelId, Timestamp};

use crate::ChannelError;

/// 指向密钥存储条目的稳定引用，不包含密钥值。
#[derive(
    Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, serde::Deserialize, serde::Serialize,
)]
#[serde(transparent)]
pub struct SecretRef(String);

impl SecretRef {
    pub fn new(value: impl Into<String>) -> Result<Self, ChannelError> {
        let value = value.into();
        if value.trim().is_empty() || value.trim() != value {
            return Err(ChannelError::permanent(
                "invalid_secret_ref",
                "密钥引用格式无效",
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for SecretRef {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

/// 独立保存密钥、游标和工作状态的渠道账号。
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct ChannelAccount {
    pub id: ChannelAccountId,
    pub channel_id: ChannelId,
    pub display_name: String,
    pub enabled: bool,
    pub config: serde_json::Value,
    pub secret_ref: Option<SecretRef>,
    pub cursor: serde_json::Value,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl ChannelAccount {
    pub fn new(
        id: ChannelAccountId,
        channel_id: ChannelId,
        display_name: impl Into<String>,
        created_at: Timestamp,
    ) -> Self {
        Self {
            id,
            channel_id,
            display_name: display_name.into(),
            enabled: true,
            config: serde_json::json!({}),
            secret_ref: None,
            cursor: serde_json::json!({}),
            created_at,
            updated_at: created_at,
        }
    }
}

/// 渠道账号健康状态，不暴露 token 或原始响应。
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct ChannelHealth {
    pub available: bool,
    pub stale: bool,
    pub detail: Option<agentnotify_domain::SafeError>,
}

impl ChannelHealth {
    pub const fn healthy() -> Self {
        Self {
            available: true,
            stale: false,
            detail: None,
        }
    }

    pub const fn stale(detail: agentnotify_domain::SafeError) -> Self {
        Self {
            available: true,
            stale: true,
            detail: Some(detail),
        }
    }

    pub const fn unavailable(detail: agentnotify_domain::SafeError) -> Self {
        Self {
            available: false,
            stale: false,
            detail: Some(detail),
        }
    }
}
