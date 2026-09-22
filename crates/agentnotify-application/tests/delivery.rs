use std::sync::{Arc, Mutex};

use agentnotify_agent_sdk::{
    AgentAdapter, AgentCapabilities, AgentDescriptor, AgentError, AgentEventEnvelope, AgentHealth,
    AgentRegistry, NormalizedAgentEvent, ResumeReceipt,
};
use agentnotify_application::{
    Clock, DeliveryService, DeliveryStore, DeliveryTarget, EventSink, IdGenerator, OutboxLease,
    OutboxState, ProcessOutcome, RetryPolicy, StoreError,
};
use agentnotify_channel_sdk::{
    ChannelAccount, ChannelAdapter, ChannelCapabilities, ChannelDescriptor, ChannelError,
    ChannelRegistry, ChannelTask, DeliveryReceipt, InboundEmitter, InboundMode,
    NotificationPresentation, OutboundMessage,
};
use agentnotify_domain::{
    AgentId, AgentSessionId, ChannelAccountId, ChannelId, ClaimKey, Delivery, DeliveryErrorKind,
    DeliveryId, DeliveryState, ExternalMessageId, Notification, NotificationId, ReplyRoute,
    SafeError, Timestamp,
};

#[derive(Clone)]
struct TestStore {
    lease: Arc<Mutex<Option<OutboxLease>>>,
    delivery: Arc<Mutex<Option<Delivery>>>,
    route: Arc<Mutex<Option<ReplyRoute>>>,
    outbox_state: Arc<Mutex<Option<OutboxState>>>,
    next_attempt_at: Arc<Mutex<Option<Timestamp>>>,
}

impl TestStore {
    fn new(lease: OutboxLease) -> Self {
        Self {
            lease: Arc::new(Mutex::new(Some(lease))),
            delivery: Arc::new(Mutex::new(None)),
            route: Arc::new(Mutex::new(None)),
            outbox_state: Arc::new(Mutex::new(None)),
            next_attempt_at: Arc::new(Mutex::new(None)),
        }
    }
}

#[async_trait::async_trait]
impl DeliveryStore for TestStore {
    async fn lease_next_outbox(
        &self,
        _now: Timestamp,
        _lease_until: Timestamp,
    ) -> Result<Option<OutboxLease>, StoreError> {
        Ok(self.lease.lock().unwrap().take())
    }

    async fn commit_delivery(
        &self,
        _lease: OutboxLease,
        delivery: Delivery,
        route: Option<ReplyRoute>,
    ) -> Result<(), StoreError> {
        *self.outbox_state.lock().unwrap() = Some(match delivery.state() {
            DeliveryState::Sent | DeliveryState::Skipped => OutboxState::Done,
            DeliveryState::Failed => OutboxState::Dead,
            DeliveryState::Unknown => OutboxState::Unknown,
            DeliveryState::Pending => OutboxState::Leased,
        });
        *self.route.lock().unwrap() = route;
        *self.delivery.lock().unwrap() = Some(delivery);
        Ok(())
    }

    async fn reschedule_outbox(
        &self,
        _lease: OutboxLease,
        delivery: Delivery,
        next_attempt_at: Timestamp,
    ) -> Result<(), StoreError> {
        *self.outbox_state.lock().unwrap() = Some(OutboxState::Pending);
        *self.next_attempt_at.lock().unwrap() = Some(next_attempt_at);
        *self.delivery.lock().unwrap() = Some(delivery);
        Ok(())
    }
}

#[derive(Default)]
struct TestSink;

#[async_trait::async_trait]
impl EventSink for TestSink {
    async fn notification_changed(&self, _notification_id: &NotificationId) {}
    async fn delivery_changed(&self, _delivery_id: &DeliveryId) {}
    async fn reply_changed(&self, _claim_key: &ClaimKey) {}
    async fn runtime_stopped(&self) {}
}

struct FixedClock(Timestamp);

impl Clock for FixedClock {
    fn now(&self) -> Timestamp {
        self.0
    }
}

struct TestIds;

