use std::{collections::HashSet, fmt::Display, sync::Arc};

use agentnotify_agent_sdk::{AgentError, AgentRegistry};
use agentnotify_channel_sdk::{
    ChannelAccount, ChannelError, ChannelRegistry, DeliveryReceipt, MessagePurpose, OutboundMessage,
};
use agentnotify_domain::{
    AgentId, AgentSessionId, ClaimKey, ClaimOutcome, ClaimState, DomainError, InboundClaim,
    InboundMessage, ReplyRoute, RouteKey, SafeError, Timestamp,
};
use time::Duration;

use crate::{ClaimStore, Clock, EventSink, RouteStore, StatusStore, StoreError, UseCase};

const DEFAULT_CLAIM_TTL_SECONDS: i64 = 24 * 60 * 60;
const DEFAULT_CONFIRMATION_TEXT: &str = "已收到，正在处理";

/// 引用回复的全局策略；新功能默认关闭。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplyConfig {
    pub enabled: bool,
    pub send_confirmation: bool,
    pub claim_ttl: Duration,
    pub confirmation_text: String,
}

impl Default for ReplyConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            send_confirmation: false,
            claim_ttl: Duration::seconds(DEFAULT_CLAIM_TTL_SECONDS),
            confirmation_text: DEFAULT_CONFIRMATION_TEXT.into(),
        }
    }
}

/// 单个渠道账号允许接收的私聊回复范围。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplyTarget {
    pub account: ChannelAccount,
    pub bound_sender_id: String,
    pub private_conversation_id: String,
}

