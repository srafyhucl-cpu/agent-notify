use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use agentnotify_application::{
    ChannelAccountStore, SecretError, SecretKind, SecretStore, SecretValue, StoreError,
};
use agentnotify_channel_clawbot::{
    ClawBotAccount, ClawBotAccountState, ClawBotChannel, ClawBotContext, ClawBotCredentials,
    ClawBotHttpResponse, ClawBotHttpSendTransport, ClawBotSendRequest, ClawBotSendTransport,
    MAX_TEXT_BYTES, NotificationRenderInput, render_notification,
};
use agentnotify_channel_sdk::{
    ChannelAccount, ChannelAdapter, ChannelError, NotificationPresentation, OutboundMessage,
};
use agentnotify_domain::{ChannelAccountId, ChannelId, DeliveryState, Timestamp};
use async_trait::async_trait;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

#[test]
fn notification_uses_descriptor_display_name_and_standard_markdown() {
    let rendered = render_notification(NotificationRenderInput {
        agent_display_name: "Registry Agent".into(),
        session_name: "修复登录".into(),
        body: "正文 **Markdown**".into(),
        occurred_at: timestamp("2026-09-19T10:20:30+08:00"),
        include_footer: true,
        replyable: true,
    })
    .unwrap();

    assert_eq!(
        rendered,
        "**🟢 Registry Agent｜修复登录**\n\n正文 **Markdown**\n\n—\n*引用此消息可继续对话* · 09/19 10:20"
    );
}

#[test]
fn footer_can_be_disabled_without_fabricating_reply_support() {
    let rendered = render_notification(NotificationRenderInput {
        agent_display_name: "Registry Agent".into(),
        session_name: "任务完成".into(),
        body: "正文".into(),
        occurred_at: timestamp("2026-09-19T10:20:30+08:00"),
        include_footer: false,
        replyable: true,
    })
    .unwrap();

    assert_eq!(rendered, "**🟢 Registry Agent｜任务完成**\n\n正文");
}

#[test]
fn oversized_rendered_notification_is_permanent() {
    let error = render_notification(NotificationRenderInput {
        agent_display_name: "Registry Agent".into(),
        session_name: "任务完成".into(),
        body: "字".repeat(MAX_TEXT_BYTES),
        occurred_at: timestamp("2026-09-19T10:20:30+08:00"),
        include_footer: true,
        replyable: true,
    })
    .unwrap_err();

    assert!(matches!(error, ChannelError::Permanent(_)));
}

