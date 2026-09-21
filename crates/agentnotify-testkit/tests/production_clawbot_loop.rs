//! 生产 ClawBot 适配器的离线全链路测试。
//!
//! 用本机 HTTP 假平台替代真实 ClawBot 服务，验证真实适配器与投递/回复服务拼在一起时：
//! 出站请求体、平台稳定消息 ID、ReplyRoute 关联、引用回复命中与未命中路由的行为都正确。
//! 真实账号与真实平台仍由 Task 9 的真机验收覆盖，本测试只证明代码路径不是只有契约级覆盖。

mod support;

use std::sync::{Arc, Mutex};

use agentnotify_agent_sdk::AgentRegistry;
use agentnotify_application::{
    AgentNotificationConfig, ChannelAccountStore, ClaimStore, DeliveryService, DeliveryTarget,
    EventSink, IngestResult, IngestService, NotificationPolicy, ProcessOutcome, ReplyConfig,
    ReplyOutcome, ReplyRejection, ReplyService, ReplyTarget, RetryPolicy, RouteStore,
};
use agentnotify_channel_clawbot::{
    CLAWBOT_CHANNEL_ID, ClawBotAccount, ClawBotChannel, ClawBotContext, ClawBotCredentials,
    ClawBotInboundMessage, normalize_inbound,
};
use agentnotify_channel_sdk::ChannelRegistry;
use agentnotify_domain::{
    AgentId, ChannelId, ClaimKey, ClaimState, DeliveryId, ExternalMessageId, NotificationId,
    RouteKey,
};
use agentnotify_storage_sqlite::SqliteStore;
use agentnotify_testkit::{FakeAgent, FakeClock, MemoryStore, SequenceIdGenerator};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use support::{AGENT_ID, completed_event, timestamp};

const BOT_ID: &str = "bot-test-0001";
const USER_ID: &str = "user-test-0001";
const BOT_TOKEN: &str = "bot-token-for-test";
const CONTEXT_TOKEN: &str = "context-token-for-test";
const SESSION_ID: &str = "session-1";
const PLATFORM_MESSAGE_ID: &str = "msg-1";
const SEND_ENDPOINT: &str = "POST /ilink/bot/sendmessage";

struct NoopEventSink;

#[async_trait::async_trait]
impl EventSink for NoopEventSink {
    async fn notification_changed(&self, _notification_id: &NotificationId) {}

    async fn delivery_changed(&self, _delivery_id: &DeliveryId) {}

    async fn reply_changed(&self, _claim_key: &ClaimKey) {}

    async fn runtime_stopped(&self) {}
}

/// 承接出站请求的最小 HTTP 服务，固定返回带稳定消息 ID 的成功响应。
struct FakePlatform {
    base_url: String,
    requests: Arc<Mutex<Vec<String>>>,
    handle: tokio::task::JoinHandle<()>,
}

impl FakePlatform {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("本机端口必须可监听");
        let address = listener.local_addr().unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = requests.clone();
        let handle = tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    return;
                };
                let captured = captured.clone();
                tokio::spawn(async move {
                    serve_once(stream, captured).await;
                });
            }
        });
        Self {
            base_url: format!("http://{address}"),
            requests,
            handle,
        }
    }

    fn requests(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }
}

impl Drop for FakePlatform {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

async fn serve_once(mut stream: tokio::net::TcpStream, captured: Arc<Mutex<Vec<String>>>) {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 1024];
    let header_end = loop {
        let read = match stream.read(&mut chunk).await {
            Ok(0) | Err(_) => return,
            Ok(read) => read,
        };
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(index) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            break index + 4;
        }
    };
    let headers = String::from_utf8_lossy(&buffer[..header_end]).to_string();
    let content_length = headers
        .lines()
        .find_map(|line| {
            line.to_ascii_lowercase()
                .strip_prefix("content-length:")
                .and_then(|value| value.trim().parse::<usize>().ok())
        })
        .unwrap_or(0);
    while buffer.len() < header_end + content_length {
        let read = match stream.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(read) => read,
        };
        buffer.extend_from_slice(&chunk[..read]);
    }
    captured
        .lock()
        .unwrap()
        .push(String::from_utf8_lossy(&buffer).into_owned());

    let body = format!(r#"{{"ret":0,"errcode":0,"message_id":"{PLATFORM_MESSAGE_ID}"}}"#);
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.flush().await;
}

struct Harness {
    platform: FakePlatform,
    store: Arc<SqliteStore>,
    account: agentnotify_channel_sdk::ChannelAccount,
    credentials: ClawBotCredentials,
    agent: Arc<FakeAgent>,
    ingest: IngestService,
    delivery: DeliveryService,
    reply: ReplyService,
}