impl IdGenerator for TestIds {
    fn next_id(&self) -> String {
        "delivery-1".into()
    }
}

#[derive(Clone)]
enum ChannelMode {
    Sent,
    Unknown,
    Retryable,
    Permanent,
    MissingMessageId,
}

struct TestChannel {
    id: ChannelId,
    mode: ChannelMode,
    messages: Arc<Mutex<Vec<OutboundMessage>>>,
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
            markdown: true,
            max_text_bytes: None,
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
                ExternalMessageId::new("external-1").unwrap(),
            )),
            ChannelMode::Unknown => Ok(DeliveryReceipt::unknown(
                SafeError::new("channel_timeout", "渠道结果无法确认").unwrap(),
            )),
            ChannelMode::Retryable => Err(ChannelError::retryable(
                "channel_retryable",
                "渠道暂时不可用",
                Some(time::Duration::seconds(20)),
            )),
            ChannelMode::Permanent => {
                Err(ChannelError::permanent("channel_rejected", "渠道拒绝消息"))
            }
            ChannelMode::MissingMessageId => Ok(DeliveryReceipt {
                external_message_id: None,
                external_thread_id: None,
                state: DeliveryState::Sent,
                error: None,
                raw_safe_metadata: Default::default(),
            }),
        }
    }

    async fn inspect(&self, _account: ChannelAccount) -> agentnotify_channel_sdk::ChannelHealth {
        agentnotify_channel_sdk::ChannelHealth::healthy()
    }

    async fn logout(&self, _account: ChannelAccount) -> Result<(), ChannelError> {
        Ok(())
    }
}

struct Fixture {
    store: Arc<TestStore>,
    service: DeliveryService,
    messages: Arc<Mutex<Vec<OutboundMessage>>>,
}

fn fixture(mode: ChannelMode) -> Fixture {
    fixture_with(mode, None, Some("session-1"), None)
}

/// 带 Agent 注册表、会话号与会话标题的装配，用于验证结构化通知信息。
fn fixture_with(
    mode: ChannelMode,
    agents: Option<Arc<AgentRegistry>>,
    session_id: Option<&str>,
    session_title: Option<&str>,
) -> Fixture {
    let now = timestamp("2026-09-19T09:00:00Z");
    let notification = Notification::new(
        NotificationId::new("notification-1").unwrap(),
        "event-1",
        AgentId::new("opencode").unwrap(),
        session_id.map(|value| AgentSessionId::new(value).unwrap()),
        session_title.map(str::to_owned),
        "任务完成",
        "Agent 已完成当前任务",
        now,
        Default::default(),
    )
    .unwrap();
    let lease = OutboxLease {
        outbox: agentnotify_application::OutboxItem {
            id: "outbox-1".into(),
            notification_id: notification.id.clone(),
            state: OutboxState::Leased,
            available_at: now,
            attempt_count: 1,
            last_error: None,
        },
        notification,
        owner: "test-owner".into(),
        lease_until: now.checked_add(time::Duration::seconds(30)).unwrap(),
    };
    let store = Arc::new(TestStore::new(lease));
    let messages = Arc::new(Mutex::new(Vec::new()));
    let mut registry = ChannelRegistry::default();
    registry
        .register(Arc::new(TestChannel {
            id: ChannelId::new("test-channel").unwrap(),
            mode,
            messages: messages.clone(),
        }))
        .unwrap();
    let account = ChannelAccount::new(
        ChannelAccountId::new("account-1").unwrap(),
        ChannelId::new("test-channel").unwrap(),
        "测试账号",
        now,
    );
    let mut service = DeliveryService::new(
        store.clone(),
        Arc::new(registry),
        vec![DeliveryTarget::new(account, "conversation-1")],
        Arc::new(FixedClock(now)),
        Arc::new(TestIds),
        Arc::new(TestSink),
        RetryPolicy::default(),
    );
    if let Some(agents) = agents {
        service = service.with_agent_registry(agents);
    }
    Fixture {
        store,
        service,
        messages,
    }
}

