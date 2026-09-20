use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use agentnotify_agent_sdk::{
    AgentAdapter, AgentCapabilities, AgentDescriptor, AgentError, AgentEventEnvelope, AgentHealth,
    AgentRegistry, NormalizedAgentEvent, ResumeReceipt,
};
use agentnotify_application::Clock;
use agentnotify_application::{
    ClaimStore, EventSink, ReplyConfig, ReplyOutcome, ReplyRejection, ReplyService, ReplyTarget,
    RouteStore, StatusSnapshot, StatusStore, StoreError,
};
use agentnotify_channel_sdk::{
    ChannelAccount, ChannelAdapter, ChannelCapabilities, ChannelDescriptor, ChannelError,
    ChannelHealth, ChannelRegistry, ChannelTask, DeliveryReceipt, InboundEmitter, InboundMode,
    MessagePurpose, OutboundMessage,
};
use agentnotify_domain::{
    AgentId, AgentSessionId, ChannelAccountId, ChannelId, ClaimKey, ClaimOutcome, ClaimState,
    ExternalMessageId, InboundClaim, InboundMessage, InboundMessageInput, NotificationId,
    ReplyRoute, RouteKey, SafeError, Timestamp,
};

#[derive(Default)]
struct TestStore {
    claims: Mutex<HashMap<ClaimKey, InboundClaim>>,
    routes: Mutex<HashMap<RouteKey, ReplyRoute>>,
    events: Mutex<Vec<String>>,
    errors: Mutex<Vec<SafeError>>,
}

impl TestStore {
    fn insert_route(&self, route: ReplyRoute) {
        self.routes.lock().unwrap().insert(route.key.clone(), route);
    }

    fn claim(&self, key: &ClaimKey) -> Option<InboundClaim> {
        self.claims.lock().unwrap().get(key).cloned()
    }

    fn route_count(&self) -> usize {
        self.routes.lock().unwrap().len()
    }

    fn events(&self) -> Vec<String> {
        self.events.lock().unwrap().clone()
    }

    fn errors(&self) -> Vec<SafeError> {
        self.errors.lock().unwrap().clone()
    }
}

#[async_trait::async_trait]
impl ClaimStore for TestStore {
    async fn claim(&self, claim: InboundClaim) -> Result<ClaimOutcome, StoreError> {
        let mut claims = self.claims.lock().unwrap();
        if let Some(existing) = claims.get(&claim.key) {
            return Ok(ClaimOutcome::AlreadyClaimed {
                state: existing.state,
                updated_at: existing.updated_at,
            });
        }
        claims.insert(claim.key.clone(), claim.clone());
        Ok(ClaimOutcome::Acquired(claim))
    }

    async fn update_claim(&self, claim: InboundClaim) -> Result<(), StoreError> {
        let mut claims = self.claims.lock().unwrap();
        if !claims.contains_key(&claim.key) {
            return Err(StoreError::not_found("claim_missing", "找不到入站 Claim"));
        }
        claims.insert(claim.key.clone(), claim);
        Ok(())
    }

    async fn find_claim(&self, key: &ClaimKey) -> Result<Option<InboundClaim>, StoreError> {
        Ok(self.claims.lock().unwrap().get(key).cloned())
    }
}

#[async_trait::async_trait]
impl RouteStore for TestStore {
    async fn find_route(
        &self,
        key: &RouteKey,
        now: Timestamp,
    ) -> Result<Option<ReplyRoute>, StoreError> {
        Ok(self
            .routes
            .lock()
            .unwrap()
            .get(key)
            .filter(|route| route.is_active(now).is_ok())
            .cloned())
    }

    async fn insert_route(&self, route: ReplyRoute) -> Result<(), StoreError> {
        self.insert_route(route);
        Ok(())
    }
}

#[async_trait::async_trait]
impl StatusStore for TestStore {
    async fn snapshot(&self) -> Result<StatusSnapshot, StoreError> {
        Ok(StatusSnapshot::default())
    }

    async fn record_error(&self, error: SafeError) -> Result<(), StoreError> {
        self.errors.lock().unwrap().push(error);
        Ok(())
    }
}

#[async_trait::async_trait]
impl EventSink for TestStore {
    async fn notification_changed(&self, _notification_id: &NotificationId) {}

    async fn delivery_changed(&self, _delivery_id: &agentnotify_domain::DeliveryId) {}

    async fn reply_changed(&self, claim_key: &ClaimKey) {
        self.events
            .lock()
            .unwrap()
            .push(format!("reply:{claim_key}"));
    }

    async fn runtime_stopped(&self) {}
}

