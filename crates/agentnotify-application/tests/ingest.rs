use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use agentnotify_agent_sdk::{
    AgentAdapter, AgentCapabilities, AgentDescriptor, AgentError, AgentEventEnvelope, AgentHealth,
    AgentRegistry, NormalizedAgentEvent, ResumeReceipt,
};
use agentnotify_application::{
    AgentNotificationConfig, Clock, EventSink, IdGenerator, IngestResult, IngestService,
    IngestStore, NotificationPolicy, OutboxItem, QuietHours, StoreError,
};
use agentnotify_domain::{
    AgentId, AgentSessionId, ClaimKey, DeliveryId, Notification, NotificationId,
    NotificationMetadata, RequestId, Timestamp,
};

#[derive(Clone)]
struct TestStore {
    notifications: Arc<Mutex<HashMap<String, Notification>>>,
    outbox_count: Arc<AtomicU64>,
}

impl TestStore {
    fn new() -> Self {
        Self {
            notifications: Arc::new(Mutex::new(HashMap::new())),
            outbox_count: Arc::new(AtomicU64::new(0)),
        }
    }

    fn outbox_count(&self) -> u64 {
        self.outbox_count.load(Ordering::SeqCst)
    }
}

#[async_trait::async_trait]
impl IngestStore for TestStore {
    async fn commit_ingest(
        &self,
        notification: Notification,
        outbox: Vec<OutboxItem>,
    ) -> Result<(), StoreError> {
        let mut notifications = self.notifications.lock().unwrap();
        let key = format!("{}:{}", notification.agent_id, notification.ingest_key);
        if notifications.contains_key(&key) {
            return Err(StoreError::conflict("store_conflict", "通知已存在"));
        }
        notifications.insert(key, notification);
        self.outbox_count
            .fetch_add(outbox.len() as u64, Ordering::SeqCst);
        Ok(())
    }

    async fn notification_by_ingest_key(
        &self,
        agent_id: &AgentId,
        ingest_key: &str,
    ) -> Result<Option<Notification>, StoreError> {
        let key = format!("{agent_id}:{ingest_key}");
        Ok(self.notifications.lock().unwrap().get(&key).cloned())
    }

    async fn recent_notification_at(
        &self,
        agent_id: &AgentId,
        session_id: &AgentSessionId,
    ) -> Result<Option<Timestamp>, StoreError> {
        Ok(self
            .notifications
            .lock()
            .unwrap()
            .values()
            .filter(|notification| {
                &notification.agent_id == agent_id
                    && notification.session_id.as_ref() == Some(session_id)
            })
            .map(|notification| notification.occurred_at)
            .max())
    }
}

#[derive(Default)]
struct TestEventSink {
    events: Mutex<Vec<String>>,
}

#[async_trait::async_trait]
impl EventSink for TestEventSink {
    async fn notification_changed(&self, notification_id: &NotificationId) {
        self.events
            .lock()
            .unwrap()
            .push(format!("notification:{notification_id}"));
    }

    async fn delivery_changed(&self, delivery_id: &DeliveryId) {
        self.events
            .lock()
            .unwrap()
            .push(format!("delivery:{delivery_id}"));
    }

    async fn reply_changed(&self, claim_key: &ClaimKey) {
        self.events
            .lock()
            .unwrap()
            .push(format!("reply:{claim_key}"));
    }

    async fn runtime_stopped(&self) {
        self.events.lock().unwrap().push("runtime.stopped".into());
    }
}

struct FixedClock(Timestamp);

impl Clock for FixedClock {
    fn now(&self) -> Timestamp {
        self.0
    }
}

struct TestIds(AtomicU64);

impl IdGenerator for TestIds {
    fn next_id(&self) -> String {
        format!("notification-{}", self.0.fetch_add(1, Ordering::SeqCst))
    }
}

struct TestAgent {
    id: AgentId,
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
            resume: true,
            ..Default::default()
        }
    }

    fn parse_event(
        &self,
        envelope: AgentEventEnvelope,
    ) -> Result<NormalizedAgentEvent, AgentError> {
        if envelope.agent_id != self.id {
            return Err(AgentError::InvalidEvent);
        }
        let payload = &envelope.payload;
        let event_type = payload
            .get("eventType")
            .and_then(serde_json::Value::as_str)
            .ok_or(AgentError::InvalidEvent)?;
        if event_type != "session.completed" {
            return Err(AgentError::InvalidEvent);
        }
        let session_id = payload
            .get("sessionId")
            .and_then(serde_json::Value::as_str)
            .map(AgentSessionId::new)
            .transpose()
            .map_err(|_| AgentError::InvalidEvent)?;
        Ok(NormalizedAgentEvent {
            idempotency_key: payload
                .get("idempotencyKey")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned),
            occurred_at: payload
                .get("occurredAt")
                .and_then(serde_json::Value::as_str)
                .map(Timestamp::parse_rfc3339)
                .transpose()
                .map_err(|_| AgentError::InvalidEvent)?
                .ok_or(AgentError::InvalidEvent)?,
            session_id,
            session_title: None,
            title: payload
                .get("title")
                .and_then(serde_json::Value::as_str)
                .ok_or(AgentError::InvalidEvent)?
                .into(),
            body: payload
                .get("body")
                .and_then(serde_json::Value::as_str)
                .ok_or(AgentError::InvalidEvent)?
                .into(),
            metadata: NotificationMetadata::default(),
        })
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

