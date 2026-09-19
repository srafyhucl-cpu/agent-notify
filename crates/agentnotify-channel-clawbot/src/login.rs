use std::{
    collections::HashMap,
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};

use agentnotify_application::{
    ChannelAccountStore, SecretError, SecretKind, SecretStore, StoreError,
};
use agentnotify_channel_sdk::{
    BeginLoginRequest, ChannelError, ChannelLoginAdapter, LoginSession, LoginSessionId,
    LoginSessionState,
};
use agentnotify_domain::{ChannelAccountId, Timestamp};
use tokio::sync::{broadcast, watch};

use crate::{
    account::ClawBotAccount,
    client::{ClawBotAuthTransport, QrStatusResponse},
    qr::qr_data_url,
    state::{ClawBotCredentials, DEFAULT_BASE_URL},
};

const INITIAL_EVENT_CAPACITY: usize = 64;
const POLL_INTERVAL: Duration = Duration::from_millis(200);
const RETRY_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LoginDecision {
    Continue,
    NeedVerifyCode,
    Confirmed(ClawBotCredentials),
    Redirect { base_url: String },
    Expired,
    Blocked,
}

/// 登录状态机只处理协议语义，不接触 HTTP、存储或 UI。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoginMachine {
    qr_code: String,
    base_url: String,
    state: LoginSessionState,
    verify_code: Option<String>,
    credentials: Option<ClawBotCredentials>,
}

impl LoginMachine {
    pub fn new(qr_code: impl Into<String>) -> Self {
        Self {
            qr_code: qr_code.into().trim().to_owned(),
            base_url: DEFAULT_BASE_URL.into(),
            state: LoginSessionState::QrReady,
            verify_code: None,
            credentials: None,
        }
    }

    pub fn apply(&mut self, status: QrStatusResponse) -> Result<LoginDecision, ChannelError> {
        validate_status_code(&status)?;
        let status_name = normalized_status(&status);

        match status_name.as_str() {
            "wait" | "scaned" => {
                self.verify_code = None;
                self.state = LoginSessionState::WaitingScan;
                Ok(LoginDecision::Continue)
            }
            "need_verifycode" => {
                self.state = LoginSessionState::NeedVerifyCode;
                Ok(LoginDecision::NeedVerifyCode)
            }
            "scaned_but_redirect" => {
                let base_url = normalize_redirect_host(&status.redirect_host)?;
                self.base_url = base_url.clone();
                self.state = LoginSessionState::WaitingScan;
                Ok(LoginDecision::Redirect { base_url })
            }
            "expired" => {
                self.verify_code = None;
                self.state = LoginSessionState::Expired;
                Ok(LoginDecision::Expired)
            }
            "verify_code_blocked" => {
                self.verify_code = None;
                self.state = LoginSessionState::Blocked;
                Ok(LoginDecision::Blocked)
            }
            "binded_redirect" => {
                // 适配器会在进入状态机前检查本机是否已有有效凭据。
                self.verify_code = None;
                self.state = LoginSessionState::Expired;
                Ok(LoginDecision::Expired)
            }
            "confirmed" => {
                let base_url = if status.baseurl.trim().is_empty() {
                    self.base_url.clone()
                } else {
                    status.baseurl.trim().to_owned()
                };
                let credentials = ClawBotCredentials::new(
                    status.bot_token,
                    status.ilink_bot_id,
                    status.ilink_user_id,
                    base_url,
                )?;
                self.verify_code = None;
                self.state = LoginSessionState::Paired;
                self.credentials = Some(credentials.clone());
                Ok(LoginDecision::Confirmed(credentials))
            }
            _ => Err(ChannelError::permanent(
                "clawbot_qr_status_invalid",
                "ClawBot 登录状态无法识别，请刷新二维码后重试",
            )),
        }
    }

    pub fn submit_code(&mut self, code: &str) -> Result<(), ChannelError> {
        let code = code.trim();
        if code.is_empty() || !code.chars().all(|character| character.is_ascii_digit()) {
            return Err(ChannelError::permanent(
                "clawbot_verify_code_invalid",
                "ClawBot 数字配对码只能包含数字",
            ));
        }
        if self.state != LoginSessionState::NeedVerifyCode {
            return Err(ChannelError::permanent(
                "clawbot_verify_code_not_requested",
                "当前登录步骤不需要数字配对码",
            ));
        }
        self.verify_code = Some(code.into());
        Ok(())
    }

    pub fn state(&self) -> LoginSessionState {
        self.state
    }

