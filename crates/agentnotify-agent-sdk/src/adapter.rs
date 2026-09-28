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

/// 编排派活选项（§4「派活透传」）：工作目录、该步模型、无人值守标志。
///
/// 支持「开新会话带选项」的适配器（OpenCode）按需使用；其它适配器默认忽略
/// （[`AgentAdapter::dispatch_with_options`] 的默认实现）。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DispatchOptions {
    /// 会话工作目录（绝对路径）；None = 跟随宿主当前项目。
    pub working_dir: Option<String>,
    /// 该步模型（`provider/model`）；None = 用该 Agent 默认模型。
    pub model: Option<String>,
    /// 无人值守（编排会话权限 ask 自动放行）；默认 true。
    pub unattended: bool,
}

impl Default for DispatchOptions {
    /// 缺省 = 不指定目录/模型，无人值守开启（与 settings `orchestration.unattended` 默认一致）。
    fn default() -> Self {
        Self {
            working_dir: None,
            model: None,
            unattended: true,
        }
    }
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

    /// 以新会话开工（可选能力，A2A「新任务 = 新会话」，编排派活 Step 1 用）。
    ///
    /// 默认不支持（返回 `UnsupportedCapability`），由编排层降级为 [`AgentAdapter::resume`]
    /// 续聊同一个稳定 session_id 起步；OpenCode 等支持「新会话」语义的适配器覆写本方法。
    async fn open(
        &self,
        _session_id: &AgentSessionId,
        _text: &str,
    ) -> Result<ResumeReceipt, AgentError> {
        Err(AgentError::UnsupportedCapability)
    }

    /// 带派活选项的投递（§4 派活透传：工作目录/模型/无人值守）：
    ///
    /// - 默认实现**丢弃选项**，按 [`AgentAdapter::open`]（`open=true`，不支持新会话时降级
    ///   [`AgentAdapter::resume`]）/ [`AgentAdapter::resume`] 的原语义投递——其它 Agent
    ///   适配器不感知这些选项；
    /// - OpenCode 等支持「新会话 + 指定模型/目录/无人值守」的适配器覆写本方法。
    async fn dispatch_with_options(
        &self,
        session_id: &AgentSessionId,
        text: &str,
        open: bool,
        options: &DispatchOptions,
    ) -> Result<ResumeReceipt, AgentError> {
        let _ = options;
        if open {
            match self.open(session_id, text).await {
                Ok(receipt) => Ok(receipt),
                Err(AgentError::UnsupportedCapability) => self.resume(session_id, text).await,
                Err(error) => Err(error),
            }
        } else {
            self.resume(session_id, text).await
        }
    }

    async fn inspect(&self) -> AgentHealth;
}
