use std::fmt::Display;

use agentnotify_domain::{
    AgentId, AgentSessionId, NotificationMetadata, RequestId, SafeError, Timestamp,
};

use crate::{AgentCapabilities, AgentDescriptor, AgentHealth};

/// 从内部入口提交给 Agent 适配器的版本化事件包。
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct AgentEventEnvelope {
    pub request_id: RequestId,
    pub agent_id: AgentId,
    pub payload: serde_json::Value,
}

/// Agent 适配器将原始事件标准化后交给应用的统一结构。
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct NormalizedAgentEvent {
    pub idempotency_key: Option<String>,
    pub occurred_at: Timestamp,
    pub session_id: Option<AgentSessionId>,
    pub session_title: Option<String>,
    pub title: String,
    pub body: String,
    pub metadata: NotificationMetadata,
}

/// resume 只表示 Agent 已接纳请求，不表示回答已经完成。
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct ResumeReceipt {
    pub session_id: AgentSessionId,
}

/// Agent 适配器的稳定失败分类。
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum AgentError {
    InvalidInput,
    UnsupportedCapability,
    InvalidEvent,
    Ignored(SafeError),
    Failed(SafeError),
    Unavailable(SafeError),
    Unknown(SafeError),
}

impl AgentError {
    pub fn code(&self) -> &str {
        match self {
            Self::InvalidInput => "agent_invalid_input",
            Self::UnsupportedCapability => "agent_unsupported_capability",
            Self::InvalidEvent => "agent_invalid_event",
            Self::Ignored(error)
            | Self::Failed(error)
            | Self::Unavailable(error)
            | Self::Unknown(error) => error.code(),
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Self::InvalidInput => "Agent 输入无效",
            Self::UnsupportedCapability => "当前 Agent 不支持该能力",
            Self::InvalidEvent => "Agent 事件格式无效",
            Self::Ignored(error)
            | Self::Failed(error)
            | Self::Unavailable(error)
            | Self::Unknown(error) => error.message(),
        }
    }
}

impl Display for AgentError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.message().fmt(formatter)
    }
}

impl std::error::Error for AgentError {}

/// 所有 Agent 适配器必须实现的稳定边界。
#[async_trait::async_trait]
pub trait AgentAdapter: Send + Sync {
    fn descriptor(&self) -> AgentDescriptor;

    fn capabilities(&self) -> AgentCapabilities;

    fn parse_event(&self, envelope: AgentEventEnvelope)
    -> Result<NormalizedAgentEvent, AgentError>;

    async fn resume(
        &self,
        session_id: &AgentSessionId,
        text: &str,
    ) -> Result<ResumeReceipt, AgentError>;

    async fn inspect(&self) -> AgentHealth;
}
