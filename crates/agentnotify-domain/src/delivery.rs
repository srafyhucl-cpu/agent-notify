use crate::{
    ChannelAccountId, ChannelId, DeliveryId, DomainError, ExternalMessageId, NotificationId,
};

const MAX_ERROR_CODE_LENGTH: usize = 64;
const MAX_ERROR_MESSAGE_LENGTH: usize = 512;

/// 投递结果在状态机中的稳定状态。
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum DeliveryState {
    Pending,
    Sent,
    Failed,
    Unknown,
    Skipped,
}

impl DeliveryState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "Pending",
            Self::Sent => "Sent",
            Self::Failed => "Failed",
            Self::Unknown => "Unknown",
            Self::Skipped => "Skipped",
        }
    }
}

/// 投递失败在重试策略中的稳定分类。
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum DeliveryErrorKind {
    Retryable,
    Permanent,
    Unknown,
    Skipped,
}

impl DeliveryErrorKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Retryable => "Retryable",
            Self::Permanent => "Permanent",
            Self::Unknown => "Unknown",
            Self::Skipped => "Skipped",
        }
    }

    pub const fn is_retryable(self) -> bool {
        matches!(self, Self::Retryable)
    }
}

/// 渠道回执中的结果类型。
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum DeliveryReceiptState {
    Sent,
    Unknown,
    Skipped,
}

/// 可持久化的安全错误信息。
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct SafeError {
    code: String,
    message: String,
}

impl SafeError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Result<Self, DomainError> {
        let code = code.into();
        let message = message.into();
        if code.is_empty()
            || code.len() > MAX_ERROR_CODE_LENGTH
            || code.trim() != code
            || !code.chars().all(|character| {
                character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_'
            })
        {
            return Err(DomainError::InvalidValue {
                field: "error_code",
            });
        }
        if message.trim().is_empty()
            || message.len() > MAX_ERROR_MESSAGE_LENGTH
            || message.contains('\0')
        {
            return Err(DomainError::InvalidValue {
                field: "error_message",
            });
        }
        Ok(Self { code, message })
    }

    pub fn code(&self) -> &str {
        &self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

/// 一次通知到指定渠道账号的投递。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Delivery {
    id: DeliveryId,
    notification_id: NotificationId,
    channel_id: ChannelId,
    account_id: ChannelAccountId,
    state: DeliveryState,
    external_message_id: Option<ExternalMessageId>,
    error: Option<SafeError>,
    error_kind: Option<DeliveryErrorKind>,
}

impl Delivery {
    pub fn pending(
        id: DeliveryId,
        notification_id: NotificationId,
        channel_id: ChannelId,
        account_id: ChannelAccountId,
    ) -> Self {
        Self {
            id,
            notification_id,
            channel_id,
            account_id,
            state: DeliveryState::Pending,
            external_message_id: None,
            error: None,
            error_kind: None,
        }
    }

    pub fn id(&self) -> &DeliveryId {
        &self.id
    }

    pub fn notification_id(&self) -> &NotificationId {
        &self.notification_id
    }

    pub fn channel_id(&self) -> &ChannelId {
        &self.channel_id
    }

    pub fn account_id(&self) -> &ChannelAccountId {
        &self.account_id
    }

    pub fn state(&self) -> DeliveryState {
        self.state
    }

    pub fn external_message_id(&self) -> Option<&ExternalMessageId> {
        self.external_message_id.as_ref()
    }

    pub fn error(&self) -> Option<&SafeError> {
        self.error.as_ref()
    }

    pub fn error_kind(&self) -> Option<DeliveryErrorKind> {
        self.error_kind
    }

    pub fn can_retry(&self) -> bool {
        match self.state {
            DeliveryState::Pending => true,
            DeliveryState::Failed => self.error_kind.is_some_and(DeliveryErrorKind::is_retryable),
            DeliveryState::Sent | DeliveryState::Unknown | DeliveryState::Skipped => false,
        }
    }

    pub fn mark_sent(&mut self, external_message_id: ExternalMessageId) -> Result<(), DomainError> {
        self.ensure_transition_allowed()?;
        self.state = DeliveryState::Sent;
        self.external_message_id = Some(external_message_id);
        self.error = None;
        self.error_kind = None;
        Ok(())
    }

    pub fn mark_retryable(&mut self, error: SafeError) -> Result<(), DomainError> {
        self.ensure_transition_allowed()?;
        self.state = DeliveryState::Failed;
        self.external_message_id = None;
        self.error = Some(error);
        self.error_kind = Some(DeliveryErrorKind::Retryable);
        Ok(())
    }

    pub fn mark_permanent_failure(&mut self, error: SafeError) -> Result<(), DomainError> {
        self.ensure_transition_allowed()?;
        self.state = DeliveryState::Failed;
        self.external_message_id = None;
        self.error = Some(error);
        self.error_kind = Some(DeliveryErrorKind::Permanent);
        Ok(())
    }

    pub fn mark_unknown(&mut self, error: SafeError) -> Result<(), DomainError> {
        self.ensure_transition_allowed()?;
        self.state = DeliveryState::Unknown;
        self.external_message_id = None;
        self.error = Some(error);
        self.error_kind = Some(DeliveryErrorKind::Unknown);
        Ok(())
    }

    pub fn mark_skipped(&mut self, error: SafeError) -> Result<(), DomainError> {
        self.ensure_transition_allowed()?;
        self.state = DeliveryState::Skipped;
        self.external_message_id = None;
        self.error = Some(error);
        self.error_kind = Some(DeliveryErrorKind::Skipped);
        Ok(())
    }

    fn ensure_transition_allowed(&self) -> Result<(), DomainError> {
        match self.state {
            DeliveryState::Pending => Ok(()),
            DeliveryState::Failed
                if self.error_kind.is_some_and(DeliveryErrorKind::is_retryable) =>
            {
                Ok(())
            }
            DeliveryState::Failed
            | DeliveryState::Sent
            | DeliveryState::Unknown
            | DeliveryState::Skipped => Err(DomainError::InvalidStateTransition),
        }
    }
}