#[derive(Clone, Copy)]
enum AgentMode {
    Success,
    Unsupported,
    Unknown,
    Failed,
}

struct TestAgent {
    id: AgentId,
    mode: AgentMode,
    resume_count: Mutex<u64>,
    last_session: Mutex<Option<AgentSessionId>>,
    last_text: Mutex<Option<String>>,
}

impl TestAgent {
    fn new(mode: AgentMode) -> Self {
        Self {
            id: AgentId::new("opencode").unwrap(),
            mode,
            resume_count: Mutex::new(0),
            last_session: Mutex::new(None),
            last_text: Mutex::new(None),
        }
    }

    fn resume_count(&self) -> u64 {
        *self.resume_count.lock().unwrap()
    }

    fn last_text(&self) -> Option<String> {
        self.last_text.lock().unwrap().clone()
    }
}

#[async_trait::async_trait]
impl AgentAdapter for TestAgent {
    fn descriptor(&self) -> AgentDescriptor {
        AgentDescriptor {
            id: self.id.clone(),
            display_name: "Test Agent".into(),
            description: "测试 Agent".into(),
            config_schema: serde_json::json!({"type": "object"}),
        }
    }

    fn capabilities(&self) -> AgentCapabilities {
        AgentCapabilities {
            notify: true,
            resume: !matches!(self.mode, AgentMode::Unsupported),
            session_title: true,
            hook_installer: false,
            reply_window: false,
        }
    }

    fn parse_event(
        &self,
        _envelope: AgentEventEnvelope,
    ) -> Result<NormalizedAgentEvent, AgentError> {
        Err(AgentError::InvalidEvent)
    }

    async fn resume(
        &self,
        session_id: &AgentSessionId,
        text: &str,
    ) -> Result<ResumeReceipt, AgentError> {
        if matches!(self.mode, AgentMode::Unsupported) {
            return Err(AgentError::UnsupportedCapability);
        }
        *self.resume_count.lock().unwrap() += 1;
        *self.last_session.lock().unwrap() = Some(session_id.clone());
        *self.last_text.lock().unwrap() = Some(text.to_owned());
        match self.mode {
            AgentMode::Success => Ok(ResumeReceipt {
                session_id: session_id.clone(),
            }),
            AgentMode::Unsupported => unreachable!(),
            AgentMode::Unknown => Err(AgentError::Unknown(
                SafeError::new("agent_timeout", "Agent 结果无法确认").unwrap(),
            )),
            AgentMode::Failed => Err(AgentError::Failed(
                SafeError::new("agent_rejected", "Agent 拒绝了回复").unwrap(),
            )),
        }
    }

    async fn inspect(&self) -> AgentHealth {
        AgentHealth::healthy()
    }
}

#[derive(Clone, Copy)]
enum ChannelMode {
    Sent,
    PermanentFailure,
}

struct TestChannel {
    id: ChannelId,
    mode: ChannelMode,
    messages: Mutex<Vec<OutboundMessage>>,
}

impl TestChannel {
    fn new(mode: ChannelMode) -> Self {
        Self {
            id: ChannelId::new("clawbot").unwrap(),
            mode,
            messages: Mutex::new(Vec::new()),
        }
    }

    fn messages(&self) -> Vec<OutboundMessage> {
        self.messages.lock().unwrap().clone()
    }
}

#[async_trait::async_trait]
impl ChannelAdapter for TestChannel {
    fn descriptor(&self) -> ChannelDescriptor {
        ChannelDescriptor {
            id: self.id.clone(),
            display_name: "Test Channel".into(),
            config_schema: serde_json::json!({"type": "object"}),
        }
    }

    fn capabilities(&self) -> ChannelCapabilities {
        ChannelCapabilities {
            send_text: true,
            receive: true,
            reply_routing: true,
            edit_message: false,
            attachments: false,
            markdown: false,
            max_text_bytes: Some(32),
            inbound_modes: vec![InboundMode::LongPolling],
        }
    }

    async fn start(
        &self,
        _account: ChannelAccount,
        _emit: InboundEmitter,
    ) -> Result<ChannelTask, ChannelError> {
        Ok(ChannelTask::completed())
    }

    async fn send(
        &self,
        _account: ChannelAccount,
        message: OutboundMessage,
    ) -> Result<DeliveryReceipt, ChannelError> {
        self.messages.lock().unwrap().push(message);
        match self.mode {
            ChannelMode::Sent => Ok(DeliveryReceipt::sent(
                ExternalMessageId::new("confirmation-1").unwrap(),
            )),
            ChannelMode::PermanentFailure => Err(ChannelError::permanent(
                "confirmation_rejected",
                "确认消息发送失败",
            )),
        }
    }