/// 只暴露通知展示所需字段的测试 Agent。
struct TestAgent {
    id: AgentId,
    display_name: &'static str,
    resume: bool,
}

impl TestAgent {
    fn new(id: &str, display_name: &'static str, resume: bool) -> Self {
        Self {
            id: AgentId::new(id).unwrap(),
            display_name,
            resume,
        }
    }
}

#[async_trait::async_trait]
impl AgentAdapter for TestAgent {
    fn descriptor(&self) -> AgentDescriptor {
        AgentDescriptor {
            id: self.id.clone(),
            display_name: self.display_name.into(),
            description: "测试 Agent".into(),
            config_schema: serde_json::json!({"type": "object"}),
        }
    }

    fn capabilities(&self) -> AgentCapabilities {
        AgentCapabilities {
            notify: true,
            resume: self.resume,
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
        _text: &str,
    ) -> Result<ResumeReceipt, AgentError> {
        Ok(ResumeReceipt {
            session_id: session_id.clone(),
        })
    }

    async fn inspect(&self) -> AgentHealth {
        AgentHealth::healthy()
    }
}

fn agent_registry(agent: TestAgent) -> Arc<AgentRegistry> {
    let mut registry = AgentRegistry::default();
    registry.register(Arc::new(agent)).unwrap();
    Arc::new(registry)
}

fn delivered_message(fixture: &Fixture) -> OutboundMessage {
    fixture
        .messages
        .lock()
        .unwrap()
        .first()
        .cloned()
        .expect("渠道必须收到一条出站消息")
}

fn timestamp(value: &str) -> Timestamp {
    Timestamp::parse_rfc3339(value).unwrap()
}

#[test]
fn unknown_never_gets_next_attempt() {
    let policy = RetryPolicy::default();
    assert_eq!(
        policy.next_attempt(
            1,
            DeliveryErrorKind::Unknown,
            timestamp("2026-09-19T09:00:00Z")
        ),
        None
    );
}

#[test]
fn retryable_uses_bounded_exponential_backoff() {
    let policy = RetryPolicy::default();
    let now = timestamp("2026-09-19T09:00:00Z");
    let second = policy
        .next_attempt(1, DeliveryErrorKind::Retryable, now)
        .unwrap();
    let third = policy
        .next_attempt(2, DeliveryErrorKind::Retryable, now)
        .unwrap();
    assert!(third > second);
    assert!(third <= now.checked_add(time::Duration::minutes(5)).unwrap());
}

#[tokio::test]
async fn sent_receipt_persists_delivery_and_route() {
    let fixture = fixture(ChannelMode::Sent);
    let outcome = fixture.service.process_next().await.unwrap();

    assert!(matches!(outcome, ProcessOutcome::Completed { .. }));
    assert_eq!(
        fixture
            .store
            .delivery
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .state(),
        DeliveryState::Sent
    );
    assert_eq!(
        fixture
            .store
            .route
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .key
            .external_message_id
            .as_str(),
        "external-1"
    );
}

#[tokio::test]
async fn unknown_receipt_never_creates_route_or_retries() {
    let fixture = fixture(ChannelMode::Unknown);
    let outcome = fixture.service.process_next().await.unwrap();

    assert!(matches!(outcome, ProcessOutcome::Completed { .. }));
    assert_eq!(
        fixture
            .store
            .delivery
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .state(),
        DeliveryState::Unknown
    );
    assert!(fixture.store.route.lock().unwrap().is_none());
    assert_eq!(
        *fixture.store.outbox_state.lock().unwrap(),
        Some(OutboxState::Unknown)
    );
}

#[tokio::test]
async fn retryable_error_reschedules_with_backoff() {
    let fixture = fixture(ChannelMode::Retryable);
    let outcome = fixture.service.process_next().await.unwrap();

    assert!(matches!(outcome, ProcessOutcome::Rescheduled { .. }));
    assert!(fixture.store.next_attempt_at.lock().unwrap().is_some());
    assert!(
        fixture
            .store
            .delivery
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .can_retry()
    );
}

#[tokio::test]
async fn missing_receipt_id_becomes_unknown_without_route() {
    let fixture = fixture(ChannelMode::MissingMessageId);
    let outcome = fixture.service.process_next().await.unwrap();

    assert!(matches!(outcome, ProcessOutcome::Completed { .. }));
    assert_eq!(
        fixture
            .store
            .delivery
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .state(),
        DeliveryState::Unknown
    );
    assert!(fixture.store.route.lock().unwrap().is_none());
}

#[tokio::test]
async fn permanent_error_marks_outbox_dead() {
    let fixture = fixture(ChannelMode::Permanent);
    let outcome = fixture.service.process_next().await.unwrap();

    assert!(matches!(outcome, ProcessOutcome::Completed { .. }));
    assert_eq!(
        *fixture.store.outbox_state.lock().unwrap(),
        Some(OutboxState::Dead)
    );
}

/// 投递层必须把注册表里的显示名、会话名与续聊能力交给渠道，同时保留原始文本。
#[tokio::test]
async fn notification_presentation_uses_registry_display_name_and_reply_capability() {
    let fixture = fixture_with(
        ChannelMode::Sent,
        Some(agent_registry(TestAgent::new("opencode", "Codex", true))),
        Some("session-1"),
        Some("修复登录"),
    );
    let outcome = fixture.service.process_next().await.unwrap();
    assert!(matches!(outcome, ProcessOutcome::Completed { .. }));

    let message = delivered_message(&fixture);
    assert_eq!(message.text, "任务完成\n\nAgent 已完成当前任务");
    assert_eq!(
        message.notification,
        Some(NotificationPresentation {
            agent_display_name: "Codex".into(),
            session_name: "修复登录".into(),
            occurred_at: timestamp("2026-09-19T09:00:00Z"),
            include_footer: true,
            replyable: true,
        })
    );
}

/// 没有会话标题时回退通知标题；没有会话号时不得声称可引用。
#[tokio::test]
async fn notification_presentation_falls_back_to_title_and_never_fabricates_reply_support() {
    let fixture = fixture_with(
        ChannelMode::Sent,
        Some(agent_registry(TestAgent::new("opencode", "Codex", true))),
        None,
        None,
    );
    fixture.service.process_next().await.unwrap();

    let message = delivered_message(&fixture);
    let presentation = message.notification.expect("必须有结构化通知信息");
    assert_eq!(presentation.session_name, "任务完成");
    assert!(!presentation.replyable, "没有会话号时不得声称可引用续聊");
}

/// Agent 未注册或未注入注册表时按原始文本投递，保持向后兼容。
#[tokio::test]
async fn notification_without_registered_agent_keeps_raw_text() {
    for agents in [
        None,
        Some(agent_registry(TestAgent::new("other-agent", "Other", true))),
    ] {
        let fixture = fixture_with(ChannelMode::Sent, agents, Some("session-1"), None);
        fixture.service.process_next().await.unwrap();

        let message = delivered_message(&fixture);
        assert!(message.notification.is_none());
        assert_eq!(message.text, "任务完成\n\nAgent 已完成当前任务");
    }
}

/// Agent 声明不支持续聊时，即使通知有会话号也不能声称可引用。
#[tokio::test]
async fn notification_presentation_marks_replyable_false_without_resume_capability() {
    let fixture = fixture_with(
        ChannelMode::Sent,
        Some(agent_registry(TestAgent::new("opencode", "Codex", false))),
        Some("session-1"),
        Some("修复登录"),
    );
    fixture.service.process_next().await.unwrap();

    let message = delivered_message(&fixture);
    let presentation = message.notification.expect("必须有结构化通知信息");
    assert!(!presentation.replyable);
}

#[tokio::test]
async fn target_account_id_in_metadata_selects_matching_account() {
    let now = timestamp("2026-09-19T09:00:00Z");
    let metadata =
        agentnotify_domain::NotificationMetadata::new([("targetAccountId", "account-2")]).unwrap();

    let notification = Notification::new(
        NotificationId::new("notif-target-1").unwrap(),
        "event-target-1",
        agentnotify_domain::AgentId::new("opencode").unwrap(),
        None,
        None,
        "Target test",
        "Target body",
        now,
        metadata,
    )
    .unwrap();

    let lease = OutboxLease {
        outbox: agentnotify_application::OutboxItem {
            id: "outbox-target-1".into(),
            notification_id: notification.id.clone(),
            state: OutboxState::Leased,
            available_at: now,
            attempt_count: 1,
            last_error: None,
        },
        notification,
        owner: "test-owner".into(),
        lease_until: now.checked_add(time::Duration::seconds(30)).unwrap(),
    };
    let store = Arc::new(TestStore::new(lease));
    let mut registry = ChannelRegistry::default();
    registry
        .register(Arc::new(TestChannel {
            id: ChannelId::new("test-channel").unwrap(),
            mode: ChannelMode::Sent,
            messages: Arc::new(Mutex::new(Vec::new())),
        }))
        .unwrap();

    let account1 = ChannelAccount::new(
        ChannelAccountId::new("account-1").unwrap(),
        ChannelId::new("test-channel").unwrap(),
        "账号1",
        now,
    );
    let account2 = ChannelAccount::new(
        ChannelAccountId::new("account-2").unwrap(),
        ChannelId::new("test-channel").unwrap(),
        "账号2",
        now,
    );

    let service = DeliveryService::new(
        store.clone(),
        Arc::new(registry),
        vec![
            DeliveryTarget::new(account1, "conv-1"),
            DeliveryTarget::new(account2, "conv-2"),
        ],
        Arc::new(FixedClock(now)),
        Arc::new(TestIds),
        Arc::new(TestSink),
        RetryPolicy::default(),
    );

    let outcome = service.process_next().await.unwrap();
    assert!(matches!(outcome, ProcessOutcome::Completed { .. }));
    let delivery = store.delivery.lock().unwrap().clone().unwrap();
    assert_eq!(delivery.account_id().as_str(), "account-2");
}

#[tokio::test]
async fn target_account_id_not_found_fails_with_no_target() {
    let now = timestamp("2026-09-19T09:00:00Z");
    let metadata = agentnotify_domain::NotificationMetadata::new([(
        "targetAccountId",
        "non-existent-account",
    )])
    .unwrap();

    let notification = Notification::new(
        NotificationId::new("notif-target-2").unwrap(),
        "event-target-2",
        agentnotify_domain::AgentId::new("opencode").unwrap(),
        None,
        None,
        "Target test",
        "Target body",
        now,
        metadata,
    )
    .unwrap();

    let lease = OutboxLease {
        outbox: agentnotify_application::OutboxItem {
            id: "outbox-target-2".into(),
            notification_id: notification.id.clone(),
            state: OutboxState::Leased,
            available_at: now,
            attempt_count: 1,
            last_error: None,
        },
        notification,
        owner: "test-owner".into(),
        lease_until: now.checked_add(time::Duration::seconds(30)).unwrap(),
    };
    let store = Arc::new(TestStore::new(lease));
    let mut registry = ChannelRegistry::default();
    registry
        .register(Arc::new(TestChannel {
            id: ChannelId::new("test-channel").unwrap(),
            mode: ChannelMode::Sent,
            messages: Arc::new(Mutex::new(Vec::new())),
        }))
        .unwrap();

    let account1 = ChannelAccount::new(
        ChannelAccountId::new("account-1").unwrap(),
        ChannelId::new("test-channel").unwrap(),
        "账号1",
        now,
    );

    let service = DeliveryService::new(
        store.clone(),
        Arc::new(registry),
        vec![DeliveryTarget::new(account1, "conv-1")],
        Arc::new(FixedClock(now)),
        Arc::new(TestIds),
        Arc::new(TestSink),
        RetryPolicy::default(),
    );

    let result = service.process_next().await;
    assert!(matches!(
        result,
        Err(agentnotify_application::DeliveryError::NoTarget)
    ));
}
