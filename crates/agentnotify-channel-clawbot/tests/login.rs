use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use agentnotify_application::{
    ChannelAccountStore, SecretError, SecretKind, SecretStore, SecretValue, StoreError,
};
use agentnotify_channel_clawbot::{
    ClawBotAccount, ClawBotAuthTransport, ClawBotLoginAdapter, LoginDecision, LoginMachine,
    QrCodeResponse, QrStatusResponse, qr_data_url,
};
use agentnotify_channel_sdk::{
    BeginLoginRequest, ChannelAccount, ChannelError, ChannelLoginAdapter, LoginSessionState,
    assert_channel_login_contract,
};
use agentnotify_domain::{ChannelAccountId, ChannelId};
use async_trait::async_trait;

#[test]
fn scaned_then_need_verify_then_confirmed_keeps_one_login_session() {
    let mut machine = LoginMachine::new("qr-code");

    assert_eq!(
        machine.apply(status("wait")).unwrap(),
        LoginDecision::Continue
    );
    assert_eq!(machine.state(), LoginSessionState::WaitingScan);
    assert_eq!(
        machine.apply(status("scaned")).unwrap(),
        LoginDecision::Continue
    );
    assert_eq!(
        machine.apply(status("need_verifycode")).unwrap(),
        LoginDecision::NeedVerifyCode
    );
    machine.submit_code("123456").unwrap();
    let decision = machine.apply(confirmed_status()).unwrap();
    assert!(matches!(decision, LoginDecision::Confirmed(_)));
    assert_eq!(machine.state(), LoginSessionState::Paired);
    assert_eq!(machine.credentials().unwrap().bot_id(), "bot-1");
}

#[test]
fn scanned_redirect_changes_the_status_base_url() {
    let mut machine = LoginMachine::new("qr-code");
    let mut status = status("scaned_but_redirect");
    status.redirect_host = "redirect.example.test".into();

    assert_eq!(
        machine.apply(status).unwrap(),
        LoginDecision::Redirect {
            base_url: "https://redirect.example.test".into()
        }
    );
    assert_eq!(machine.base_url(), "https://redirect.example.test");
    assert_eq!(machine.state(), LoginSessionState::WaitingScan);
}

#[test]
fn expired_and_blocked_are_terminal_states() {
    let mut expired = LoginMachine::new("qr-code");
    assert_eq!(
        expired.apply(status("expired")).unwrap(),
        LoginDecision::Expired
    );
    assert_eq!(expired.state(), LoginSessionState::Expired);

    let mut blocked = LoginMachine::new("qr-code");
    assert_eq!(
        blocked.apply(status("verify_code_blocked")).unwrap(),
        LoginDecision::Blocked
    );
    assert_eq!(blocked.state(), LoginSessionState::Blocked);
}

#[test]
fn qr_data_url_is_generated_in_memory() {
    let data_url = qr_data_url("clawbot://login/test").unwrap();

    assert!(data_url.starts_with("data:image/svg+xml;base64,"));
    assert!(data_url.len() > 100);
}

#[tokio::test]
async fn login_adapter_passes_shared_contract() {
    let adapter = ClawBotLoginAdapter::new(
        Arc::new(WaitingTransport),
        Arc::new(TestSecrets::default()),
        Arc::new(TestAccounts::default()),
    );

    assert_channel_login_contract(Arc::new(adapter)).await;
}

#[tokio::test]
async fn confirmed_login_persists_account_and_bot_secret() {
    let secrets = Arc::new(TestSecrets::default());
    let accounts = Arc::new(TestAccounts::default());
    let adapter = ClawBotLoginAdapter::new(
        Arc::new(ConfirmingTransport),
        secrets.clone(),
        accounts.clone(),
    );
    let mut events = adapter.subscribe();

    let initial = adapter
        .begin_login(BeginLoginRequest::new("first-login"))
        .await
        .unwrap();
    let mut paired = None;
    for _ in 0..4 {
        let session = tokio::time::timeout(Duration::from_secs(1), events.recv())
            .await
            .expect("登录事件不能超时")
            .expect("登录事件通道不能关闭");
        if session.state() == LoginSessionState::WaitingFirstInbound {
            paired = Some(session);
            break;
        }
    }
    let paired = paired.expect("确认登录后必须进入等待首条入站消息");
    assert_eq!(paired.account_key(), "first-login");
    assert!(initial.id() == paired.id());

    let account = ClawBotAccount::from_platform_ids("bot-1", "user-1").unwrap();
    assert!(accounts.get(account.id()).await.unwrap().is_some());
    let secret = secrets
        .get(account.id(), SecretKind::BotToken)
        .await
        .unwrap();
    assert!(!secret.expose().contains("context-token"));
}