    async fn inspect(&self, _account: ChannelAccount) -> ChannelHealth {
        ChannelHealth::healthy()
    }

    async fn logout(&self, _account: ChannelAccount) -> Result<(), ChannelError> {
        Ok(())
    }
}

struct Fixture {
    service: ReplyService,
    store: Arc<TestStore>,
    agent: Arc<TestAgent>,
    channel: Arc<TestChannel>,
}

fn fixture(agent_mode: AgentMode, channel_mode: ChannelMode, confirmation: bool) -> Fixture {
    let now = timestamp("2026-09-19T09:00:02Z");
    let store = Arc::new(TestStore::default());
    let account_id = ChannelAccountId::new("account-1").unwrap();
    let channel_id = ChannelId::new("clawbot").unwrap();
    let agent_id = AgentId::new("opencode").unwrap();
    let session_id = AgentSessionId::new("session-1").unwrap();
    let external_message_id = ExternalMessageId::new("external-1").unwrap();
    store.insert_route(ReplyRoute::new(
        RouteKey::new(channel_id.clone(), account_id.clone(), external_message_id),
        agent_id,
        session_id,
        now,
        now.checked_add(time::Duration::hours(1)).unwrap(),
    ));
    let agent = Arc::new(TestAgent::new(agent_mode));
    let channel = Arc::new(TestChannel::new(channel_mode));
    let mut agents = AgentRegistry::default();
    agents.register(agent.clone()).unwrap();
    let mut channels = ChannelRegistry::default();
    channels.register(channel.clone()).unwrap();
    let account = ChannelAccount::new(account_id, channel_id, "测试账号", now);
    let service = ReplyService::new(
        store.clone(),
        store.clone(),
        Arc::new(channels),
        Arc::new(agents),
        Arc::new(FixedClock(now)),
        store.clone(),
        vec![ReplyTarget::new(account, "user-1", "user-1").unwrap()],
        ReplyConfig {
            enabled: true,
            send_confirmation: confirmation,
            ..ReplyConfig::default()
        },
        Some(store.clone()),
    )
    .unwrap();
    Fixture {
        service,
        store,
        agent,
        channel,
    }
}

struct FixedClock(Timestamp);

impl Clock for FixedClock {
    fn now(&self) -> Timestamp {
        self.0
    }
}

fn timestamp(value: &str) -> Timestamp {
    Timestamp::parse_rfc3339(value).unwrap()
}

fn inbound(references: Vec<&str>, text: &str) -> InboundMessage {
    InboundMessage::new(
        agentnotify_domain::InboundMessageId::new("inbound-1").unwrap(),
        ChannelId::new("clawbot").unwrap(),
        ChannelAccountId::new("account-1").unwrap(),
        InboundMessageInput {
            external_message_id: ExternalMessageId::new("reply-1").unwrap(),
            sender_id: "user-1".into(),
            conversation_id: "user-1".into(),
            referenced_message_ids: references
                .into_iter()
                .map(|value| ExternalMessageId::new(value).unwrap())
                .collect(),
            text: text.into(),
            received_at: timestamp("2026-09-19T09:00:01Z"),
        },
    )
    .unwrap()
}

#[tokio::test]
async fn missing_message_id_does_not_fall_back_to_recent_route() {
    let fixture = fixture(AgentMode::Success, ChannelMode::Sent, false);
    let message = inbound(Vec::new(), "继续检查");

    let result = fixture.service.handle(message).await.unwrap();

    assert!(matches!(
        result,
        ReplyOutcome::Rejected(ReplyRejection::NoExactRoute)
    ));
    assert_eq!(fixture.agent.resume_count(), 0);
    assert_eq!(
        fixture
            .store
            .claim(&inbound(Vec::new(), "继续检查").claim_key().unwrap())
            .unwrap()
            .state,
        ClaimState::Failed
    );
}

#[tokio::test]
async fn missing_route_sends_visible_notice_without_new_route() {
    let fixture = fixture(AgentMode::Success, ChannelMode::Sent, false);

    let result = fixture
        .service
        .handle(inbound(Vec::new(), "继续检查"))
        .await
        .unwrap();

    assert!(matches!(
        result,
        ReplyOutcome::Rejected(ReplyRejection::NoExactRoute)
    ));
    assert_eq!(fixture.agent.resume_count(), 0);
    assert_eq!(fixture.store.route_count(), 1);
    let messages = fixture.channel.messages();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].purpose, MessagePurpose::ReplyRejection);
    assert_eq!(messages[0].conversation_id, "user-1");
    assert_eq!(messages[0].reply_to.as_ref().unwrap().as_str(), "reply-1");
    assert!(
        messages[0].text.contains("无可用会话记录"),
        "{}",
        messages[0].text
    );
}

