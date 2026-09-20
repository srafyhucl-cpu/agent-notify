use std::{sync::Arc, time::Duration};

use agentnotify_application::{
    ChannelAccountStore, SecretError, SecretKind, SecretStore, StoreError,
};
use agentnotify_channel_sdk::{ChannelAccount, ChannelError, InboundEmitter};
use agentnotify_domain::{ChannelAccountId, Timestamp};
use async_trait::async_trait;
use reqwest::header::{HeaderValue, InvalidHeaderValue};
use serde::Serialize;
use tokio::sync::watch;

use crate::{
    account::{ClawBotAccount, stable_account_id},
    inbound::{ClawBotInboundMessage, normalize_inbound},
    state::{ClawBotContext, ClawBotCredentials, ClawBotCursor},
};

const ENDPOINT_GET_UPDATES: &str = "/ilink/bot/getupdates";
const ENDPOINT_NOTIFY_START: &str = "/ilink/bot/msg/notifystart";
const ENDPOINT_NOTIFY_STOP: &str = "/ilink/bot/msg/notifystop";
const CHANNEL_VERSION: &str = "2.4.6";
const APP_ID: &str = "bot";
const APP_CLIENT_VERSION: &str = "132102";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const LONG_POLL_TIMEOUT: Duration = Duration::from_secs(35);
const LIFECYCLE_TIMEOUT: Duration = Duration::from_secs(10);
const INITIAL_RETRY_DELAY: Duration = Duration::from_secs(1);
const MAX_RETRY_DELAY: Duration = Duration::from_secs(60);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClawBotGetUpdatesRequest {
    pub base_url: String,
    pub bot_token: String,
    pub cursor: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClawBotLifecycleRequest {
    pub base_url: String,
    pub bot_token: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ClawBotUpdates {
    pub messages: Vec<ClawBotInboundMessage>,
    pub cursor: String,
}

/// 单轮入站处理结果：继续轮询表示游标已经持久化。
enum PollOutcome {
    Continue,
    Stop,
}

/// 长轮询与生命周期请求的可替换边界，便于无网络验证游标恢复逻辑。
#[async_trait]
pub trait ClawBotSessionTransport: Send + Sync {
    async fn get_updates(
        &self,
        request: ClawBotGetUpdatesRequest,
    ) -> Result<ClawBotUpdates, ChannelError>;

    async fn notify_start(&self, _request: ClawBotLifecycleRequest) -> Result<(), ChannelError> {
        Ok(())
    }

    async fn notify_stop(&self, _request: ClawBotLifecycleRequest) -> Result<(), ChannelError> {
        Ok(())
    }
}

#[derive(Clone)]
pub struct ClawBotHttpSessionTransport {
    client: reqwest::Client,
    long_poll_timeout: Duration,
    lifecycle_timeout: Duration,
}

impl ClawBotHttpSessionTransport {
    pub fn new() -> Self {
        Self::with_timeouts(LONG_POLL_TIMEOUT, LIFECYCLE_TIMEOUT)
    }

    pub fn with_timeouts(long_poll_timeout: Duration, lifecycle_timeout: Duration) -> Self {
        let client = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT.min(long_poll_timeout))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            client,
            long_poll_timeout,
            lifecycle_timeout,
        }
    }

    async fn post_json(
        &self,
        base_url: &str,
        bot_token: &str,
        endpoint: &str,
        body: &SessionRequestBody,
        timeout: Duration,
        operation: &str,
    ) -> Result<serde_json::Value, ChannelError> {
        let endpoint = format!("{}{endpoint}", normalize_base_url(base_url)?);
        let authorization = bearer_header(bot_token)?;
        let response = self
            .client
            .post(endpoint)
            .timeout(timeout)
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .header("AuthorizationType", "ilink_bot_token")
            .header("Authorization", authorization)
            .header("X-WECHAT-UIN", random_wechat_uin())
            .header("iLink-App-Id", APP_ID)
            .header("iLink-App-ClientVersion", APP_CLIENT_VERSION)
            .json(body)
            .send()
            .await
            .map_err(|_| transport_error(operation))?;
        let status = response.status().as_u16();
        if is_retryable_http_status(status) {
            return Err(retryable_http_error(operation));
        }
        if !(200..=299).contains(&status) {
            return Err(ChannelError::permanent(
                format!("clawbot_{operation}_http_rejected"),
                format!("ClawBot 请求被拒绝（HTTP {status}）"),
            ));
        }
        let body = response
            .bytes()
            .await
            .map_err(|_| transport_error(operation))?;
        serde_json::from_slice(&body).map_err(|_| {
            ChannelError::unknown(
                format!("clawbot_{operation}_invalid"),
                "ClawBot 返回了无法解析的响应",
            )
        })
    }
}

