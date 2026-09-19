#![allow(dead_code)]

use std::{path::Path, sync::Arc};

use agentnotify_agent_sdk::{AgentEventEnvelope, AgentRegistry};
use agentnotify_application::{
    AgentNotificationConfig, ChannelAccountStore, DeliveryService, DeliveryTarget, EventSink,
    IngestService, NotificationPolicy, ReplyConfig, ReplyService, ReplyTarget, RetryPolicy,
};
use agentnotify_channel_sdk::{ChannelAccount, ChannelRegistry};
use agentnotify_domain::{
    AgentId, AgentSessionId, ChannelAccountId, ChannelId, ClaimKey, DeliveryId, ExternalMessageId,
    InboundMessage, InboundMessageId, InboundMessageInput, NotificationId, RequestId, Timestamp,
};
use agentnotify_runtime::RuntimeConfig;
use agentnotify_storage_sqlite::SqliteStore;
use agentnotify_testkit::{FakeAgent, FakeChannel, FakeClock, SequenceIdGenerator};

pub const AGENT_ID: &str = "opencode";
pub const CHANNEL_ID: &str = "fake";
pub const ACCOUNT_ID: &str = "account-1";
pub const CONVERSATION_ID: &str = "conversation-1";
pub const SENDER_ID: &str = "user-1";

#[derive(Clone)]
pub struct SharedAdapters {
    pub agent: Arc<FakeAgent>,
    pub channel: Arc<FakeChannel>,
    pub clock: FakeClock,
    pub ids: SequenceIdGenerator,
    pub account: ChannelAccount,
}

impl SharedAdapters {
    pub fn new() -> Self {
        Self::with_channel(Arc::new(FakeChannel::new()))
    }

    pub fn unknown_channel() -> Self {
        Self::with_channel(Arc::new(FakeChannel::unknown_on_send()))
    }

    fn with_channel(channel: Arc<FakeChannel>) -> Self {
        let now = timestamp("2026-09-19T09:00:00Z");
        let account = ChannelAccount::new(
            ChannelAccountId::new(ACCOUNT_ID).unwrap(),
            ChannelId::new(CHANNEL_ID).unwrap(),
            "测试账号",
            now,
        );
        Self {
            agent: Arc::new(FakeAgent::new(AGENT_ID)),
            channel,
            clock: FakeClock::new(now),
            ids: SequenceIdGenerator::default(),
            account,
        }
    }
}

impl Default for SharedAdapters {
    fn default() -> Self {
        Self::new()
    }
}

struct NoopEventSink;

#[async_trait::async_trait]
impl EventSink for NoopEventSink {
    async fn notification_changed(&self, _notification_id: &NotificationId) {}

    async fn delivery_changed(&self, _delivery_id: &DeliveryId) {}

    async fn reply_changed(&self, _claim_key: &ClaimKey) {}

    async fn runtime_stopped(&self) {}
}

pub struct CoreServices {
    pub ingest: IngestService,
    pub delivery: DeliveryService,
    pub reply: ReplyService,
}

impl CoreServices {
    pub fn new(store: Arc<SqliteStore>, shared: &SharedAdapters) -> Self {
        let mut agents = AgentRegistry::default();
        agents.register(shared.agent.clone()).unwrap();
        let agents = Arc::new(agents);

        let mut channels = ChannelRegistry::default();
        channels.register(shared.channel.clone()).unwrap();
        let channels = Arc::new(channels);

        let policy = NotificationPolicy::default().with_agent(
            AgentId::new(AGENT_ID).unwrap(),
            AgentNotificationConfig::default(),
        );
        let events = Arc::new(NoopEventSink);
        let ingest = IngestService::new(
            agents.clone(),
            store.clone(),
            events.clone(),
            Arc::new(shared.clock.clone()),
            Arc::new(shared.ids.clone()),
            policy,
        );
        let delivery = DeliveryService::new(
            store.clone(),
            channels.clone(),
            vec![DeliveryTarget::new(shared.account.clone(), CONVERSATION_ID)],
            Arc::new(shared.clock.clone()),
            Arc::new(shared.ids.clone()),
            events.clone(),
            RetryPolicy::default(),
        );
        let reply = ReplyService::new(
            store.clone(),
            store.clone(),
            channels,
            agents,
            Arc::new(shared.clock.clone()),
            events,
            vec![ReplyTarget::new(shared.account.clone(), SENDER_ID, CONVERSATION_ID).unwrap()],
            ReplyConfig {
                enabled: true,
                send_confirmation: false,
                ..ReplyConfig::default()
            },
            None,
        )
        .unwrap();

        Self {
            ingest,
            delivery,
            reply,
        }
    }
}