    pub fn credentials(&self) -> Option<&ClawBotCredentials> {
        self.credentials.as_ref()
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub(crate) fn qr_code(&self) -> &str {
        &self.qr_code
    }

    pub(crate) fn verify_code(&self) -> Option<&str> {
        self.verify_code.as_deref()
    }
}

#[derive(Clone)]
struct AdapterInner {
    transport: Arc<dyn ClawBotAuthTransport>,
    secrets: Arc<dyn SecretStore>,
    accounts: Arc<dyn ChannelAccountStore>,
    sessions: Arc<Mutex<HashMap<LoginSessionId, LoginSession>>>,
    machines: Arc<Mutex<HashMap<LoginSessionId, Arc<Mutex<LoginMachine>>>>>,
    cancels: Arc<Mutex<HashMap<LoginSessionId, watch::Sender<bool>>>>,
    events: broadcast::Sender<LoginSession>,
}

pub struct ClawBotLoginAdapter {
    inner: AdapterInner,
}

impl ClawBotLoginAdapter {
    pub fn new(
        transport: Arc<dyn ClawBotAuthTransport>,
        secrets: Arc<dyn SecretStore>,
        accounts: Arc<dyn ChannelAccountStore>,
    ) -> Self {
        let (events, _) = broadcast::channel(INITIAL_EVENT_CAPACITY);
        Self {
            inner: AdapterInner {
                transport,
                secrets,
                accounts,
                sessions: Arc::new(Mutex::new(HashMap::new())),
                machines: Arc::new(Mutex::new(HashMap::new())),
                cancels: Arc::new(Mutex::new(HashMap::new())),
                events,
            },
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<LoginSession> {
        self.inner.events.subscribe()
    }

    pub async fn snapshot(&self, session_id: &LoginSessionId) -> Option<LoginSession> {
        lock(&self.inner.sessions).get(session_id).cloned()
    }
}

#[async_trait::async_trait]
impl ChannelLoginAdapter for ClawBotLoginAdapter {
    async fn begin_login(&self, request: BeginLoginRequest) -> Result<LoginSession, ChannelError> {
        let local_tokens = local_tokens_for(
            self.inner.accounts.as_ref(),
            self.inner.secrets.as_ref(),
            &request.account_key,
        )
        .await;
        let response = self
            .inner
            .transport
            .fetch_qr_code(DEFAULT_BASE_URL, &local_tokens)
            .await?;
        let qr_payload = qr_data_url(response.display_content())?;
        let session_id = LoginSessionId::new(uuid::Uuid::new_v4().to_string())?;
        let session = LoginSession::new(
            session_id.clone(),
            request.account_key,
            LoginSessionState::QrReady,
            Timestamp::now_utc(),
        )?
        .with_qr_payload(qr_payload);
        let machine = Arc::new(Mutex::new(LoginMachine::new(response.qrcode)));
        let (cancel, cancel_receiver) = watch::channel(false);

        lock(&self.inner.sessions).insert(session_id.clone(), session.clone());
        lock(&self.inner.machines).insert(session_id.clone(), machine.clone());
        lock(&self.inner.cancels).insert(session_id.clone(), cancel);
        let _ = self.inner.events.send(session.clone());
        tokio::spawn(run_login_task(
            self.inner.clone(),
            session_id,
            machine,
            cancel_receiver,
        ));
        Ok(session)
    }

    async fn submit_login_code(
        &self,
        session_id: &LoginSessionId,
        code: &str,
    ) -> Result<LoginSession, ChannelError> {
        let code = code.trim();
        if code.is_empty() || !code.chars().all(|character| character.is_ascii_digit()) {
            return Err(ChannelError::permanent(
                "clawbot_verify_code_invalid",
                "ClawBot 数字配对码只能包含数字",
            ));
        }
        if !lock(&self.inner.sessions).contains_key(session_id) {
            return Err(ChannelError::permanent(
                "clawbot_login_session_missing",
                "ClawBot 登录会话已取消或过期，请重新扫码",
            ));
        }
        let machine = lock(&self.inner.machines)
            .get(session_id)
            .cloned()
            .ok_or_else(|| {
                ChannelError::permanent(
                    "clawbot_login_session_missing",
                    "ClawBot 登录会话已取消或过期，请重新扫码",
                )
            })?;
        lock(&machine).submit_code(code)?;
        lock(&self.inner.sessions)
            .get(session_id)
            .cloned()
            .ok_or_else(|| {
                ChannelError::permanent(
                    "clawbot_login_session_missing",
                    "ClawBot 登录会话已取消或过期，请重新扫码",
                )
            })
    }

    async fn cancel_login(&self, session_id: &LoginSessionId) -> Result<(), ChannelError> {
        lock(&self.inner.sessions).remove(session_id);
        lock(&self.inner.machines).remove(session_id);
        if let Some(cancel) = lock(&self.inner.cancels).remove(session_id) {
            let _ = cancel.send(true);
        }
        Ok(())
    }
}

async fn run_login_task(
    inner: AdapterInner,
    session_id: LoginSessionId,
    machine: Arc<Mutex<LoginMachine>>,
    mut cancel: watch::Receiver<bool>,
) {
    loop {
        if *cancel.borrow() {
            return;
        }

        let (qr_code, base_url, verify_code) = {
            let machine = lock(&machine);
            (
                machine.qr_code().to_owned(),
                machine.base_url().to_owned(),
                machine.verify_code().map(str::to_owned),
            )
        };

        let status = match inner
            .transport
            .poll_qr_status(&base_url, &qr_code, verify_code.as_deref())
            .await
        {
            Ok(status) => status,
            Err(ChannelError::Retryable { .. }) => {
                if wait_for_poll(&mut cancel, RETRY_INTERVAL).await {
                    return;
                }
                continue;
            }
            Err(ChannelError::Unknown(_)) => {
                if wait_for_poll(&mut cancel, RETRY_INTERVAL).await {
                    return;
                }
                continue;
            }
            Err(error) => {
                publish_state(&inner, &session_id, LoginSessionState::Failed);
                tracing::warn!(code = error.code(), "ClawBot 登录轮询失败");
                return;
            }
        };

        if is_binded_redirect(&status) {
            if let Ok(Some(credentials)) = load_credentials_for_session(&inner, &session_id).await {
                if persist_credentials(&inner, &credentials).await.is_ok() {
                    publish_state(&inner, &session_id, LoginSessionState::WaitingFirstInbound);
                    return;
                }
            }
        }

        let decision = {
            let mut machine = lock(&machine);
            match machine.apply(status) {
                Ok(decision) => decision,
                Err(error) => {
                    publish_state(&inner, &session_id, LoginSessionState::Failed);
                    tracing::warn!(code = error.code(), "ClawBot 登录状态无法处理");
                    return;
                }
            }
        };

        match decision {
            LoginDecision::Continue => {
                publish_state(&inner, &session_id, LoginSessionState::WaitingScan);
            }
            LoginDecision::NeedVerifyCode => {
                publish_state(&inner, &session_id, LoginSessionState::NeedVerifyCode);
            }
            LoginDecision::Redirect { .. } => {
                publish_state(&inner, &session_id, LoginSessionState::WaitingScan);
            }
            LoginDecision::Confirmed(credentials) => {
                if persist_credentials(&inner, &credentials).await.is_err() {
                    publish_state(&inner, &session_id, LoginSessionState::Failed);
                    return;
                }
                publish_state(&inner, &session_id, LoginSessionState::WaitingFirstInbound);
                return;
            }
            LoginDecision::Expired => {
                publish_state(&inner, &session_id, LoginSessionState::Expired);
                return;
            }
            LoginDecision::Blocked => {
                publish_state(&inner, &session_id, LoginSessionState::Blocked);
                return;
            }
        }

        if wait_for_poll(&mut cancel, POLL_INTERVAL).await {
            return;
        }
    }
}

async fn wait_for_poll(cancel: &mut watch::Receiver<bool>, duration: Duration) -> bool {
    tokio::select! {
        changed = cancel.changed() => changed.is_ok() && *cancel.borrow(),
        () = tokio::time::sleep(duration) => false,
    }
}

fn publish_state(inner: &AdapterInner, session_id: &LoginSessionId, state: LoginSessionState) {
    let session = {
        let mut sessions = lock(&inner.sessions);
        let Some(current) = sessions.get(session_id).cloned() else {
            return;
        };
        let next = replace_session_state(&current, state);
        sessions.insert(session_id.clone(), next.clone());
        next
    };
    let _ = inner.events.send(session);
}

fn replace_session_state(current: &LoginSession, state: LoginSessionState) -> LoginSession {
    let keep_qr = matches!(
        state,
        LoginSessionState::Preparing
            | LoginSessionState::QrReady
            | LoginSessionState::WaitingScan
            | LoginSessionState::NeedVerifyCode
    );
    let mut next = LoginSession::new(
        current.id().clone(),
        current.account_key(),
        state,
        current.created_at(),
    )
    .expect("现有登录会话字段保持有效");
    if keep_qr {
        if let Some(qr_payload) = current.qr_payload() {
            next = next.with_qr_payload(qr_payload);
        }
    }
    next
}

async fn persist_credentials(
    inner: &AdapterInner,
    credentials: &ClawBotCredentials,
) -> Result<(), ChannelError> {
    let account = ClawBotAccount::from_platform_ids(credentials.bot_id(), credentials.user_id())?
        .with_base_url(credentials.base_url(), Timestamp::now_utc())?;
    let account_id = account.id().clone();
    inner
        .secrets
        .set(&account_id, SecretKind::BotToken, credentials.to_secret()?)
        .await
        .map_err(map_secret_error)?;
    inner
        .accounts
        .upsert(account.into_channel_account()?)
        .await
        .map_err(map_store_error)?;
    Ok(())
}

async fn local_tokens_for(
    accounts: &dyn ChannelAccountStore,
    secrets: &dyn SecretStore,
    account_key: &str,
) -> Vec<String> {
    match load_credentials_for_key(accounts, secrets, account_key).await {
        Ok(Some(credentials)) => vec![credentials.bot_token],
        _ => Vec::new(),
    }
}

async fn load_credentials_for_session(
    inner: &AdapterInner,
    session_id: &LoginSessionId,
) -> Result<Option<ClawBotCredentials>, ChannelError> {
    let account_key = lock(&inner.sessions)
        .get(session_id)
        .map(|session| session.account_key().to_owned())
        .ok_or_else(|| {
            ChannelError::permanent(
                "clawbot_login_session_missing",
                "ClawBot 登录会话已取消或过期，请重新扫码",
            )
        })?;
    load_credentials_for_key(
        inner.accounts.as_ref(),
        inner.secrets.as_ref(),
        &account_key,
    )
    .await
}

async fn load_credentials_for_key(
    accounts: &dyn ChannelAccountStore,
    secrets: &dyn SecretStore,
    account_key: &str,
) -> Result<Option<ClawBotCredentials>, ChannelError> {
    let Ok(account_id) = ChannelAccountId::new(account_key) else {
        return Ok(None);
    };
    let Some(_) = accounts.get(&account_id).await.map_err(map_store_error)? else {
        return Ok(None);
    };
    match secrets.get(&account_id, SecretKind::BotToken).await {
        Ok(value) => ClawBotCredentials::from_secret(&value).map(Some),
        Err(error) if error.code() == "secret_not_found" => Ok(None),
        Err(error) => Err(map_secret_error(error)),
    }
}

fn normalized_status(status: &QrStatusResponse) -> String {
    if status.verify_code_blocked {
        return "verify_code_blocked".into();
    }
    if status.binded_redirect {
        return "binded_redirect".into();
    }
    if status.need_verifycode {
        return "need_verifycode".into();
    }
    let value = status.status.trim();
    if !value.is_empty() {
        return value.to_owned();
    }
    if !status.bot_token.trim().is_empty() {
        return "confirmed".into();
    }
    String::new()
}

fn is_binded_redirect(status: &QrStatusResponse) -> bool {
    status.binded_redirect || status.status.trim() == "binded_redirect"
}

fn validate_status_code(status: &QrStatusResponse) -> Result<(), ChannelError> {
    if status.ret == -14 || status.errcode == -14 {
        return Err(ChannelError::invalid_account(
            "clawbot_invalid_account",
            "ClawBot 登录状态已失效，请重新扫码",
        ));
    }
    if status.ret != 0 || status.errcode != 0 {
        return Err(ChannelError::retryable(
            "clawbot_api_error",
            "ClawBot 服务暂时未完成请求，请稍后重试",
            Some(time::Duration::seconds(1)),
        ));
    }
    Ok(())
}

fn normalize_redirect_host(host: &str) -> Result<String, ChannelError> {
    let host = host.trim().trim_end_matches('/');
    if host.is_empty() || host.chars().any(char::is_whitespace) {
        return Err(ChannelError::permanent(
            "clawbot_redirect_invalid",
            "ClawBot 登录跳转地址无效，请刷新二维码后重试",
        ));
    }
    if host.starts_with("http://") || host.starts_with("https://") {
        Ok(host.into())
    } else {
        Ok(format!("https://{host}"))
    }
}

fn map_secret_error(_error: SecretError) -> ChannelError {
    ChannelError::retryable(
        "clawbot_secret_store_failed",
        "ClawBot 登录凭据保存失败，请稍后重试",
        None,
    )
}

fn map_store_error(_error: StoreError) -> ChannelError {
    ChannelError::retryable(
        "clawbot_account_store_failed",
        "ClawBot 账号保存失败，请稍后重试",
        None,
    )
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
