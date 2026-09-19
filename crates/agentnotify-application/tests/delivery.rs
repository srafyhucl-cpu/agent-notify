use std::sync::{Arc, Mutex};

use agentnotify_application::{
    Clock, DeliveryService, DeliveryStore, DeliveryTarget, EventSink, IdGenerator, OutboxLease,
    OutboxState, ProcessOutcome, RetryPolicy, StoreError,
};
use agentnotify_channel_sdk::{
    ChannelAccount, ChannelAdapter, ChannelCapabilities, ChannelDescriptor, ChannelError,
    ChannelRegistry, ChannelTask, DeliveryReceipt, InboundEmitter, InboundMode, OutboundMessage,
};
use agentnotify_domain::{
    ChannelAccountId, ChannelId, ClaimKey, Delivery, DeliveryErrorKind, DeliveryId, DeliveryState,
    ExternalMessageId, Notification, NotificationId, ReplyRoute, SafeError, Timestamp,
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
        _message: OutboundMessage,
    ) -> Result<DeliveryReceipt, ChannelError> {
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
}

fn fixture(mode: ChannelMode) -> Fixture {
    let now = timestamp("2026-09-19T09:00:00Z");
    let notification = Notification::new(
        NotificationId::new("notification-1").unwrap(),
        "event-1",
        agentnotify_domain::AgentId::new("opencode").unwrap(),
        Some(agentnotify_domain::AgentSessionId::new("session-1").unwrap()),
        None,
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
    let mut registry = ChannelRegistry::default();
    registry
        .register(Arc::new(TestChannel {
            id: ChannelId::new("test-channel").unwrap(),
            mode,
        }))
        .unwrap();
    let account = ChannelAccount::new(
        ChannelAccountId::new("account-1").unwrap(),
        ChannelId::new("test-channel").unwrap(),
        "测试账号",
        now,
    );
    let service = DeliveryService::new(
        store.clone(),
        Arc::new(registry),
        vec![DeliveryTarget::new(account, "conversation-1")],
        Arc::new(FixedClock(now)),
        Arc::new(TestIds),
        Arc::new(TestSink),
        RetryPolicy::default(),
    );
    Fixture { store, service }
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
