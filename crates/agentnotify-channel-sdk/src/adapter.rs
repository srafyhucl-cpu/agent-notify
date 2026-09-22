use std::{collections::BTreeMap, fmt::Display};

use agentnotify_domain::{
    DeliveryState, ExternalMessageId, InboundMessage, NotificationMetadata, SafeError, Timestamp,
};
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::{ChannelAccount, ChannelCapabilities, ChannelDescriptor, ChannelHealth};

/// 入站消息统一提交到应用层的有界通道。
pub type InboundEmitter = tokio::sync::mpsc::Sender<InboundMessage>;

/// 出站消息在核心中的用途，不包含渠道专属字段。
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum MessagePurpose {
    Notification,
    Reply,
    ReplyConfirmation,
    ReplyRejection,
}

/// 通知的结构化展示信息：渠道有渲染器时据此还原标题栏、页脚与引用提示。
///
/// 由应用层从 Agent 注册表与通知事实组装；`OutboundMessage` 不携带它时，
/// 渠道必须按 `text` 原样发送，保证旧调用方与未接入渲染的渠道行为不变。
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct NotificationPresentation {
    /// Agent 注册表 descriptor 里的显示名。
    pub agent_display_name: String,
    /// 会话名：通知的会话标题，缺省时回退为通知标题。
    pub session_name: String,
    /// 事件发生时间，用于页脚时间戳。
    pub occurred_at: Timestamp,
    /// 是否输出页脚；与 Go 版一致，通知默认带页脚。
    pub include_footer: bool,
    /// 是否声明可引用续聊；只表示原通知有会话号且 Agent 支持续聊，不代表回复路由已建立。
    pub replyable: bool,
}

/// 发往单个渠道账号的标准化消息。
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct OutboundMessage {
    pub purpose: MessagePurpose,
    pub conversation_id: String,
    pub text: String,
    pub client_id: String,
    pub reply_to: Option<ExternalMessageId>,
    /// 结构化通知信息；只有通知用途携带，`None` 时渠道按 `text` 原样发送。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notification: Option<NotificationPresentation>,
    pub safe_metadata: BTreeMap<String, String>,
}

impl OutboundMessage {
    pub fn notification(
        conversation_id: impl Into<String>,
        text: impl Into<String>,
        client_id: impl Into<String>,
    ) -> Result<Self, ChannelError> {
        Self::new(
            MessagePurpose::Notification,
            conversation_id.into(),
            text.into(),
            client_id.into(),
            None,
        )
    }

    pub fn reply(
        conversation_id: impl Into<String>,
        text: impl Into<String>,
        client_id: impl Into<String>,
        reply_to: ExternalMessageId,
    ) -> Result<Self, ChannelError> {
        Self::new(
            MessagePurpose::Reply,
            conversation_id.into(),
            text.into(),
            client_id.into(),
            Some(reply_to),
        )
    }

    /// 附加结构化通知信息；由投递层在能取到 Agent 显示名与会话名时填充。
    pub fn with_notification(mut self, notification: NotificationPresentation) -> Self {
        self.notification = Some(notification);
        self
    }

    fn new(
        purpose: MessagePurpose,
        conversation_id: String,
        text: String,
        client_id: String,
        reply_to: Option<ExternalMessageId>,
    ) -> Result<Self, ChannelError> {
        if conversation_id.trim().is_empty()
            || text.trim().is_empty()
            || client_id.trim().is_empty()
        {
            return Err(ChannelError::permanent(
                "invalid_outbound_message",
                "渠道消息的会话、正文或客户端标识无效",
            ));
        }
        Ok(Self {
            purpose,
            conversation_id,
            text,
            client_id,
            reply_to,
            notification: None,
            safe_metadata: BTreeMap::new(),
        })
    }
}

/// 渠道发送后可供核心持久化的安全回执。
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct DeliveryReceipt {
    pub external_message_id: Option<ExternalMessageId>,
    pub external_thread_id: Option<String>,
    pub state: DeliveryState,
    pub error: Option<SafeError>,
    pub raw_safe_metadata: BTreeMap<String, String>,
}

impl DeliveryReceipt {
    pub fn sent(external_message_id: ExternalMessageId) -> Self {
        Self {
            external_message_id: Some(external_message_id),
            external_thread_id: None,
            state: DeliveryState::Sent,
            error: None,
            raw_safe_metadata: BTreeMap::new(),
        }
    }

    pub fn unknown(error: SafeError) -> Self {
        Self {
            external_message_id: None,
            external_thread_id: None,
            state: DeliveryState::Unknown,
            error: Some(error),
            raw_safe_metadata: BTreeMap::new(),
        }
    }