impl Default for ClawBotHttpSessionTransport {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ClawBotSessionTransport for ClawBotHttpSessionTransport {
    async fn get_updates(
        &self,
        request: ClawBotGetUpdatesRequest,
    ) -> Result<ClawBotUpdates, ChannelError> {
        let response = self
            .post_json(
                &request.base_url,
                &request.bot_token,
                ENDPOINT_GET_UPDATES,
                &SessionRequestBody::get_updates(&request.cursor),
                self.long_poll_timeout,
                "getupdates",
            )
            .await?;
        validate_api_status(&response, "getupdates")?;

        let cursor = response
            .get("get_updates_buf")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_owned();
        let cursor = if cursor.is_empty() {
            response
                .get("sync_buf")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .trim()
                .to_owned()
        } else {
            cursor
        };
        let messages = match response.get("msgs") {
            None | Some(serde_json::Value::Null) => Vec::new(),
            Some(value) => serde_json::from_value(value.clone()).map_err(|_| {
                ChannelError::unknown(
                    "clawbot_getupdates_invalid",
                    "ClawBot 返回的入站消息格式无效",
                )
            })?,
        };
        Ok(ClawBotUpdates { messages, cursor })
    }

    async fn notify_start(&self, request: ClawBotLifecycleRequest) -> Result<(), ChannelError> {
        self.notify(request, ENDPOINT_NOTIFY_START, "notifystart")
            .await
    }

    async fn notify_stop(&self, request: ClawBotLifecycleRequest) -> Result<(), ChannelError> {
        self.notify(request, ENDPOINT_NOTIFY_STOP, "notifystop")
            .await
    }
}

impl ClawBotHttpSessionTransport {
    async fn notify(
        &self,
        request: ClawBotLifecycleRequest,
        endpoint: &str,
        operation: &str,
    ) -> Result<(), ChannelError> {
        let response = self
            .post_json(
                &request.base_url,
                &request.bot_token,
                endpoint,
                &SessionRequestBody::lifecycle(),
                self.lifecycle_timeout,
                operation,
            )
            .await?;
        validate_api_status(&response, operation)
    }
}

#[derive(Serialize)]
struct SessionRequestBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    get_updates_buf: Option<String>,
    base_info: SessionBaseInfo,
}

impl SessionRequestBody {
    fn get_updates(cursor: &str) -> Self {
        Self {
            get_updates_buf: Some(cursor.trim().to_owned()),
            base_info: SessionBaseInfo::new(),
        }
    }

    fn lifecycle() -> Self {
        Self {
            get_updates_buf: None,
            base_info: SessionBaseInfo::new(),
        }
    }
}

#[derive(Serialize)]
struct SessionBaseInfo {
    channel_version: &'static str,
    bot_agent: String,
}

impl SessionBaseInfo {
    fn new() -> Self {
        Self {
            channel_version: CHANNEL_VERSION,
            bot_agent: format!("AgentNotify/{} (windows)", env!("CARGO_PKG_VERSION")),
        }
    }
}

