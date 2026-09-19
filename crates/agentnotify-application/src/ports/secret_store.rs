use std::fmt::{Debug, Formatter};

use agentnotify_domain::ChannelAccountId;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SecretKind {
    BotToken,
    ContextToken,
    AppSecret,
}

impl SecretKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BotToken => "bot_token",
            Self::ContextToken => "context_token",
            Self::AppSecret => "app_secret",
        }
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct SecretValue(String);

impl SecretValue {
    pub fn new(value: impl Into<String>) -> Result<Self, SecretError> {
        let value = value.into();
        if value.is_empty() {
            return Err(SecretError::new("invalid_secret", "密钥值不能为空"));
        }
        Ok(Self(value))
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl Debug for SecretValue {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("SecretValue([REDACTED])")
    }
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct SecretError {
    code: String,
    message: String,
}

impl SecretError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn code(&self) -> &str {
        &self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl std::fmt::Display for SecretError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for SecretError {}

#[async_trait::async_trait]
pub trait SecretStore: Send + Sync {
    async fn get(
        &self,
        account_id: &ChannelAccountId,
        kind: SecretKind,
    ) -> Result<SecretValue, SecretError>;

    async fn set(
        &self,
        account_id: &ChannelAccountId,
        kind: SecretKind,
        value: SecretValue,
    ) -> Result<(), SecretError>;

    async fn delete(
        &self,
        account_id: &ChannelAccountId,
        kind: SecretKind,
    ) -> Result<(), SecretError>;
}