#[tokio::test]
async fn structured_notification_is_rendered_with_title_bar_and_footer() {
    let capture = Arc::new(Mutex::new(String::new()));
    let base_url =
        spawn_json_server(r#"{"ret":0,"message_id":"platform-1"}"#, capture.clone()).await;
    let fixture = fixture(Arc::new(ClawBotHttpSendTransport::new()), &base_url).await;
    let message = fixture
        .message
        .clone()
        .with_notification(NotificationPresentation {
            agent_display_name: "Registry Agent".into(),
            session_name: "修复登录".into(),
            occurred_at: timestamp("2026-09-19T10:20:30+08:00"),
            include_footer: true,
            replyable: true,
        });

    let receipt = fixture
        .channel
        .send(fixture.account, message)
        .await
        .unwrap();

    assert_eq!(receipt.state, DeliveryState::Sent);
    let request = capture.lock().unwrap().clone();
    assert_eq!(
        request_text(&request),
        "**🟢 Registry Agent｜修复登录**\n\nhello **world**\n\n—\n*引用此消息可继续对话* · 09/19 10:20"
    );
}

#[tokio::test]
async fn notification_without_presentation_keeps_raw_text() {
    let capture = Arc::new(Mutex::new(String::new()));
    let base_url =
        spawn_json_server(r#"{"ret":0,"message_id":"platform-1"}"#, capture.clone()).await;
    let fixture = fixture(Arc::new(ClawBotHttpSendTransport::new()), &base_url).await;

    let receipt = fixture
        .channel
        .send(fixture.account, fixture.message)
        .await
        .unwrap();

    assert_eq!(receipt.state, DeliveryState::Sent);
    let request = capture.lock().unwrap().clone();
    assert_eq!(request_text(&request), "hello **world**");
}

#[tokio::test]
async fn oversized_rendered_notification_is_rejected_before_network_access() {
    let transport = Arc::new(TestTransport::http(200, r#"{"ret":0,"message_id":"m1"}"#));
    let fixture = fixture(transport.clone(), "https://business.example.test").await;
    let message =
        OutboundMessage::notification("user-1", "x".repeat(MAX_TEXT_BYTES), "client-oversized")
            .unwrap()
            .with_notification(NotificationPresentation {
                agent_display_name: "Registry Agent".into(),
                session_name: "任务完成".into(),
                occurred_at: timestamp("2026-09-19T10:20:30+08:00"),
                include_footer: true,
                replyable: true,
            });

    let error = fixture
        .channel
        .send(fixture.account, message)
        .await
        .unwrap_err();

    assert!(matches!(error, ChannelError::Permanent(_)));
    assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
}

/// 从抓到的 HTTP 请求里取出 JSON 正文里的文本项，避免用整串匹配。
fn request_text(request: &str) -> String {
    let (_, body) = request
        .split_once("\r\n\r\n")
        .expect("抓包必须包含请求头与正文分隔");
    let value: serde_json::Value = serde_json::from_str(body).expect("出站正文必须是 JSON");
    value["msg"]["item_list"][0]["text_item"]["text"]
        .as_str()
        .expect("出站正文必须包含文本项")
        .to_owned()
}

#[tokio::test]
async fn timeout_maps_to_unknown() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (_stream, _) = listener.accept().await.unwrap();
        tokio::time::sleep(Duration::from_millis(250)).await;
    });
    let fixture = fixture(
        Arc::new(ClawBotHttpSendTransport::with_timeout(
            Duration::from_millis(40),
        )),
        &format!("http://{address}"),
    )
    .await;

    let error = fixture
        .channel
        .send(fixture.account, fixture.message)
        .await
        .unwrap_err();

    assert!(matches!(error, ChannelError::Unknown(_)));
}

#[tokio::test]
async fn default_http_transport_sends_protocol_request_and_returns_platform_id() {
    let capture = Arc::new(Mutex::new(String::new()));
    let base_url =
        spawn_json_server(r#"{"ret":0,"message_id":"platform-1"}"#, capture.clone()).await;
    let fixture = fixture(Arc::new(ClawBotHttpSendTransport::new()), &base_url).await;

    let receipt = fixture
        .channel
        .send(fixture.account, fixture.message)
        .await
        .unwrap();

    assert_eq!(receipt.state, DeliveryState::Sent);
    assert_eq!(
        receipt.external_message_id.as_ref().unwrap().as_str(),
        "platform-1"
    );
    let request = capture.lock().unwrap().to_ascii_lowercase();
    assert!(request.contains("authorization: bearer bot-secret"));
    assert!(request.contains("authorizationtype: ilink_bot_token"));
    assert!(request.contains("\"to_user_id\":\"user-1\""));
    assert!(request.contains("\"client_id\":\"client-1\""));
    assert!(request.contains("\"context_token\":\"context-secret\""));
    assert!(request.contains("\"channel_version\":\"2.4.6\""));
}

#[tokio::test]
async fn http_408_429_and_5xx_are_retryable() {
    for status in [408, 429, 500, 503] {
        let fixture = fixture(
            Arc::new(TestTransport::http(status, "temporary failure")),
            "https://business.example.test",
        )
        .await;

        let error = fixture
            .channel
            .send(fixture.account, fixture.message)
            .await
            .unwrap_err();

        assert!(
            matches!(error, ChannelError::Retryable { .. }),
            "status={status}"
        );
    }
}

#[tokio::test]
async fn parameter_and_size_rejections_are_permanent() {
    for status in [400, 413] {
        let fixture = fixture(
            Arc::new(TestTransport::http(status, "invalid message size")),
            "https://business.example.test",
        )
        .await;

        let error = fixture
            .channel
            .send(fixture.account, fixture.message)
            .await
            .unwrap_err();

        assert!(
            matches!(error, ChannelError::Permanent(_)),
            "status={status}"
        );
    }
}

#[tokio::test]
async fn explicit_server_retry_error_is_retryable() {
    let fixture = fixture(
        Arc::new(TestTransport::http(
            200,
            r#"{"ret":123,"errcode":456,"errmsg":"server busy, please retry"}"#,
        )),
        "https://business.example.test",
    )
    .await;

    let error = fixture
        .channel
        .send(fixture.account, fixture.message)
        .await
        .unwrap_err();

    assert!(matches!(error, ChannelError::Retryable { .. }));
}

#[tokio::test]
async fn invalid_account_clears_context_cursor_and_marks_account_stale() {
    for body in [
        r#"{"ret":-14,"errmsg":"token expired"}"#,
        r#"{"ret":0,"errcode":-14,"errmsg":"token expired"}"#,
    ] {
        let fixture = fixture(
            Arc::new(TestTransport::http(200, body)),
            "https://business.example.test",
        )
        .await;
        let account_id = fixture.account.id.clone();

        let error = fixture
            .channel
            .send(fixture.account, fixture.message)
            .await
            .unwrap_err();

        assert!(matches!(error, ChannelError::InvalidAccount(_)));
        assert!(!error.to_string().contains("bot-secret"));
        assert!(!error.to_string().contains("context-secret"));
        assert!(matches!(
            fixture
                .secrets
                .get(&account_id, SecretKind::ContextToken)
                .await,
            Err(SecretError { .. })
        ));
        assert!(
            fixture
                .secrets
                .get(&account_id, SecretKind::BotToken)
                .await
                .is_ok()
        );
        let stored = fixture.accounts.get(&account_id).await.unwrap().unwrap();
        let state: ClawBotAccountState = serde_json::from_value(stored.config).unwrap();
        assert!(state.stale_at.is_some());
        assert_eq!(stored.cursor["get_updates_buf"], "");
    }
}

#[tokio::test]
async fn prepare_failed_returns_skipped_and_clears_only_context() {
    let fixture = fixture(
        Arc::new(TestTransport::http(
            200,
            r#"{"ret":-2,"errmsg":"prepare failed"}"#,
        )),
        "https://business.example.test",
    )
    .await;
    let account_id = fixture.account.id.clone();

    let receipt = fixture
        .channel
        .send(fixture.account, fixture.message)
        .await
        .unwrap();

    assert_eq!(receipt.state, DeliveryState::Skipped);
    let error = receipt.error.as_ref().expect("跳过必须带可读原因");
    assert_eq!(error.code(), "session_missing");
    assert_eq!(
        error.message(),
        "平台未能准备会话，请给 ClawBot 发送一条消息后重试"
    );
    assert!(matches!(
        fixture
            .secrets
            .get(&account_id, SecretKind::ContextToken)
            .await,
        Err(SecretError { .. })
    ));
    let stored = fixture.accounts.get(&account_id).await.unwrap().unwrap();
    assert!(stored.config["stale_at"].is_null());
    assert_eq!(stored.cursor["get_updates_buf"], "cursor-1");
}

#[tokio::test]
async fn success_without_platform_id_is_unknown_and_keeps_client_id_only_as_metadata() {
    let fixture = fixture(
        Arc::new(TestTransport::http(
            200,
            r#"{"ret":0,"client_id":"server-client"}"#,
        )),
        "https://business.example.test",
    )
    .await;

    let receipt = fixture
        .channel
        .send(fixture.account, fixture.message)
        .await
        .unwrap();

    assert_eq!(receipt.state, DeliveryState::Unknown);
    assert!(receipt.external_message_id.is_none());
    assert_eq!(
        receipt.error.as_ref().unwrap().code(),
        "clawbot_message_id_missing"
    );
    assert_eq!(
        receipt.raw_safe_metadata.get("client_id").unwrap(),
        "client-1"
    );
    assert!(
        fixture
            .accounts
            .get(&fixture.account_id)
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn conflicting_platform_ids_are_unknown() {
    let fixture = fixture(
        Arc::new(TestTransport::http(
            200,
            r#"{"ret":0,"message_id":"m1","data":{"msg_id":"m2"}}"#,
        )),
        "https://business.example.test",
    )
    .await;

    let receipt = fixture
        .channel
        .send(fixture.account, fixture.message)
        .await
        .unwrap();

    assert_eq!(receipt.state, DeliveryState::Unknown);
    assert!(receipt.external_message_id.is_none());
    assert_eq!(
        receipt.error.as_ref().unwrap().code(),
        "clawbot_message_id_ambiguous"
    );
}

#[tokio::test]
async fn mismatched_context_user_is_skipped_without_network_access() {
    let transport = Arc::new(TestTransport::http(200, r#"{"ret":0,"message_id":"m1"}"#));
    let fixture = fixture(transport.clone(), "https://business.example.test").await;
    let wrong_context = ClawBotContext::new("context-secret", "user-2").unwrap();
    fixture
        .channel
        .save_context(&fixture.account_id, &wrong_context)
        .await
        .unwrap();

    let receipt = fixture
        .channel
        .send(fixture.account, fixture.message)
        .await
        .unwrap();

    assert_eq!(receipt.state, DeliveryState::Skipped);
    let error = receipt.error.as_ref().expect("跳过必须带可读原因");
    assert_eq!(error.code(), "session_missing");
    assert_eq!(
        error.message(),
        "ClawBot 主动推送会话已失效，请先给 ClawBot 发送一条消息"
    );
    assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn missing_context_is_skipped_with_actionable_message() {
    let transport = Arc::new(TestTransport::http(200, r#"{"ret":0,"message_id":"m1"}"#));
    let fixture = fixture(transport.clone(), "https://business.example.test").await;
    fixture
        .secrets
        .delete(&fixture.account_id, SecretKind::ContextToken)
        .await
        .unwrap();

    let receipt = fixture
        .channel
        .send(fixture.account, fixture.message)
        .await
        .unwrap();

    assert_eq!(receipt.state, DeliveryState::Skipped);
    let error = receipt.error.as_ref().expect("跳过必须带可读原因");
    assert_eq!(error.code(), "session_missing");
    assert_eq!(
        error.message(),
        "ClawBot 主动推送会话已失效，请先给 ClawBot 发送一条消息"
    );
    assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
}

struct Fixture {
    channel: ClawBotChannel,
    account: ChannelAccount,
    account_id: ChannelAccountId,
    message: OutboundMessage,
    secrets: Arc<TestSecrets>,
    accounts: Arc<TestAccounts>,
}

async fn fixture(transport: Arc<dyn ClawBotSendTransport>, base_url: &str) -> Fixture {
    let secrets = Arc::new(TestSecrets::default());
    let accounts = Arc::new(TestAccounts::default());
    let account = ClawBotAccount::from_platform_ids("bot-1", "user-1")
        .unwrap()
        .with_base_url(base_url, Timestamp::now_utc())
        .unwrap();
    let mut channel_account = account.channel_account().clone();
    channel_account.cursor = serde_json::json!({"get_updates_buf": "cursor-1"});
    accounts.upsert(channel_account.clone()).await.unwrap();
    let channel = ClawBotChannel::with_send_transport(secrets.clone(), transport)
        .with_account_store(accounts.clone());
    channel
        .save_credentials(
            account.id(),
            &ClawBotCredentials::new("bot-secret", "bot-1", "user-1", base_url).unwrap(),
        )
        .await
        .unwrap();
    channel
        .save_context(
            account.id(),
            &ClawBotContext::new("context-secret", "user-1").unwrap(),
        )
        .await
        .unwrap();
    Fixture {
        channel,
        account: channel_account,
        account_id: account.id().clone(),
        message: OutboundMessage::notification("user-1", "hello **world**", "client-1").unwrap(),
        secrets,
        accounts,
    }
}

struct TestTransport {
    response: TransportResponse,
    calls: Arc<AtomicUsize>,
}

enum TransportResponse {
    Http { status: u16, body: String },
}

impl TestTransport {
    fn http(status: u16, body: impl Into<String>) -> Self {
        Self {
            response: TransportResponse::Http {
                status,
                body: body.into(),
            },
            calls: Arc::new(AtomicUsize::new(0)),
        }
    }
}

#[async_trait]
impl ClawBotSendTransport for TestTransport {
    async fn send_message(
        &self,
        _request: ClawBotSendRequest,
    ) -> Result<ClawBotHttpResponse, ChannelError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        match &self.response {
            TransportResponse::Http { status, body } => {
                Ok(ClawBotHttpResponse::new(*status, body.clone()))
            }
        }
    }
}

#[derive(Default)]
struct TestSecrets {
    values: Mutex<HashMap<(ChannelAccountId, SecretKind), SecretValue>>,
}

#[async_trait]
impl SecretStore for TestSecrets {
    async fn get(
        &self,
        account_id: &ChannelAccountId,
        kind: SecretKind,
    ) -> Result<SecretValue, SecretError> {
        self.values
            .lock()
            .unwrap()
            .get(&(account_id.clone(), kind))
            .cloned()
            .ok_or_else(|| SecretError::new("secret_not_found", "找不到测试密钥"))
    }

    async fn set(
        &self,
        account_id: &ChannelAccountId,
        kind: SecretKind,
        value: SecretValue,
    ) -> Result<(), SecretError> {
        self.values
            .lock()
            .unwrap()
            .insert((account_id.clone(), kind), value);
        Ok(())
    }

    async fn delete(
        &self,
        account_id: &ChannelAccountId,
        kind: SecretKind,
    ) -> Result<(), SecretError> {
        self.values
            .lock()
            .unwrap()
            .remove(&(account_id.clone(), kind));
        Ok(())
    }
}

#[derive(Default)]
struct TestAccounts {
    values: Mutex<HashMap<ChannelAccountId, ChannelAccount>>,
}

#[async_trait]
impl ChannelAccountStore for TestAccounts {
    async fn get(
        &self,
        account_id: &ChannelAccountId,
    ) -> Result<Option<ChannelAccount>, StoreError> {
        Ok(self.values.lock().unwrap().get(account_id).cloned())
    }

    async fn list(&self, channel_id: &ChannelId) -> Result<Vec<ChannelAccount>, StoreError> {
        Ok(self
            .values
            .lock()
            .unwrap()
            .values()
            .filter(|account| &account.channel_id == channel_id)
            .cloned()
            .collect())
    }

    async fn upsert(&self, account: ChannelAccount) -> Result<(), StoreError> {
        self.values
            .lock()
            .unwrap()
            .insert(account.id.clone(), account);
        Ok(())
    }

    async fn set_enabled(
        &self,
        _account_id: &ChannelAccountId,
        _enabled: bool,
    ) -> Result<(), StoreError> {
        Ok(())
    }
}

async fn spawn_json_server(response_body: &'static str, capture: Arc<Mutex<String>>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut buffer = [0_u8; 1024];
        let header_end;
        loop {
            let read = stream.read(&mut buffer).await.unwrap();
            if read == 0 {
                return;
            }
            request.extend_from_slice(&buffer[..read]);
            if let Some(index) = request.windows(4).position(|window| window == b"\r\n\r\n") {
                header_end = index + 4;
                break;
            }
        }
        let headers = String::from_utf8_lossy(&request[..header_end]);
        let content_length = headers
            .lines()
            .find_map(|line| {
                line.to_ascii_lowercase()
                    .strip_prefix("content-length:")
                    .and_then(|value| value.trim().parse::<usize>().ok())
            })
            .unwrap_or(0);
        while request.len() < header_end + content_length {
            let read = stream.read(&mut buffer).await.unwrap();
            if read == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..read]);
        }
        *capture.lock().unwrap() = String::from_utf8_lossy(&request).into_owned();
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            response_body.len(),
            response_body
        );
        stream.write_all(response.as_bytes()).await.unwrap();
    });
    format!("http://{address}")
}

fn timestamp(value: &str) -> Timestamp {
    Timestamp::parse_rfc3339(value).unwrap()
}