pub(crate) async fn run_session_task(
    secrets: Arc<dyn SecretStore>,
    accounts: Arc<dyn ChannelAccountStore>,
    transport: Arc<dyn ClawBotSessionTransport>,
    account: ChannelAccount,
    emit: InboundEmitter,
    mut cancel: watch::Receiver<bool>,
) -> Result<(), ChannelError> {
    let account = ClawBotAccount::from_channel_account(account)?;
    if !account.channel_account().enabled {
        return Err(ChannelError::permanent(
            "clawbot_account_disabled",
            "ClawBot 账号已停用",
        ));
    }
    let account_id = account.id().clone();
    let credentials = load_credentials(secrets.as_ref(), &account_id).await?;
    let expected_account_id = stable_account_id(credentials.bot_id(), credentials.user_id())?;
    if expected_account_id != account_id {
        return Err(invalid_account("ClawBot 账号与登录凭据不匹配，请重新扫码"));
    }

    let lifecycle = ClawBotLifecycleRequest {
        base_url: credentials.base_url().to_owned(),
        bot_token: credentials.bot_token().to_owned(),
    };

    // notifystart 只在会话上下文就绪后发送：过早调用会被服务端拒绝，
    // 而失败不重试会导致主动推送无法在微信里展示。
    let result = run_poll_loop(
        secrets.clone(),
        accounts,
        transport.as_ref(),
        &account_id,
        &credentials,
        &lifecycle,
        account.cursor().get_updates_buf.clone(),
        emit,
        &mut cancel,
    )
    .await;

    if let Err(error) = transport.notify_stop(lifecycle).await {
        tracing::debug!(code = error.code(), "ClawBot notifystop 最佳努力失败");
    }
    result
}

#[allow(clippy::too_many_arguments)]
async fn run_poll_loop(
    secrets: Arc<dyn SecretStore>,
    accounts: Arc<dyn ChannelAccountStore>,
    transport: &dyn ClawBotSessionTransport,
    account_id: &ChannelAccountId,
    credentials: &ClawBotCredentials,
    lifecycle: &ClawBotLifecycleRequest,
    mut cursor: String,
    emit: InboundEmitter,
    cancel: &mut watch::Receiver<bool>,
) -> Result<(), ChannelError> {
    let mut retry_delay = INITIAL_RETRY_DELAY;
    let mut announced = false;
    loop {
        if is_cancelled(cancel) {
            return Ok(());
        }
        let request = ClawBotGetUpdatesRequest {
            base_url: credentials.base_url().to_owned(),
            bot_token: credentials.bot_token().to_owned(),
            cursor: cursor.clone(),
        };
        let updates = tokio::select! {
            changed = cancel.changed() => {
                if changed.is_err() || *cancel.borrow() {
                    return Ok(());
                }
                continue;
            }
            result = transport.get_updates(request) => result,
        };

        match updates {
            Ok(updates) => {
                let next_cursor = if updates.cursor.trim().is_empty() {
                    cursor.clone()
                } else {
                    updates.cursor.trim().to_owned()
                };
                match process_updates(
                    secrets.as_ref(),
                    accounts.as_ref(),
                    account_id,
                    credentials,
                    &next_cursor,
                    updates.messages,
                    &emit,
                )
                .await
                {
                    Ok(PollOutcome::Continue) => {
                        cursor = next_cursor;
                        retry_delay = INITIAL_RETRY_DELAY;
                        if !announced
                            && announce_session_start(
                                secrets.as_ref(),
                                transport,
                                account_id,
                                credentials.user_id(),
                                lifecycle,
                            )
                            .await
                        {
                            announced = true;
                        }
                    }
                    Ok(PollOutcome::Stop) => return Ok(()),
                    Err(error) if matches!(error, ChannelError::InvalidAccount(_)) => {
                        return Err(error);
                    }
                    Err(error) => {
                        tracing::warn!(code = error.code(), "ClawBot 入站状态保存失败");
                        if wait_for_retry(cancel, retry_delay).await {
                            return Ok(());
                        }
                        retry_delay = next_retry_delay(retry_delay);
                    }
                }
            }
            Err(ChannelError::InvalidAccount(_)) => {
                return invalidate_account(
                    secrets.as_ref(),
                    accounts.as_ref(),
                    account_id,
                    credentials.bot_token(),
                )
                .await;
            }
            Err(error) => {
                tracing::warn!(code = error.code(), "ClawBot 长轮询暂时失败");
                if wait_for_retry(cancel, retry_delay).await {
                    return Ok(());
                }
                retry_delay = next_retry_delay(retry_delay);
            }
        }
    }
}

