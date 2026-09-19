use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

use agentnotify_channel_sdk::{
    BeginLoginRequest, ChannelError, ChannelLoginAdapter, LoginSession, LoginSessionId,
    LoginSessionState, assert_channel_login_contract,
};
use agentnotify_domain::Timestamp;

#[derive(Default)]
struct FakeLoginChannel {
    cancelled: Arc<Mutex<HashSet<LoginSessionId>>>,
}

#[async_trait::async_trait]
impl ChannelLoginAdapter for FakeLoginChannel {
    async fn begin_login(&self, request: BeginLoginRequest) -> Result<LoginSession, ChannelError> {
        LoginSession::new(
            LoginSessionId::new("login-1").unwrap(),
            request.account_key,
            LoginSessionState::QrReady,
            Timestamp::parse_rfc3339("2026-09-19T10:00:00Z").unwrap(),
        )
    }

    async fn submit_login_code(
        &self,
        session_id: &LoginSessionId,
        code: &str,
    ) -> Result<LoginSession, ChannelError> {
        if self.cancelled.lock().unwrap().contains(session_id) {
            return Err(ChannelError::invalid_account(
                "login_session_cancelled",
                "登录会话已取消",
            ));
        }
        if code.trim().is_empty() {
            return Err(ChannelError::permanent(
                "empty_verification_code",
                "验证码不能为空",
            ));
        }
        LoginSession::new(
            session_id.clone(),
            "account-a",
            LoginSessionState::Paired,
            Timestamp::parse_rfc3339("2026-09-19T10:00:00Z").unwrap(),
        )
    }

    async fn cancel_login(&self, _session_id: &LoginSessionId) -> Result<(), ChannelError> {
        self.cancelled.lock().unwrap().insert(_session_id.clone());
        Ok(())
    }
}

#[tokio::test]
async fn login_adapter_rejects_empty_verification_code() {
    let adapter = FakeLoginChannel::default();
    let session = adapter
        .begin_login(BeginLoginRequest::new("account-a"))
        .await
        .unwrap();
    let error = adapter
        .submit_login_code(session.id(), " ")
        .await
        .unwrap_err();
    assert!(matches!(error, ChannelError::Permanent(_)));
}

#[tokio::test]
async fn login_contract_requires_cancel_and_nonempty_code() {
    assert_channel_login_contract(Arc::new(FakeLoginChannel::default())).await;
}
