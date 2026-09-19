use std::fmt::Display;

use agentnotify_agent_sdk::AgentError;
use agentnotify_channel_sdk::ChannelError;
use agentnotify_domain::{DomainError, SafeError};

/// 不暴露基础设施细节的存储错误。
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct StoreError {
    code: String,
    message: String,
}

impl StoreError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn conflict(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(code, message)
    }

    pub fn not_found(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(code, message)
    }

    pub fn unavailable(message: impl Into<String>) -> Self {
        Self::new("store_unavailable", message)
    }

    pub fn corrupted(message: impl Into<String>) -> Self {
        Self::new("store_corrupted", message)
    }

    pub fn code(&self) -> &str {
        &self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl Display for StoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for StoreError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ApplicationError {
    Domain(DomainError),
    Store(StoreError),
    Agent(AgentError),
    Channel(ChannelError),
    InvalidInput { field: &'static str },
    Conflict { code: &'static str },
    Unavailable { code: &'static str },
    Safe(SafeError),
}

impl ApplicationError {
    pub fn code(&self) -> &str {
        match self {
            Self::Domain(error) => error.code(),
            Self::Store(error) => error.code(),
            Self::Agent(error) => error.code(),
            Self::Channel(error) => error.code(),
            Self::InvalidInput { field } => field,
            Self::Conflict { code } | Self::Unavailable { code } => code,
            Self::Safe(error) => error.code(),
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Self::Domain(error) => error.message(),
            Self::Store(error) => error.message(),
            Self::Agent(error) => error.message(),
            Self::Channel(error) => error.message(),
            Self::InvalidInput { field } => field,
            Self::Conflict { code } | Self::Unavailable { code } => code,
            Self::Safe(error) => error.message(),
        }
    }
}

impl Display for ApplicationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.message().fmt(formatter)
    }
}

impl std::error::Error for ApplicationError {}

impl From<DomainError> for ApplicationError {
    fn from(value: DomainError) -> Self {
        Self::Domain(value)
    }
}

impl From<StoreError> for ApplicationError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}

impl From<AgentError> for ApplicationError {
    fn from(value: AgentError) -> Self {
        Self::Agent(value)
    }
}

impl From<ChannelError> for ApplicationError {
    fn from(value: ChannelError) -> Self {
        Self::Channel(value)
    }
}
