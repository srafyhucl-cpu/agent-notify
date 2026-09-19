use std::{
    collections::{HashMap, VecDeque},
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
    ClawBotAccount, ClawBotAccountState, ClawBotChannel, ClawBotCredentials,
    ClawBotGetUpdatesRequest, ClawBotInboundMessage, ClawBotLifecycleRequest,
    ClawBotSessionTransport, ClawBotUpdates,
};
use agentnotify_channel_sdk::{ChannelAccount, ChannelAdapter, ChannelError};
use agentnotify_domain::{ChannelAccountId, ChannelId, InboundMessage, Timestamp};
use async_trait::async_trait;
use serde_json::json;
use tokio::{sync::mpsc, time::Instant};

#[tokio::test]
async fn cursor_is_committed_only_after_inbound_is_handled() {
    let message = inbound_message("message-1", "context-1", "继续");
    let transport = Arc::new(ScriptedTransport::new(vec![Ok(ClawBotUpdates {
        messages: vec![message],
        cursor: "cursor-1".into(),
    })]));
    let fixture = Fixture::new(transport.clone(), true).await;
    let (emit, mut receiver) = mpsc::channel(1);
    let dummy = dummy_inbound(&fixture.account_id);
    emit.send(dummy).await.unwrap();

    let task = fixture
        .channel
        .start(fixture.account.clone(), emit)
        .await
        .unwrap();
    wait_until("首次 getupdates", || transport.request_count() >= 1).await;
    assert_eq!(transport.start_count(), 1);
    tokio::time::sleep(Duration::from_millis(40)).await;

    let account = fixture
        .accounts
        .get(&fixture.account_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(account.cursor, json!({ "get_updates_buf": "cursor-0" }));

    let _ = receiver.recv().await.expect("预置入站消息应存在");
    let inbound = receiver.recv().await.expect("归一化消息应进入核心");
    assert_eq!(inbound.text, "继续");
    wait_for_cursor(&fixture.accounts, &fixture.account_id, "cursor-1").await;

    let account = fixture
        .accounts
        .get(&fixture.account_id)
        .await
        .unwrap()
        .unwrap();
    let state = account_state(&account);
    assert!(state.stale_at.is_none());
    assert!(state.session_established_at.is_some());
    let context = fixture
        .channel
        .load_context(&fixture.account_id)
        .await
        .unwrap();
    assert_eq!(context.context_token(), "context-1");
    assert_eq!(context.user_id(), "user-1");
    wait_until("下一次轮询使用新游标", || {
        transport
            .requests()
            .get(1)
            .is_some_and(|request| request.cursor == "cursor-1")
    })
    .await;

    let result = tokio::time::timeout(Duration::from_secs(1), task.shutdown())
        .await
        .expect("渠道任务应及时退出");
    assert!(result.is_ok());
    assert_eq!(transport.stop_count(), 1);
}

#[tokio::test]
async fn invalid_message_does_not_establish_session_context() {
    let message = inbound_message("message-1", "context-1", " ");
    let transport = Arc::new(ScriptedTransport::new(vec![Ok(ClawBotUpdates {
        messages: vec![message],
        cursor: "cursor-1".into(),
    })]));
    let fixture = Fixture::new(transport.clone(), true).await;
    let (emit, mut receiver) = mpsc::channel(4);

    let task = fixture
        .channel
        .start(fixture.account.clone(), emit)
        .await
        .unwrap();
    wait_for_cursor(&fixture.accounts, &fixture.account_id, "cursor-1").await;

    assert!(receiver.try_recv().is_err());
    let context = fixture
        .channel
        .load_context(&fixture.account_id)
        .await
        .unwrap();
    assert_eq!(context.context_token(), "old-context");
    let account = fixture
        .accounts
        .get(&fixture.account_id)
        .await
        .unwrap()
        .unwrap();
    let state = account_state(&account);
    assert!(state.stale_at.is_some());
    assert!(state.session_established_at.is_none());

    let _ = task.shutdown().await;
    assert_eq!(transport.stop_count(), 1);
}

#[tokio::test]
async fn ret_minus_14_marks_account_stale_and_clears_session_state() {
    let transport = Arc::new(ScriptedTransport::new(vec![Err(
        ChannelError::invalid_account("clawbot_invalid_account", "登录已失效"),
    )]));
    let fixture = Fixture::new(transport.clone(), false).await;
    let (emit, _receiver) = mpsc::channel(4);

    let task = fixture
        .channel
        .start(fixture.account.clone(), emit)
        .await
        .unwrap();
    wait_until("轮询失败退出", || task.is_finished()).await;

    let result = task.shutdown().await;
    assert!(matches!(result, Err(ChannelError::InvalidAccount(_))));
    let account = fixture
        .accounts
        .get(&fixture.account_id)
        .await
        .unwrap()
        .unwrap();
    let state = account_state(&account);
    assert!(state.stale_at.is_some());
    assert_eq!(account.cursor, json!({ "get_updates_buf": "" }));
    assert!(
        fixture
            .secrets
            .get_sync(&fixture.account_id, SecretKind::ContextToken)
            .is_none()
    );
    assert_eq!(transport.stop_count(), 1);
}

struct Fixture {
    channel: ClawBotChannel,
    account: ChannelAccount,
    account_id: ChannelAccountId,
    secrets: Arc<TestSecrets>,
    accounts: Arc<TestAccounts>,
}

impl Fixture {
    async fn new(transport: Arc<ScriptedTransport>, stale: bool) -> Self {
        let secrets = Arc::new(TestSecrets::default());
        let accounts = Arc::new(TestAccounts::default());
        let account = ClawBotAccount::from_platform_ids_at(
            "bot-1",
            "user-1",
            Timestamp::parse_rfc3339("2026-09-19T10:00:00Z").unwrap(),
        )
        .unwrap();
        let account_id = account.id().clone();
        let credentials = credentials();
        let mut account = account.into_channel_account().unwrap();
        let mut state = account_state(&account);
        if stale {
            state.stale_at = Some(Timestamp::now_utc());
        } else {
            state.stale_at = None;
        }
        account.config = serde_json::to_value(state).unwrap();
        account.cursor = json!({ "get_updates_buf": "cursor-0" });
        accounts.upsert(account.clone()).await.unwrap();

        let channel = ClawBotChannel::new(secrets.clone())
            .with_account_store(accounts.clone())
            .with_session_transport(transport);
        channel
            .save_credentials(&account_id, &credentials)
            .await
            .unwrap();
        channel
            .save_context(
                &account_id,
                &agentnotify_channel_clawbot::ClawBotContext::new("old-context", "user-1").unwrap(),
            )
            .await
            .unwrap();

        Self {
            channel,
            account,
            account_id,
            secrets,
            accounts,
        }
    }
}

struct ScriptedTransport {
    responses: Mutex<VecDeque<Result<ClawBotUpdates, ChannelError>>>,
    requests: Mutex<Vec<ClawBotGetUpdatesRequest>>,
    starts: AtomicUsize,
    stops: AtomicUsize,
}

impl ScriptedTransport {
    fn new(responses: Vec<Result<ClawBotUpdates, ChannelError>>) -> Self {
        Self {
            responses: Mutex::new(responses.into()),
            requests: Mutex::new(Vec::new()),
            starts: AtomicUsize::new(0),
            stops: AtomicUsize::new(0),
        }
    }

    fn request_count(&self) -> usize {
        self.requests.lock().unwrap().len()
    }

    fn start_count(&self) -> usize {
        self.starts.load(Ordering::SeqCst)
    }

    fn requests(&self) -> Vec<ClawBotGetUpdatesRequest> {
        self.requests.lock().unwrap().clone()
    }

    fn stop_count(&self) -> usize {
        self.stops.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl ClawBotSessionTransport for ScriptedTransport {
    async fn get_updates(
        &self,
        request: ClawBotGetUpdatesRequest,
    ) -> Result<ClawBotUpdates, ChannelError> {
        self.requests.lock().unwrap().push(request);
        if let Some(response) = self.responses.lock().unwrap().pop_front() {
            return response;
        }
        std::future::pending::<()>().await;
        unreachable!("pending future 不会完成")
    }

    async fn notify_start(&self, _request: ClawBotLifecycleRequest) -> Result<(), ChannelError> {
        self.starts.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn notify_stop(&self, _request: ClawBotLifecycleRequest) -> Result<(), ChannelError> {
        self.stops.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[derive(Default)]
struct TestSecrets {
    values: Mutex<HashMap<(ChannelAccountId, SecretKind), SecretValue>>,
}

impl TestSecrets {
    fn get_sync(&self, account_id: &ChannelAccountId, kind: SecretKind) -> Option<SecretValue> {
        self.values
            .lock()
            .unwrap()
            .get(&(account_id.clone(), kind))
            .cloned()
    }
}

#[async_trait]
impl SecretStore for TestSecrets {
    async fn get(
        &self,
        account_id: &ChannelAccountId,
        kind: SecretKind,
    ) -> Result<SecretValue, SecretError> {
        self.get_sync(account_id, kind)
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
    values: tokio::sync::RwLock<HashMap<ChannelAccountId, ChannelAccount>>,
}

impl TestAccounts {
    async fn get(
        &self,
        account_id: &ChannelAccountId,
    ) -> Result<Option<ChannelAccount>, StoreError> {
        Ok(self.values.read().await.get(account_id).cloned())
    }
}

#[async_trait]
impl ChannelAccountStore for TestAccounts {
    async fn get(
        &self,
        account_id: &ChannelAccountId,
    ) -> Result<Option<ChannelAccount>, StoreError> {
        TestAccounts::get(self, account_id).await
    }

    async fn list(&self, channel_id: &ChannelId) -> Result<Vec<ChannelAccount>, StoreError> {
        Ok(self
            .values
            .read()
            .await
            .values()
            .filter(|account| &account.channel_id == channel_id)
            .cloned()
            .collect())
    }

    async fn upsert(&self, account: ChannelAccount) -> Result<(), StoreError> {
        self.values
            .write()
            .await
            .insert(account.id.clone(), account);
        Ok(())
    }

    async fn set_enabled(
        &self,
        account_id: &ChannelAccountId,
        enabled: bool,
    ) -> Result<(), StoreError> {
        let mut values = self.values.write().await;
        let account = values
            .get_mut(account_id)
            .ok_or_else(|| StoreError::not_found("account_missing", "找不到测试渠道账号"))?;
        account.enabled = enabled;
        Ok(())
    }
}

fn credentials() -> ClawBotCredentials {
    ClawBotCredentials::new(
        "bot-token",
        "bot-1",
        "user-1",
        "https://business.example.test",
    )
    .unwrap()
}

fn inbound_message(message_id: &str, context_token: &str, text: &str) -> ClawBotInboundMessage {
    serde_json::from_value(json!({
        "seq": 1,
        "msg_id": message_id,
        "from_user_id": "user-1",
        "to_user_id": "bot-1",
        "message_type": 1,
        "context_token": context_token,
        "item_list": [{
            "type": 1,
            "text_item": { "text": text }
        }]
    }))
    .unwrap()
}

fn account_state(account: &ChannelAccount) -> ClawBotAccountState {
    serde_json::from_value(account.config.clone()).unwrap()
}

fn dummy_inbound(account_id: &ChannelAccountId) -> InboundMessage {
    InboundMessage::without_external_id(
        ChannelId::new("clawbot").unwrap(),
        account_id.clone(),
        "dummy-cursor",
        Vec::new(),
        "预置",
        Timestamp::now_utc(),
    )
    .unwrap()
}

async fn wait_until(label: &str, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if condition() {
            return;
        }
        assert!(Instant::now() < deadline, "等待超时：{label}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

async fn wait_for_cursor(accounts: &TestAccounts, account_id: &ChannelAccountId, cursor: &str) {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let current = accounts
            .get(account_id)
            .await
            .unwrap()
            .map(|account| account.cursor);
        if current == Some(json!({ "get_updates_buf": cursor })) {
            return;
        }
        assert!(Instant::now() < deadline, "等待游标超时：{cursor}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}