#[tokio::test]
async fn confirmed_login_updates_snapshot_through_read_model() {
    let adapter = ClawBotLoginAdapter::new(
        Arc::new(ConfirmingTransport),
        Arc::new(TestSecrets::default()),
        Arc::new(TestAccounts::default()),
    );
    let initial = adapter
        .begin_login(BeginLoginRequest::new("first-login"))
        .await
        .unwrap();

    let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
    loop {
        if let Some(session) = adapter.snapshot(initial.id()).await {
            if session.state() == LoginSessionState::WaitingFirstInbound {
                break;
            }
        }
        assert!(tokio::time::Instant::now() < deadline, "登录状态不能超时");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

fn status(value: &str) -> QrStatusResponse {
    QrStatusResponse {
        status: value.into(),
        ..QrStatusResponse::default()
    }
}

fn confirmed_status() -> QrStatusResponse {
    QrStatusResponse {
        status: "confirmed".into(),
        bot_token: "bot-token-value".into(),
        ilink_bot_id: "bot-1".into(),
        ilink_user_id: "user-1".into(),
        baseurl: "https://business.example.test".into(),
        ..QrStatusResponse::default()
    }
}

struct WaitingTransport;

#[async_trait]
impl ClawBotAuthTransport for WaitingTransport {
    async fn fetch_qr_code(
        &self,
        _base_url: &str,
        _local_tokens: &[String],
    ) -> Result<QrCodeResponse, ChannelError> {
        Ok(QrCodeResponse {
            qrcode: "qr-code".into(),
            ..QrCodeResponse::default()
        })
    }

    async fn poll_qr_status(
        &self,
        _base_url: &str,
        _qr_code: &str,
        _verify_code: Option<&str>,
    ) -> Result<QrStatusResponse, ChannelError> {
        Ok(status("wait"))
    }
}

struct ConfirmingTransport;

#[async_trait]
impl ClawBotAuthTransport for ConfirmingTransport {
    async fn fetch_qr_code(
        &self,
        _base_url: &str,
        _local_tokens: &[String],
    ) -> Result<QrCodeResponse, ChannelError> {
        Ok(QrCodeResponse {
            qrcode: "qr-code".into(),
            ..QrCodeResponse::default()
        })
    }

    async fn poll_qr_status(
        &self,
        _base_url: &str,
        _qr_code: &str,
        _verify_code: Option<&str>,
    ) -> Result<QrStatusResponse, ChannelError> {
        Ok(confirmed_status())
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
            .expect("密钥锁不应失败")
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
            .expect("密钥锁不应失败")
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
            .expect("密钥锁不应失败")
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
        Ok(self
            .values
            .lock()
            .expect("账号锁不应失败")
            .get(account_id)
            .cloned())
    }

    async fn list(&self, channel_id: &ChannelId) -> Result<Vec<ChannelAccount>, StoreError> {
        Ok(self
            .values
            .lock()
            .expect("账号锁不应失败")
            .values()
            .filter(|account| &account.channel_id == channel_id)
            .cloned()
            .collect())
    }

    async fn upsert(&self, account: ChannelAccount) -> Result<(), StoreError> {
        self.values
            .lock()
            .expect("账号锁不应失败")
            .insert(account.id.clone(), account);
        Ok(())
    }

    async fn set_enabled(
        &self,
        account_id: &ChannelAccountId,
        enabled: bool,
    ) -> Result<(), StoreError> {
        let mut values = self.values.lock().expect("账号锁不应失败");
        let account = values
            .get_mut(account_id)
            .ok_or_else(|| StoreError::not_found("account_missing", "找不到测试渠道账号"))?;
        account.enabled = enabled;
        Ok(())
    }
}