#[tokio::test]
async fn ambiguous_reference_sends_visible_notice() {
    let fixture = fixture(AgentMode::Success, ChannelMode::Sent, false);

    let result = fixture
        .service
        .handle(inbound(vec!["external-1", "external-2"], "继续"))
        .await
        .unwrap();

    assert!(matches!(
        result,
        ReplyOutcome::Rejected(ReplyRejection::AmbiguousRoute)
    ));
    assert_eq!(fixture.agent.resume_count(), 0);
    let messages = fixture.channel.messages();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].purpose, MessagePurpose::ReplyRejection);
    assert!(
        messages[0].text.contains("多个会话"),
        "{}",
        messages[0].text
    );
}

#[tokio::test]
async fn unsupported_agent_sends_visible_notice() {
    let fixture = fixture(AgentMode::Unsupported, ChannelMode::Sent, false);

    let result = fixture
        .service
        .handle(inbound(vec!["external-1"], "继续"))
        .await
        .unwrap();

    assert!(matches!(
        result,
        ReplyOutcome::Rejected(ReplyRejection::AgentUnsupported)
    ));
    let messages = fixture.channel.messages();
    assert_eq!(messages.len(), 1);
    assert!(
        messages[0].text.contains("不支持继续会话"),
        "{}",
        messages[0].text
    );
}

#[tokio::test]
async fn unconfirmed_agent_result_sends_one_visible_notice() {
    let fixture = fixture(AgentMode::Unknown, ChannelMode::Sent, false);
    let message = inbound(vec!["external-1"], "继续");

    fixture.service.handle(message.clone()).await.unwrap();
    fixture.service.handle(message).await.unwrap();

    let messages = fixture.channel.messages();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].purpose, MessagePurpose::ReplyRejection);
    assert!(
        messages[0].text.contains("未自动重试"),
        "{}",
        messages[0].text
    );
}

#[tokio::test]
async fn wrong_sender_never_receives_visible_notice() {
    let fixture = fixture(AgentMode::Success, ChannelMode::Sent, false);
    let mut message = inbound(vec!["external-1"], "继续");
    message.sender_id = "stranger".into();

    let result = fixture.service.handle(message).await.unwrap();

    assert!(matches!(
        result,
        ReplyOutcome::Rejected(ReplyRejection::SenderNotAllowed)
    ));
    assert!(fixture.channel.messages().is_empty());
}

#[tokio::test]
async fn notice_send_failure_keeps_failed_claim() {
    let fixture = fixture(AgentMode::Success, ChannelMode::PermanentFailure, false);
    let message = inbound(Vec::new(), "继续检查");

    let result = fixture.service.handle(message.clone()).await.unwrap();

    assert!(matches!(
        result,
        ReplyOutcome::Rejected(ReplyRejection::NoExactRoute)
    ));
    assert_eq!(
        fixture
            .store
            .claim(&message.claim_key().unwrap())
            .unwrap()
            .state,
        ClaimState::Failed
    );
    assert_eq!(fixture.store.errors().len(), 1);
}

#[tokio::test]
async fn duplicate_inbound_is_not_submitted_twice() {
    let fixture = fixture(AgentMode::Success, ChannelMode::Sent, false);
    let message = inbound(vec!["external-1"], "  继续检查  ");

    let first = fixture.service.handle(message.clone()).await.unwrap();
    let second = fixture.service.handle(message).await.unwrap();

    assert!(matches!(first, ReplyOutcome::Accepted { .. }));
    assert!(matches!(
        second,
        ReplyOutcome::AlreadyClaimed {
            state: ClaimState::Completed,
            ..
        }
    ));
    assert_eq!(fixture.agent.resume_count(), 1);
    assert_eq!(fixture.agent.last_text().as_deref(), Some("继续检查"));
    assert_eq!(fixture.store.events().len(), 1);
}

#[tokio::test]
async fn ambiguous_references_are_rejected_after_claim() {
    let fixture = fixture(AgentMode::Success, ChannelMode::Sent, false);

    let result = fixture
        .service
        .handle(inbound(vec!["external-1", "external-2"], "继续"))
        .await
        .unwrap();

    assert!(matches!(
        result,
        ReplyOutcome::Rejected(ReplyRejection::AmbiguousRoute)
    ));
    assert_eq!(fixture.agent.resume_count(), 0);
}