pub async fn initialize_store(database_path: &Path, shared: &SharedAdapters) -> Arc<SqliteStore> {
    let store = Arc::new(SqliteStore::open(database_path).unwrap());
    store.upsert(shared.account.clone()).await.unwrap();
    store
}

pub fn runtime_config(database_path: std::path::PathBuf, shared: &SharedAdapters) -> RuntimeConfig {
    let mut agents = AgentRegistry::default();
    agents.register(shared.agent.clone()).unwrap();
    let mut channels = ChannelRegistry::default();
    channels.register(shared.channel.clone()).unwrap();

    RuntimeConfig {
        database_path,
        migration: None,
        agents: Arc::new(agents),
        channels: Arc::new(channels),
        clock: Arc::new(shared.clock.clone()),
        id_generator: Arc::new(shared.ids.clone()),
        notification_policy: NotificationPolicy::default().with_agent(
            AgentId::new(AGENT_ID).unwrap(),
            AgentNotificationConfig::default(),
        ),
        delivery_targets: vec![DeliveryTarget::new(shared.account.clone(), CONVERSATION_ID)],
        reply_targets: vec![
            ReplyTarget::new(shared.account.clone(), SENDER_ID, CONVERSATION_ID).unwrap(),
        ],
        reply_config: ReplyConfig {
            enabled: true,
            send_confirmation: false,
            ..ReplyConfig::default()
        },
        app_version: "0.1.0-test".into(),
        platform: "windows".into(),
        ingress_spool_dir: None,
        ingress_pipe_enabled: false,
        telemetry: None,
        inbound_capacity: 16,
        worker_idle_delay: std::time::Duration::from_millis(5),
        status_refresh_interval: std::time::Duration::from_millis(5),
        channel_poll_interval: std::time::Duration::from_millis(5),
    }
}

pub fn completed_event(
    request_id: &str,
    idempotency_key: &str,
    session_id: &str,
) -> AgentEventEnvelope {
    AgentEventEnvelope {
        request_id: RequestId::new(request_id).unwrap(),
        agent_id: AgentId::new(AGENT_ID).unwrap(),
        payload: serde_json::json!({
            "eventType": "session.completed",
            "idempotencyKey": idempotency_key,
            "occurredAt": "2026-09-19T09:00:01Z",
            "sessionId": session_id,
            "title": "任务完成",
            "body": "测试 Agent 已完成当前任务"
        }),
    }
}

pub fn quoted_message(reply_id: &str, external_message_id: &str, text: &str) -> InboundMessage {
    InboundMessage::new(
        InboundMessageId::new(reply_id).unwrap(),
        ChannelId::new(CHANNEL_ID).unwrap(),
        ChannelAccountId::new(ACCOUNT_ID).unwrap(),
        InboundMessageInput {
            external_message_id: ExternalMessageId::new(reply_id).unwrap(),
            sender_id: SENDER_ID.into(),
            conversation_id: CONVERSATION_ID.into(),
            referenced_message_ids: vec![ExternalMessageId::new(external_message_id).unwrap()],
            text: text.into(),
            received_at: timestamp("2026-09-19T08:59:59Z"),
        },
    )
    .unwrap()
}

pub fn external_id_for_fixture() -> ExternalMessageId {
    ExternalMessageId::new(format!("message-{ACCOUNT_ID}")).unwrap()
}

pub fn timestamp(value: &str) -> Timestamp {
    Timestamp::parse_rfc3339(value).unwrap()
}

pub fn agent_session(value: &str) -> AgentSessionId {
    AgentSessionId::new(value).unwrap()
}