async fn process_updates(
    secrets: &dyn SecretStore,
    accounts: &dyn ChannelAccountStore,
    account_id: &ChannelAccountId,
    credentials: &ClawBotCredentials,
    cursor: &str,
    messages: Vec<ClawBotInboundMessage>,
    emit: &InboundEmitter,
) -> Result<PollOutcome, ChannelError> {
    let now = Timestamp::now_utc();
    let mut next_context_token = None;
    for message in messages {
        if !message.is_private_from(credentials.user_id()) {
            continue;
        }
        let context_token = message.context_token.clone();
        let normalized =
            match normalize_inbound(account_id, credentials, message, Some(cursor), now) {
                Ok(message) => message,
                Err(error) => {
                    tracing::warn!(code = error.code(), "ClawBot 入站消息已忽略");
                    continue;
                }
            };
        if let Some(context_token) = context_token
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            next_context_token = Some(context_token.to_owned());
        }
        if emit.send(normalized).await.is_err() {
            return Ok(PollOutcome::Stop);
        }
    }

    persist_poll_state(
        secrets,
        accounts,
        account_id,
        credentials.bot_token(),
        credentials.user_id(),
        cursor,
        next_context_token.as_deref(),
        now,
    )
    .await?;
    Ok(PollOutcome::Continue)
}

/// 主动推送依赖入站会话上下文；上下文就绪后才发送 notifystart，失败则在后续轮询重试。
async fn announce_session_start(
    secrets: &dyn SecretStore,
    transport: &dyn ClawBotSessionTransport,
    account_id: &ChannelAccountId,
    bound_user_id: &str,
    lifecycle: &ClawBotLifecycleRequest,
) -> bool {
    match session_context_ready(secrets, account_id, bound_user_id).await {
        Ok(true) => {}
        Ok(false) => return false,
        Err(error) => {
            tracing::debug!(
                code = error.code(),
                "ClawBot 会话状态读取失败，暂缓 notifystart"
            );
            return false;
        }
    }
    match transport.notify_start(lifecycle.clone()).await {
        Ok(()) => true,
        Err(error) => {
            tracing::debug!(
                code = error.code(),
                "ClawBot notifystart 最佳努力失败，将在后续轮询重试"
            );
            false
        }
    }
}

async fn session_context_ready(
    secrets: &dyn SecretStore,
    account_id: &ChannelAccountId,
    bound_user_id: &str,
) -> Result<bool, ChannelError> {
    let secret = match secrets.get(account_id, SecretKind::ContextToken).await {
        Ok(secret) => secret,
        Err(error) if secret_not_found(&error) => return Ok(false),
        Err(error) => return Err(map_secret_error(error)),
    };
    match ClawBotContext::from_secret(&secret) {
        Ok(context) => Ok(context.user_id() == bound_user_id),
        Err(_) => Ok(false),
    }
}

#[allow(clippy::too_many_arguments)]
async fn persist_poll_state(
    secrets: &dyn SecretStore,
    accounts: &dyn ChannelAccountStore,
    account_id: &ChannelAccountId,
    used_bot_token: &str,
    bound_user_id: &str,
    cursor: &str,
    context_token: Option<&str>,
    now: Timestamp,
) -> Result<(), ChannelError> {
    if cursor.trim().is_empty() && context_token.is_none() {
        return Ok(());
    }
    if current_credentials_changed(secrets, account_id, used_bot_token).await? {
        return Ok(());
    }
    if let Some(context_token) = context_token {
        let context = ClawBotContext::new(context_token, bound_user_id)?;
        secrets
            .set(account_id, SecretKind::ContextToken, context.to_secret()?)
            .await
            .map_err(map_secret_error)?;
    }

    let Some(mut account) = accounts.get(account_id).await.map_err(map_store_error)? else {
        return Ok(());
    };
    let parsed = ClawBotAccount::from_channel_account(account.clone())?;
    let mut state = parsed.state().clone();
    if context_token.is_some() {
        state.stale_at = None;
        state.session_established_at = Some(now);
    }
    account.config = serde_json::to_value(state)
        .map_err(|_| permanent_state_error("ClawBot 账号状态编码失败"))?;
    if !cursor.trim().is_empty() {
        account.cursor = serde_json::to_value(ClawBotCursor::new(cursor.trim()))
            .map_err(|_| permanent_state_error("ClawBot 账号游标编码失败"))?;
    }
    account.updated_at = now;
    accounts.upsert(account).await.map_err(map_store_error)
}