#[tokio::test]
async fn wrong_sender_is_rejected_before_claim() {
    let fixture = fixture(AgentMode::Success, ChannelMode::Sent, false);
    let mut message = inbound(vec!["external-1"], "继续");
    message.sender_id = "stranger".into();

    let result = fixture.service.handle(message.clone()).await.unwrap();

    assert!(matches!(
        result,
        ReplyOutcome::Rejected(ReplyRejection::SenderNotAllowed)
    ));
    assert!(fixture.store.claim(&message.claim_key().unwrap()).is_none());
    assert_eq!(fixture.agent.resume_count(), 0);
}

#[tokio::test]
async fn unsupported_agent_marks_claim_failed_without_resume() {
    let fixture = fixture(AgentMode::Unsupported, ChannelMode::Sent, false);

    let result = fixture
        .service
        .handle(inbound(vec!["external-1"], "继续"))
        .await
        .unwrap();

    assert!(matches!(
        result,
        ReplyOutcome::Rejected(ReplyRejection::AgentUnsupported)
    ));
    assert_eq!(fixture.agent.resume_count(), 0);
    assert_eq!(
        fixture
            .store
            .claim(&inbound(vec!["external-1"], "继续").claim_key().unwrap())
            .unwrap()
            .state,
        ClaimState::Failed
    );
}

#[tokio::test]
async fn unknown_agent_result_is_terminal_and_never_retried() {
    let fixture = fixture(AgentMode::Unknown, ChannelMode::Sent, false);
    let message = inbound(vec!["external-1"], "继续");

    let first = fixture.service.handle(message.clone()).await.unwrap();
    let second = fixture.service.handle(message).await.unwrap();

    assert!(matches!(
        first,
        ReplyOutcome::Rejected(ReplyRejection::AgentUnknown(_))
    ));
    assert!(matches!(
        second,
        ReplyOutcome::AlreadyClaimed {
            state: ClaimState::Unknown,
            ..
        }
    ));
    assert_eq!(fixture.agent.resume_count(), 1);
}

#[tokio::test]
async fn failed_agent_result_marks_claim_failed() {
    let fixture = fixture(AgentMode::Failed, ChannelMode::Sent, false);

    let result = fixture
        .service
        .handle(inbound(vec!["external-1"], "继续"))
        .await
        .unwrap();

    assert!(matches!(
        result,
        ReplyOutcome::Rejected(ReplyRejection::AgentFailed(_))
    ));
    assert_eq!(
        fixture
            .store
            .claim(&inbound(vec!["external-1"], "继续").claim_key().unwrap())
            .unwrap()
            .state,
        ClaimState::Failed
    );
}

#[tokio::test]
async fn confirmation_uses_reply_confirmation_purpose_without_new_route() {
    let fixture = fixture(AgentMode::Success, ChannelMode::Sent, true);

    let result = fixture
        .service
        .handle(inbound(vec!["external-1"], "继续"))
        .await
        .unwrap();

    assert!(matches!(result, ReplyOutcome::Accepted { .. }));
    let messages = fixture.channel.messages();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].purpose, MessagePurpose::ReplyConfirmation);
    assert_eq!(messages[0].conversation_id, "user-1");
    assert_eq!(messages[0].reply_to.as_ref().unwrap().as_str(), "reply-1");
    assert_eq!(messages[0].text, "已收到，正在处理");
    assert_eq!(fixture.store.route_count(), 1);
}

#[tokio::test]
async fn confirmation_failure_does_not_change_accepted_claim() {
    let fixture = fixture(AgentMode::Success, ChannelMode::PermanentFailure, true);

    let result = fixture
        .service
        .handle(inbound(vec!["external-1"], "继续"))
        .await
        .unwrap();

    assert!(matches!(result, ReplyOutcome::Accepted { .. }));
    assert_eq!(
        fixture
            .store
            .claim(&inbound(vec!["external-1"], "继续").claim_key().unwrap())
            .unwrap()
            .state,
        ClaimState::Completed
    );
    assert_eq!(fixture.store.errors()[0].code(), "confirmation_rejected");
}

#[tokio::test]
async fn oversized_reply_is_rejected_before_claim() {
    let fixture = fixture(AgentMode::Success, ChannelMode::Sent, false);
    let message = inbound(vec!["external-1"], "这是一条明显超过测试上限的消息正文");

    let result = fixture.service.handle(message.clone()).await.unwrap();

    assert!(matches!(
        result,
        ReplyOutcome::Rejected(ReplyRejection::TextTooLarge)
    ));
    assert!(fixture.store.claim(&message.claim_key().unwrap()).is_none());
}