impl ReplyTarget {
    pub fn new(
        account: ChannelAccount,
        bound_sender_id: impl Into<String>,
        private_conversation_id: impl Into<String>,
    ) -> Result<Self, ReplyError> {
        let bound_sender_id = bound_sender_id.into().trim().to_owned();
        if bound_sender_id.is_empty() {
            return Err(ReplyError::InvalidConfiguration {
                field: "bound_sender_id",
            });
        }
        let private_conversation_id = private_conversation_id.into().trim().to_owned();
        if private_conversation_id.is_empty() {
            return Err(ReplyError::InvalidConfiguration {
                field: "private_conversation_id",
            });
        }
        Ok(Self {
            account,
            bound_sender_id,
            private_conversation_id,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReplyOutcome {
    Accepted {
        agent_id: AgentId,
        session_id: AgentSessionId,
    },
    AlreadyClaimed {
        state: ClaimState,
        updated_at: Timestamp,
    },
    Rejected(ReplyRejection),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReplyRejection {
    Disabled,
    AccountNotConfigured,
    AccountDisabled,
    AccountChannelMismatch,
    ChannelMissing,
    ChannelUnsupported,
    SenderNotAllowed,
    ConversationNotAllowed,
    EmptyText,
    TextTooLarge,
    AmbiguousRoute,
    NoExactRoute,
    AgentMissing,
    AgentUnsupported,
    AgentFailed(SafeError),
    AgentUnknown(SafeError),
}

impl ReplyRejection {
    pub fn code(&self) -> &str {
        match self {
            Self::Disabled => "reply_disabled",
            Self::AccountNotConfigured => "reply_account_not_configured",
            Self::AccountDisabled => "reply_account_disabled",
            Self::AccountChannelMismatch => "reply_account_channel_mismatch",
            Self::ChannelMissing => "reply_channel_missing",
            Self::ChannelUnsupported => "reply_channel_unsupported",
            Self::SenderNotAllowed => "reply_sender_not_allowed",
            Self::ConversationNotAllowed => "reply_conversation_not_allowed",
            Self::EmptyText => "reply_empty_text",
            Self::TextTooLarge => "reply_text_too_large",
            Self::AmbiguousRoute => "reply_ambiguous_route",
            Self::NoExactRoute => "reply_exact_route_missing",
            Self::AgentMissing => "reply_agent_missing",
            Self::AgentUnsupported => "reply_agent_unsupported",
            Self::AgentFailed(error) | Self::AgentUnknown(error) => error.code(),
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Self::Disabled => "引用回复功能未启用",
            Self::AccountNotConfigured => "该渠道账号未配置引用回复",
            Self::AccountDisabled => "该渠道账号已停用",
            Self::AccountChannelMismatch => "入站消息与渠道账号不匹配",
            Self::ChannelMissing => "找不到对应的渠道适配器",
            Self::ChannelUnsupported => "当前渠道不支持引用回复",
            Self::SenderNotAllowed => "只有账号绑定用户可以回复通知",
            Self::ConversationNotAllowed => "只允许在绑定的私聊会话中回复通知",
            Self::EmptyText => "回复正文不能为空",
            Self::TextTooLarge => "回复正文超过渠道上限",
            Self::AmbiguousRoute => "回复引用了多条消息，无法确定目标会话",
            Self::NoExactRoute => "找不到对应的通知，请直接引用本次通知后回复",
            Self::AgentMissing => "原通知对应的 Agent 当前不可用",
            Self::AgentUnsupported => "原通知对应的 Agent 不支持继续会话",
            Self::AgentFailed(error) | Self::AgentUnknown(error) => error.message(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReplyError {
    Domain(DomainError),
    Store(StoreError),
    InvalidConfiguration { field: &'static str },
    InvalidClaimWindow,
}

impl ReplyError {
    pub fn code(&self) -> &str {
        match self {
            Self::Domain(error) => error.code(),
            Self::Store(error) => error.code(),
            Self::InvalidConfiguration { field } => field,
            Self::InvalidClaimWindow => "reply_invalid_claim_window",
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Self::Domain(error) => error.message(),
            Self::Store(error) => error.message(),
            Self::InvalidConfiguration { .. } => "引用回复配置无效",
            Self::InvalidClaimWindow => "引用回复的去重时间窗口无效",
        }
    }
}

impl Display for ReplyError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.message())
    }
}

impl std::error::Error for ReplyError {}

impl From<DomainError> for ReplyError {
    fn from(value: DomainError) -> Self {
        Self::Domain(value)
    }
}

impl From<StoreError> for ReplyError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}

/// 将精确引用的入站消息恢复至原 Agent 会话。
pub struct ReplyService {
    claim_store: Arc<dyn ClaimStore>,
    route_store: Arc<dyn RouteStore>,
    channels: Arc<ChannelRegistry>,
    agents: Arc<AgentRegistry>,
    clock: Arc<dyn Clock>,
    event_sink: Arc<dyn EventSink>,
    targets: Vec<ReplyTarget>,
    config: ReplyConfig,
    status_store: Option<Arc<dyn StatusStore>>,
}

impl ReplyService {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        claim_store: Arc<dyn ClaimStore>,
        route_store: Arc<dyn RouteStore>,
        channels: Arc<ChannelRegistry>,
        agents: Arc<AgentRegistry>,
        clock: Arc<dyn Clock>,
        event_sink: Arc<dyn EventSink>,
        targets: Vec<ReplyTarget>,
        config: ReplyConfig,
        status_store: Option<Arc<dyn StatusStore>>,
    ) -> Result<Self, ReplyError> {
        if config.claim_ttl <= Duration::ZERO {
            return Err(ReplyError::InvalidConfiguration { field: "claim_ttl" });
        }
        if config.send_confirmation && config.confirmation_text.trim().is_empty() {
            return Err(ReplyError::InvalidConfiguration {
                field: "confirmation_text",
            });
        }
        let mut account_ids = HashSet::with_capacity(targets.len());
        for target in &targets {
            if !account_ids.insert(target.account.id.clone()) {
                return Err(ReplyError::InvalidConfiguration {
                    field: "reply_target",
                });
            }
        }
        Ok(Self {
            claim_store,
            route_store,
            channels,
            agents,
            clock,
            event_sink,
            targets,
            config,
            status_store,
        })
    }

    pub fn with_status_store(mut self, status_store: Arc<dyn StatusStore>) -> Self {
        self.status_store = Some(status_store);
        self
    }

    #[tracing::instrument(
        name = "reply",
        skip_all,
        fields(
            intent_hash = %crate::observability::hash_identifier(message.id.as_str()),
            route_result = tracing::field::Empty,
            agent = tracing::field::Empty,
            claim_state = tracing::field::Empty,
        )
    )]
    pub async fn handle(&self, message: InboundMessage) -> Result<ReplyOutcome, ReplyError> {
        let result = self.handle_inner(message).await;
        match &result {
            Ok(ReplyOutcome::Accepted { agent_id, .. }) => {
                tracing::Span::current().record("route_result", "accepted");
                tracing::Span::current().record("agent", tracing::field::display(agent_id));
                tracing::Span::current().record("claim_state", "Completed");
            }
            Ok(ReplyOutcome::AlreadyClaimed { state, .. }) => {
                tracing::Span::current().record("route_result", "already_claimed");
                tracing::Span::current().record("claim_state", state.as_str());
            }
            Ok(ReplyOutcome::Rejected(rejection)) => {
                tracing::Span::current().record("route_result", rejection.code());
                let claim_state = match rejection {
                    ReplyRejection::AgentUnknown(_) => "Unknown",
                    ReplyRejection::NoExactRoute
                    | ReplyRejection::AmbiguousRoute
                    | ReplyRejection::AgentMissing
                    | ReplyRejection::AgentUnsupported
                    | ReplyRejection::AgentFailed(_) => "Failed",
                    _ => "none",
                };
                tracing::Span::current().record("claim_state", claim_state);
            }
            Err(error) => {
                tracing::Span::current().record("route_result", error.code());
            }
        }
        result
    }

    async fn handle_inner(&self, message: InboundMessage) -> Result<ReplyOutcome, ReplyError> {
        if !self.config.enabled {
            return Ok(ReplyOutcome::Rejected(ReplyRejection::Disabled));
        }
        let Some(target) = self
            .targets
            .iter()
            .find(|target| target.account.id == message.account_id)
        else {
            return Ok(ReplyOutcome::Rejected(ReplyRejection::AccountNotConfigured));
        };
        if !target.account.enabled {
            return Ok(ReplyOutcome::Rejected(ReplyRejection::AccountDisabled));
        }
        if target.account.channel_id != message.channel_id {
            return Ok(ReplyOutcome::Rejected(
                ReplyRejection::AccountChannelMismatch,
            ));
        }
        if target.bound_sender_id != message.sender_id.trim() {
            return Ok(ReplyOutcome::Rejected(ReplyRejection::SenderNotAllowed));
        }
        if target.private_conversation_id != message.conversation_id.trim() {
            return Ok(ReplyOutcome::Rejected(
                ReplyRejection::ConversationNotAllowed,
            ));
        }

        let text = message.text.trim();
        if text.is_empty() {
            return Ok(ReplyOutcome::Rejected(ReplyRejection::EmptyText));
        }
        let Some(channel) = self.channels.get(&target.account.channel_id) else {
            return Ok(ReplyOutcome::Rejected(ReplyRejection::ChannelMissing));
        };
        let capabilities = channel.capabilities();
        if !capabilities.receive || !capabilities.reply_routing {
            return Ok(ReplyOutcome::Rejected(ReplyRejection::ChannelUnsupported));
        }
        if capabilities
            .max_text_bytes
            .is_some_and(|max_text_bytes| message.text.len() > max_text_bytes)
        {
            return Ok(ReplyOutcome::Rejected(ReplyRejection::TextTooLarge));
        }

        let expires_at = message
            .received_at
            .checked_add(self.config.claim_ttl)
            .ok_or(ReplyError::InvalidClaimWindow)?;
        let claim = InboundClaim::from_inbound(&message, expires_at)?;
        let mut claim = match self.claim_store.claim(claim).await? {
            ClaimOutcome::Acquired(claim) => claim,
            ClaimOutcome::AlreadyClaimed { state, updated_at } => {
                return Ok(ReplyOutcome::AlreadyClaimed { state, updated_at });
            }
        };

        let route = match self.resolve_route(&message).await? {
            Ok(route) => route,
            Err(rejection) => {
                return self.reject_after_claim(&mut claim, rejection).await;
            }
        };
        let Some(agent) = self.agents.get(&route.agent_id) else {
            return self
                .reject_after_claim(&mut claim, ReplyRejection::AgentMissing)
                .await;
        };
        if !agent.capabilities().resume {
            return self
                .reject_after_claim(&mut claim, ReplyRejection::AgentUnsupported)
                .await;
        }

        match agent.resume(&route.session_id, text).await {
            Ok(receipt) if receipt.session_id == route.session_id => {
                claim.mark_completed(self.clock.now())?;
                self.claim_store.update_claim(claim).await?;
                let claim_key = message.claim_key()?;
                self.event_sink.reply_changed(&claim_key).await;
                self.send_confirmation(target, &message, &claim_key).await;
                Ok(ReplyOutcome::Accepted {
                    agent_id: route.agent_id,
                    session_id: route.session_id,
                })
            }
            Ok(_) => {
                let error = safe_error(
                    "agent_session_mismatch",
                    "Agent 返回了与目标不一致的会话标识",
                )?;
                claim.mark_unknown(self.clock.now())?;
                self.claim_store.update_claim(claim).await?;
                let claim_key = message.claim_key()?;
                self.event_sink.reply_changed(&claim_key).await;
                Ok(ReplyOutcome::Rejected(ReplyRejection::AgentUnknown(error)))
            }
            Err(AgentError::Unknown(error)) => {
                claim.mark_unknown(self.clock.now())?;
                self.claim_store.update_claim(claim).await?;
                let claim_key = message.claim_key()?;
                self.event_sink.reply_changed(&claim_key).await;
                Ok(ReplyOutcome::Rejected(ReplyRejection::AgentUnknown(error)))
            }
            Err(AgentError::UnsupportedCapability) => {
                self.reject_after_claim(&mut claim, ReplyRejection::AgentUnsupported)
                    .await
            }
            Err(error) => {
                let rejection = ReplyRejection::AgentFailed(agent_error_safe(error)?);
                self.reject_after_claim(&mut claim, rejection).await
            }
        }
    }

    async fn resolve_route(
        &self,
        message: &InboundMessage,
    ) -> Result<Result<ReplyRoute, ReplyRejection>, ReplyError> {
        let referenced_message_id = match message.referenced_message_ids.as_slice() {
            [referenced_message_id] => referenced_message_id.clone(),
            [] => return Ok(Err(ReplyRejection::NoExactRoute)),
            _ => return Ok(Err(ReplyRejection::AmbiguousRoute)),
        };
        let key = RouteKey::new(
            message.channel_id.clone(),
            message.account_id.clone(),
            referenced_message_id,
        );
        Ok(
            match self.route_store.find_route(&key, self.clock.now()).await? {
                Some(route) => Ok(route),
                None => Err(ReplyRejection::NoExactRoute),
            },
        )
    }

    async fn reject_after_claim(
        &self,
        claim: &mut InboundClaim,
        rejection: ReplyRejection,
    ) -> Result<ReplyOutcome, ReplyError> {
        claim.mark_failed(self.clock.now())?;
        self.claim_store.update_claim(claim.clone()).await?;
        self.event_sink.reply_changed(&claim.key).await;
        Ok(ReplyOutcome::Rejected(rejection))
    }

    async fn send_confirmation(
        &self,
        target: &ReplyTarget,
        message: &InboundMessage,
        claim_key: &ClaimKey,
    ) {
        if !self.config.send_confirmation {
            return;
        }
        let Some(channel) = self.channels.get(&target.account.channel_id) else {
            return;
        };
        let capabilities = channel.capabilities();
        let outbound = OutboundMessage {
            purpose: MessagePurpose::ReplyConfirmation,
            conversation_id: message.conversation_id.clone(),
            text: self.config.confirmation_text.clone(),
            client_id: format!("reply-confirmation-{claim_key}"),
            reply_to: message.external_message_id.clone(),
            safe_metadata: Default::default(),
        };
        match channel.send(target.account.clone(), outbound).await {
            Ok(receipt) if valid_confirmation_receipt(&receipt, &capabilities) => {}
            Ok(_) => {
                self.record_confirmation_error(
                    SafeError::new("reply_confirmation_unconfirmed", "渠道未确认回复送达提示")
                        .expect("内置安全错误必须有效"),
                )
                .await;
            }
            Err(error) => {
                self.record_confirmation_error(channel_error_safe(error))
                    .await;
            }
        }
    }

    async fn record_confirmation_error(&self, error: SafeError) {
        tracing::warn!(
            code = error.code(),
            "回复送达确认失败，不影响已完成的 Agent 接纳"
        );
        if let Some(status_store) = &self.status_store {
            if let Err(store_error) = status_store.record_error(error).await {
                tracing::warn!(code = store_error.code(), "记录回复送达确认错误失败");
            }
        }
    }
}

impl UseCase for ReplyService {
    fn area(&self) -> agentnotify_domain::DomainArea {
        agentnotify_domain::DomainArea::Reply
    }

    fn name(&self) -> &'static str {
        "reply"
    }
}

fn valid_confirmation_receipt(
    receipt: &DeliveryReceipt,
    capabilities: &agentnotify_channel_sdk::ChannelCapabilities,
) -> bool {
    receipt.state == agentnotify_domain::DeliveryState::Sent && receipt.is_valid_for(capabilities)
}

fn agent_error_safe(error: AgentError) -> Result<SafeError, ReplyError> {
    SafeError::new(error.code(), error.message()).map_err(ReplyError::from)
}

fn channel_error_safe(error: ChannelError) -> SafeError {
    SafeError::new(error.code(), error.message()).expect("渠道适配器错误常量必须有效")
}

fn safe_error(code: &str, message: &str) -> Result<SafeError, ReplyError> {
    SafeError::new(code, message).map_err(ReplyError::from)
}