async fn invalidate_account(
    secrets: &dyn SecretStore,
    accounts: &dyn ChannelAccountStore,
    account_id: &ChannelAccountId,
    used_bot_token: &str,
) -> Result<(), ChannelError> {
    let stale_result = mark_account_stale(secrets, accounts, account_id, used_bot_token).await;
    let context_result = clear_context_if_matches(secrets, account_id, None).await;
    stale_result?;
    context_result?;
    Err(invalid_account("ClawBot 登录状态已失效，请重新扫码"))
}

async fn mark_account_stale(
    secrets: &dyn SecretStore,
    accounts: &dyn ChannelAccountStore,
    account_id: &ChannelAccountId,
    used_bot_token: &str,
) -> Result<(), ChannelError> {
    if current_credentials_changed(secrets, account_id, used_bot_token).await? {
        return Ok(());
    }
    let Some(mut account) = accounts.get(account_id).await.map_err(map_store_error)? else {
        return Ok(());
    };
    let parsed = ClawBotAccount::from_channel_account(account.clone())?;
    let mut state = parsed.state().clone();
    state.stale_at = Some(Timestamp::now_utc());
    account.config = serde_json::to_value(state)
        .map_err(|_| permanent_state_error("ClawBot 账号状态编码失败"))?;
    account.cursor = serde_json::to_value(ClawBotCursor::default())
        .map_err(|_| permanent_state_error("ClawBot 账号游标编码失败"))?;
    account.updated_at = Timestamp::now_utc();
    accounts.upsert(account).await.map_err(map_store_error)
}

async fn clear_context_if_matches(
    secrets: &dyn SecretStore,
    account_id: &ChannelAccountId,
    expected_context_token: Option<&str>,
) -> Result<(), ChannelError> {
    if let Some(expected_context_token) = expected_context_token {
        let secret = match secrets.get(account_id, SecretKind::ContextToken).await {
            Ok(secret) => secret,
            Err(error) if secret_not_found(&error) => return Ok(()),
            Err(error) => return Err(map_secret_error(error)),
        };
        let matches = ClawBotContext::from_secret(&secret)
            .map(|context| context.context_token() == expected_context_token)
            .unwrap_or(true);
        if !matches {
            return Ok(());
        }
    }
    match secrets.delete(account_id, SecretKind::ContextToken).await {
        Ok(()) => Ok(()),
        Err(error) if secret_not_found(&error) => Ok(()),
        Err(error) => Err(map_secret_error(error)),
    }
}

async fn current_credentials_changed(
    secrets: &dyn SecretStore,
    account_id: &ChannelAccountId,
    used_bot_token: &str,
) -> Result<bool, ChannelError> {
    let secret = match secrets.get(account_id, SecretKind::BotToken).await {
        Ok(secret) => secret,
        Err(error) if secret_not_found(&error) => return Ok(false),
        Err(error) => return Err(map_secret_error(error)),
    };
    let current = ClawBotCredentials::from_secret(&secret)?;
    Ok(current.bot_token() != used_bot_token)
}

async fn load_credentials(
    secrets: &dyn SecretStore,
    account_id: &ChannelAccountId,
) -> Result<ClawBotCredentials, ChannelError> {
    let secret = match secrets.get(account_id, SecretKind::BotToken).await {
        Ok(secret) => secret,
        Err(error) if secret_not_found(&error) => {
            return Err(invalid_account("ClawBot 登录凭据缺失，请重新扫码"));
        }
        Err(error) => return Err(map_secret_error(error)),
    };
    ClawBotCredentials::from_secret(&secret)
}

fn validate_api_status(response: &serde_json::Value, operation: &str) -> Result<(), ChannelError> {
    let ret = json_i32(response, "ret")?.unwrap_or(0);
    let errcode = json_i32(response, "errcode")?.unwrap_or(0);
    if ret == -14 || errcode == -14 {
        return Err(invalid_account("ClawBot 登录状态已失效，请重新扫码"));
    }
    if ret == 0 && errcode == 0 {
        return Ok(());
    }
    Err(ChannelError::retryable(
        format!("clawbot_{operation}_retryable"),
        "ClawBot 服务暂时未完成请求，请稍后重试",
        Some(time::Duration::seconds(1)),
    ))
}