struct Fixture {
    _store: Arc<TestStore>,
    _sink: Arc<TestEventSink>,
    service: IngestService,
    envelope: AgentEventEnvelope,
}

fn fixture(now: &str, policy: NotificationPolicy) -> Fixture {
    let mut registry = AgentRegistry::default();
    registry
        .register(Arc::new(TestAgent {
            id: AgentId::new("opencode").unwrap(),
        }))
        .unwrap();
    let store = Arc::new(TestStore::new());
    let sink = Arc::new(TestEventSink::default());
    let clock = Arc::new(FixedClock(Timestamp::parse_rfc3339(now).unwrap()));
    let ids = Arc::new(TestIds(AtomicU64::new(1)));
    let service = IngestService::new(
        Arc::new(registry),
        store.clone(),
        sink.clone(),
        clock,
        ids,
        policy,
    );
    Fixture {
        _store: store,
        _sink: sink,
        service,
        envelope: envelope("event-1", "session-1", "任务完成", now),
    }
}

fn envelope(
    idempotency_key: &str,
    session_id: &str,
    title: &str,
    occurred_at: &str,
) -> AgentEventEnvelope {
    AgentEventEnvelope {
        request_id: RequestId::new(format!("request-{idempotency_key}")).unwrap(),
        agent_id: AgentId::new("opencode").unwrap(),
        payload: serde_json::json!({
            "eventType": "session.completed",
            "idempotencyKey": idempotency_key,
            "occurredAt": occurred_at,
            "sessionId": session_id,
            "title": title,
            "body": "Release 已生成",
        }),
    }
}

fn enabled_agent_config() -> AgentNotificationConfig {
    AgentNotificationConfig {
        enabled: true,
        quiet_hours: None,
        cooldown: None,
    }
}

#[tokio::test]
async fn duplicate_event_returns_original_notification_and_single_outbox() {
    let fixture = fixture(
        "2026-09-19T09:00:00Z",
        NotificationPolicy::default()
            .with_agent(AgentId::new("opencode").unwrap(), enabled_agent_config()),
    );
    let first = fixture
        .service
        .ingest(fixture.envelope.clone())
        .await
        .unwrap();
    let second = fixture.service.ingest(fixture.envelope).await.unwrap();

    assert_eq!(first.notification_id(), second.notification_id());
    assert!(matches!(second, IngestResult::Duplicate { .. }));
    assert_eq!(fixture._store.outbox_count(), 1);
}

#[tokio::test]
async fn quiet_hours_skip_produces_notification_without_outbox() {
    let mut config = enabled_agent_config();
    config.quiet_hours = Some(QuietHours {
        start_minute: 23 * 60,
        end_minute: 7 * 60,
        utc_offset_minutes: 8 * 60,
    });
    let fixture = fixture(
        "2026-09-19T23:30:00+08:00",
        NotificationPolicy::default().with_agent(AgentId::new("opencode").unwrap(), config),
    );
    let result = fixture.service.ingest(fixture.envelope).await.unwrap();

    assert_eq!(
        result,
        IngestResult::Skipped {
            reason: agentnotify_application::SkipReason::QuietHours
        }
    );
    assert_eq!(fixture._store.outbox_count(), 0);
}

#[tokio::test]
async fn cooldown_uses_recent_session_notification() {
    let mut config = enabled_agent_config();
    config.cooldown = Some(time::Duration::minutes(30));
    let policy =
        NotificationPolicy::default().with_agent(AgentId::new("opencode").unwrap(), config);
    let mut fixture = fixture("2026-09-19T09:00:00Z", policy);

    let first = fixture
        .service
        .ingest(fixture.envelope.clone())
        .await
        .unwrap();
    assert!(matches!(first, IngestResult::Queued { .. }));

    fixture.envelope = envelope("event-2", "session-1", "任务完成", "2026-09-19T09:01:00Z");
    let second = fixture.service.ingest(fixture.envelope).await.unwrap();
    assert_eq!(
        second,
        IngestResult::Skipped {
            reason: agentnotify_application::SkipReason::Cooldown
        }
    );
}

#[tokio::test]
async fn missing_agent_configuration_skips_without_queuing() {
    let fixture = fixture("2026-09-19T09:00:00Z", NotificationPolicy::default());
    let result = fixture.service.ingest(fixture.envelope).await.unwrap();

    assert_eq!(
        result,
        IngestResult::Skipped {
            reason: agentnotify_application::SkipReason::AgentNotConfigured
        }
    );
    assert_eq!(fixture._store.outbox_count(), 0);
}