async fn harness() -> Harness {
    let platform = FakePlatform::start().await;
    // 内存库：SQLite 连接由专用线程持有，落盘临时文件会在测试结束时留下残渣。
    let store = Arc::new(SqliteStore::open(":memory:").unwrap());
    let secrets: Arc<dyn agentnotify_application::SecretStore> = Arc::new(MemoryStore::default());
    let channel = Arc::new(ClawBotChannel::new(secrets).with_account_store(store.clone()));

    let account = ClawBotAccount::from_platform_ids(BOT_ID, USER_ID)
        .unwrap()
        .with_base_url(platform.base_url.clone(), timestamp("2026-09-19T09:00:00Z"))
        .unwrap()
        .into_channel_account()
        .unwrap();
    let credentials =
        ClawBotCredentials::new(BOT_TOKEN, BOT_ID, USER_ID, platform.base_url.clone()).unwrap();
    channel
        .save_credentials(&account.id, &credentials)
        .await
        .unwrap();
    channel
        .save_context(
            &account.id,
            &ClawBotContext::new(CONTEXT_TOKEN, USER_ID).unwrap(),
        )
        .await
        .unwrap();

    store.upsert(account.clone()).await.unwrap();

    let agent = Arc::new(FakeAgent::new(AGENT_ID));
    let mut agents = AgentRegistry::default();
    agents.register(agent.clone()).unwrap();
    let agents = Arc::new(agents);

    let mut channels = ChannelRegistry::default();
    channels.register(channel).unwrap();
    let channels = Arc::new(channels);

    let clock = FakeClock::new(timestamp("2026-09-19T09:00:00Z"));
    let ids = SequenceIdGenerator::default();
    let events: Arc<dyn EventSink> = Arc::new(NoopEventSink);

    let ingest = IngestService::new(
        agents.clone(),
        store.clone(),
        events.clone(),
        Arc::new(clock.clone()),
        Arc::new(ids.clone()),
        NotificationPolicy::default().with_agent(
            AgentId::new(AGENT_ID).unwrap(),
            AgentNotificationConfig::default(),
        ),
    );
    let delivery = DeliveryService::new(
        store.clone(),
        channels.clone(),
        vec![DeliveryTarget::new(account.clone(), USER_ID)],
        Arc::new(clock.clone()),
        Arc::new(ids.clone()),
        events.clone(),
        RetryPolicy::default(),
    );
    let reply = ReplyService::new(
        store.clone(),
        store.clone(),
        channels,
        agents,
        Arc::new(clock),
        events,
        vec![ReplyTarget::new(account.clone(), USER_ID, USER_ID).unwrap()],
        ReplyConfig {
            enabled: true,
            send_confirmation: false,
            ..ReplyConfig::default()
        },
        None,
    )
    .unwrap();

    Harness {
        platform,
        store,
        account,
        credentials,
        agent,
        ingest,
        delivery,
        reply,
    }
}

fn quoted(text: &str, reference: &str) -> ClawBotInboundMessage {
    serde_json::from_value(serde_json::json!({
        "seq": 7,
        "msg_id": format!("reply-{reference}"),
        "from_user_id": USER_ID,
        "to_user_id": BOT_ID,
        "message_type": 1,
        "context_token": CONTEXT_TOKEN,
        "item_list": [{
            "type": 1,
            "text_item": { "text": text },
            "ref_msg": { "message_item": { "msg_id": reference } }
        }]
    }))
    .unwrap()
}

#[tokio::test]
async fn production_clawbot_adapter_creates_route_from_platform_message_id() {
    let harness = harness().await;

    let ingested = harness
        .ingest
        .ingest(completed_event("req-1", "event-1", SESSION_ID))
        .await
        .unwrap();
    assert!(matches!(ingested, IngestResult::Queued { .. }));

    let outcome = harness.delivery.process_next().await.unwrap();
    assert!(matches!(outcome, ProcessOutcome::Completed { .. }));

    let requests = harness.platform.requests();
    assert_eq!(requests.len(), 1, "每次投递只应触发一次平台请求");
    let request = &requests[0];
    assert!(
        request.starts_with(SEND_ENDPOINT),
        "出站必须打 ClawBot sendmessage 端点：{}",
        request.lines().next().unwrap_or_default()
    );
    assert!(
        request.to_ascii_lowercase().contains("authorization:"),
        "出站必须携带 bot token 授权头"
    );

    // ReplyRoute 必须挂在平台返回的稳定 message ID 上，而不是标题或正文。
    let route = harness
        .store
        .find_route(
            &RouteKey::new(
                ChannelId::new(CLAWBOT_CHANNEL_ID).unwrap(),
                harness.account.id.clone(),
                ExternalMessageId::new(PLATFORM_MESSAGE_ID).unwrap(),
            ),
            timestamp("2026-09-19T09:00:03Z"),
        )
        .await
        .unwrap()
        .expect("成功投递后必须建立可引用路由");
    assert_eq!(route.session_id.as_str(), SESSION_ID);
    assert_eq!(route.agent_id.as_str(), AGENT_ID);
}

#[tokio::test]
async fn production_clawbot_loop_replies_only_to_quoted_route() {
    let harness = harness().await;

    harness
        .ingest
        .ingest(completed_event("req-1", "event-1", SESSION_ID))
        .await
        .unwrap();
    harness.delivery.process_next().await.unwrap();

    let inbound = normalize_inbound(
        &harness.account.id,
        &harness.credentials,
        quoted("继续处理", PLATFORM_MESSAGE_ID),
        None,
        timestamp("2026-09-19T08:59:58Z"),
    )
    .unwrap();
    let claim_key = inbound.claim_key().unwrap();
    let outcome = harness.reply.handle(inbound).await.unwrap();
    assert!(matches!(outcome, ReplyOutcome::Accepted { .. }));
    assert_eq!(harness.agent.resume_count(), 1);
    assert_eq!(harness.agent.last_session().unwrap().as_str(), SESSION_ID);

    let claim = harness.store.find_claim(&claim_key).await.unwrap().unwrap();
    assert_eq!(claim.state, ClaimState::Completed);

    // 引用一条没有路由的消息：必须拒绝，且不得再次唤起 Agent。
    let unmatched = normalize_inbound(
        &harness.account.id,
        &harness.credentials,
        quoted("继续处理", "msg-unknown"),
        None,
        timestamp("2026-09-19T08:59:57Z"),
    )
    .unwrap();
    let outcome = harness.reply.handle(unmatched).await.unwrap();
    assert!(matches!(
        outcome,
        ReplyOutcome::Rejected(ReplyRejection::NoExactRoute)
    ));
    assert_eq!(harness.agent.resume_count(), 1);
}