    pub fn skipped(error: SafeError) -> Self {
        Self {
            external_message_id: None,
            external_thread_id: None,
            state: DeliveryState::Skipped,
            error: Some(error),
            raw_safe_metadata: BTreeMap::new(),
        }
    }

    pub fn is_valid_for(&self, capabilities: &ChannelCapabilities) -> bool {
        let state_is_valid = match self.state {
            DeliveryState::Sent => {
                self.error.is_none()
                    && (!capabilities.reply_routing || self.external_message_id.is_some())
            }
            DeliveryState::Unknown | DeliveryState::Skipped => self.error.is_some(),
            DeliveryState::Pending | DeliveryState::Failed => false,
        };
        state_is_valid
            && NotificationMetadata::new(self.raw_safe_metadata.clone()).is_ok()
            && self
                .external_thread_id
                .as_deref()
                .is_none_or(|value| !value.trim().is_empty())
    }
}

/// 渠道适配器的稳定失败分类。
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum ChannelError {
    Permanent(SafeError),
    Retryable {
        error: SafeError,
        retry_after: Option<time::Duration>,
    },
    Unknown(SafeError),
    InvalidAccount(SafeError),
    UnsupportedCapability(SafeError),
}

impl ChannelError {
    pub fn permanent(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Permanent(safe_error(code, message))
    }

    pub fn retryable(
        code: impl Into<String>,
        message: impl Into<String>,
        retry_after: Option<time::Duration>,
    ) -> Self {
        Self::Retryable {
            error: safe_error(code, message),
            retry_after,
        }
    }

    pub fn unknown(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Unknown(safe_error(code, message))
    }

    pub fn invalid_account(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::InvalidAccount(safe_error(code, message))
    }

    pub fn unsupported_capability(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::UnsupportedCapability(safe_error(code, message))
    }

    pub fn code(&self) -> &str {
        self.error().code()
    }

    pub fn message(&self) -> &str {
        self.error().message()
    }

    pub const fn is_retryable(&self) -> bool {
        matches!(self, Self::Retryable { .. })
    }

    pub fn retry_after(&self) -> Option<time::Duration> {
        match self {
            Self::Retryable { retry_after, .. } => *retry_after,
            Self::Permanent(_)
            | Self::Unknown(_)
            | Self::InvalidAccount(_)
            | Self::UnsupportedCapability(_) => None,
        }
    }

    fn error(&self) -> &SafeError {
        match self {
            Self::Permanent(error)
            | Self::Unknown(error)
            | Self::InvalidAccount(error)
            | Self::UnsupportedCapability(error) => error,
            Self::Retryable { error, .. } => error,
        }
    }
}

impl Display for ChannelError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.message().fmt(formatter)
    }
}

impl std::error::Error for ChannelError {}

/// 单个渠道账号的后台任务与显式取消句柄。
pub struct ChannelTask {
    handle: Option<JoinHandle<Result<(), ChannelError>>>,
    cancel: Option<watch::Sender<bool>>,
}

impl ChannelTask {
    pub fn new(handle: JoinHandle<Result<(), ChannelError>>, cancel: watch::Sender<bool>) -> Self {
        Self {
            handle: Some(handle),
            cancel: Some(cancel),
        }
    }

    pub const fn completed() -> Self {
        Self {
            handle: None,
            cancel: None,
        }
    }

    pub fn is_finished(&self) -> bool {
        self.handle
            .as_ref()
            .is_none_or(tokio::task::JoinHandle::is_finished)
    }

    pub fn cancel(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            let _ = cancel.send(true);
        }
    }

    pub async fn shutdown(mut self) -> Result<(), ChannelError> {
        self.cancel();
        match self.handle.take() {
            Some(handle) => handle.await.map_err(|_| {
                ChannelError::unknown("channel_task_failed", "渠道后台任务意外退出")
            })?,
            None => Ok(()),
        }
    }
}

impl Drop for ChannelTask {
    fn drop(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            let _ = cancel.send(true);
        }
    }
}

/// 所有渠道适配器必须实现的稳定边界。
#[async_trait::async_trait]
pub trait ChannelAdapter: Send + Sync {
    fn descriptor(&self) -> ChannelDescriptor;

    fn capabilities(&self) -> ChannelCapabilities;

    async fn start(
        &self,
        account: ChannelAccount,
        emit: InboundEmitter,
    ) -> Result<ChannelTask, ChannelError>;

    async fn send(
        &self,
        account: ChannelAccount,
        message: OutboundMessage,
    ) -> Result<DeliveryReceipt, ChannelError>;

    async fn inspect(&self, account: ChannelAccount) -> ChannelHealth;

    async fn logout(&self, account: ChannelAccount) -> Result<(), ChannelError>;
}

fn safe_error(code: impl Into<String>, message: impl Into<String>) -> SafeError {
    SafeError::new(code, message).expect("渠道适配器错误常量必须有效")
}