fn json_i32(value: &serde_json::Value, field: &str) -> Result<Option<i32>, ChannelError> {
    let Some(value) = value.get(field) else {
        return Ok(None);
    };
    if let Some(value) = value.as_i64() {
        return i32::try_from(value).map(Some).map_err(|_| {
            ChannelError::unknown(
                "clawbot_response_invalid",
                format!("ClawBot 返回的 {field} 超出范围"),
            )
        });
    }
    if let Some(value) = value.as_str() {
        return value.trim().parse::<i32>().map(Some).map_err(|_| {
            ChannelError::unknown(
                "clawbot_response_invalid",
                format!("ClawBot 返回的 {field} 格式无效"),
            )
        });
    }
    Err(ChannelError::unknown(
        "clawbot_response_invalid",
        format!("ClawBot 返回的 {field} 格式无效"),
    ))
}

async fn wait_for_retry(cancel: &mut watch::Receiver<bool>, delay: Duration) -> bool {
    loop {
        tokio::select! {
            changed = cancel.changed() => {
                if changed.is_err() || *cancel.borrow() {
                    return true;
                }
            }
            () = tokio::time::sleep(delay) => return false,
        }
    }
}

fn next_retry_delay(current: Duration) -> Duration {
    (current * 2).min(MAX_RETRY_DELAY)
}

fn is_cancelled(cancel: &watch::Receiver<bool>) -> bool {
    *cancel.borrow()
}

fn normalize_base_url(base_url: &str) -> Result<String, ChannelError> {
    let base_url = base_url.trim().trim_end_matches('/');
    if base_url.is_empty() || base_url.chars().any(char::is_whitespace) {
        return Err(ChannelError::permanent(
            "clawbot_base_url_invalid",
            "ClawBot 服务地址格式无效",
        ));
    }
    Ok(base_url.into())
}

fn bearer_header(bot_token: &str) -> Result<HeaderValue, ChannelError> {
    HeaderValue::from_str(&format!("Bearer {bot_token}")).map_err(map_invalid_header)
}

fn map_invalid_header(_: InvalidHeaderValue) -> ChannelError {
    invalid_account("ClawBot 登录凭据格式无效，请重新扫码")
}

fn transport_error(operation: &str) -> ChannelError {
    ChannelError::unknown(
        format!("clawbot_{operation}_unknown"),
        "ClawBot 网络请求中断，稍后会继续尝试",
    )
}

fn retryable_http_error(operation: &str) -> ChannelError {
    ChannelError::retryable(
        format!("clawbot_{operation}_http_retryable"),
        "ClawBot 服务暂时不可用，请稍后重试",
        Some(time::Duration::seconds(1)),
    )
}

fn is_retryable_http_status(status: u16) -> bool {
    status == 408 || status == 429 || (500..=599).contains(&status)
}

fn invalid_account(message: &str) -> ChannelError {
    ChannelError::invalid_account("clawbot_invalid_account", message)
}

fn map_secret_error(_: SecretError) -> ChannelError {
    ChannelError::retryable(
        "clawbot_secret_store_failed",
        "ClawBot 凭据读取失败，请稍后重试",
        None,
    )
}

fn map_store_error(_: StoreError) -> ChannelError {
    ChannelError::retryable(
        "clawbot_account_store_failed",
        "ClawBot 账号状态保存失败，请稍后重试",
        None,
    )
}

fn permanent_state_error(message: &str) -> ChannelError {
    ChannelError::permanent("clawbot_account_state_invalid", message)
}

fn secret_not_found(error: &SecretError) -> bool {
    error.code() == "secret_not_found"
}

fn random_wechat_uin() -> String {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    let value = uuid::Uuid::new_v4().as_u128() as u32;
    STANDARD.encode(value.to_string().as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retryable_http_status_matches_reference_client() {
        for status in [408, 429, 500, 599] {
            assert!(is_retryable_http_status(status), "status={status}");
        }
        for status in [400, 401, 404, 413] {
            assert!(!is_retryable_http_status(status), "status={status}");
        }
    }

    #[test]
    fn api_status_ret_minus_14_is_invalid_account() {
        let error =
            validate_api_status(&serde_json::json!({ "ret": -14 }), "getupdates").unwrap_err();
        assert!(matches!(error, ChannelError::InvalidAccount(_)));
    }
}
