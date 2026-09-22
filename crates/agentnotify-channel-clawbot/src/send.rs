use std::{collections::BTreeMap, time::Duration};

use agentnotify_application::{ChannelAccountStore, SecretError, SecretKind, SecretStore};
use agentnotify_channel_sdk::{ChannelAccount, ChannelError, DeliveryReceipt, OutboundMessage};
use agentnotify_domain::{DeliveryState, ExternalMessageId, SafeError, Timestamp};
use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use reqwest::header::{HeaderValue, InvalidHeaderValue};
use serde::Serialize;

use crate::{
    account::{ClawBotAccount, stable_account_id},
    render::{NotificationRenderInput, render_notification},
    response::parse_response_ids,
    state::{ClawBotContext, ClawBotCredentials, ClawBotCursor},
};

const ENDPOINT_SEND_MESSAGE: &str = "/ilink/bot/sendmessage";
const CHANNEL_VERSION: &str = "2.4.6";
const APP_ID: &str = "bot";
const APP_CLIENT_VERSION: &str = "132102";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const RETRY_AFTER: time::Duration = time::Duration::seconds(1);
const MESSAGE_TYPE_BOT: i32 = 2;
const MESSAGE_STATE_FINISH: i32 = 2;
const ITEM_TYPE_TEXT: i32 = 1;
const RETRYABLE_ERROR_MARKERS: [&str; 10] = [
    "temporar",
    "try again",
    "retry",
    "busy",
    "timeout",
    "rate limit",
    "too many",
    "稍后",
    "重试",
    "繁忙",
];

/// 可替换的 ClawBot 出站 HTTP 边界。实现不得记录 token 或消息正文。
#[async_trait]
pub trait ClawBotSendTransport: Send + Sync {
    async fn send_message(
        &self,
        request: ClawBotSendRequest,
    ) -> Result<ClawBotHttpResponse, ChannelError>;
}

/// 单次出站请求；`Debug` 被刻意省略，避免凭据进入日志。
pub struct ClawBotSendRequest {
    base_url: String,
    bot_token: String,
    body: Vec<u8>,
}

impl ClawBotSendRequest {
    pub fn body(&self) -> &[u8] {
        &self.body
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn client_id(&self) -> Option<String> {
        json_string_field(&self.body, "client_id")
    }

    pub fn to_user_id(&self) -> Option<String> {
        json_string_field(&self.body, "to_user_id")
    }

    pub fn text(&self) -> Option<String> {
        let value = serde_json::from_slice::<serde_json::Value>(&self.body).ok()?;
        value
            .get("msg")?
            .get("item_list")?
            .as_array()?
            .first()?
            .get("text_item")?
            .get("text")?
            .as_str()
            .map(str::to_owned)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClawBotHttpResponse {
    status: u16,
    body: Vec<u8>,
}

impl ClawBotHttpResponse {
    pub fn new(status: u16, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status,
            body: body.into(),
        }
    }

    pub fn status(&self) -> u16 {
        self.status
    }

    pub fn body(&self) -> &[u8] {
        &self.body
    }
}

/// 使用 reqwest + rustls 的默认出站实现。
#[derive(Clone)]
pub struct ClawBotHttpSendTransport {
    client: reqwest::Client,
    timeout: Duration,
}

impl ClawBotHttpSendTransport {
    pub fn new() -> Self {
        Self::with_timeout(REQUEST_TIMEOUT)
    }

    pub fn with_timeout(timeout: Duration) -> Self {
        let client = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT.min(timeout))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self { client, timeout }
    }
}

impl Default for ClawBotHttpSendTransport {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ClawBotSendTransport for ClawBotHttpSendTransport {
    async fn send_message(
        &self,
        request: ClawBotSendRequest,
    ) -> Result<ClawBotHttpResponse, ChannelError> {
        let ClawBotSendRequest {
            base_url,
            bot_token,
            body,
        } = request;
        let endpoint = format!(
            "{}{ENDPOINT_SEND_MESSAGE}",
            normalize_send_base_url(&base_url)?
        );
        let authorization = bearer_header(&bot_token)?;
        let response = self
            .client
            .post(endpoint)
            .timeout(self.timeout)
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .header("AuthorizationType", "ilink_bot_token")
            .header("Authorization", authorization)
            .header("X-WECHAT-UIN", random_wechat_uin())
            .header("iLink-App-Id", APP_ID)
            .header("iLink-App-ClientVersion", APP_CLIENT_VERSION)
            .body(body)
            .send()
            .await
            .map_err(map_send_transport_error)?;
        let status = response.status().as_u16();
        if is_retryable_http_status(status) {
            return Err(retryable_http_error());
        }
        if !(200..=299).contains(&status) {
            return Err(ChannelError::permanent(
                "clawbot_http_rejected",
                format!("ClawBot 请求被拒绝（HTTP {status}）"),
            ));
        }
        let body = response.bytes().await.map_err(|_| {
            ChannelError::unknown(
                "clawbot_send_result_unknown",
                "ClawBot 响应读取中断，无法确认投递结果",
            )
        })?;
        Ok(ClawBotHttpResponse::new(status, body.to_vec()))
    }
}

pub(crate) async fn send_outbound(
    secrets: &dyn SecretStore,
    accounts: Option<&dyn ChannelAccountStore>,
    transport: &dyn ClawBotSendTransport,
    account: ChannelAccount,
    message: OutboundMessage,
) -> Result<DeliveryReceipt, ChannelError> {
    let account = ClawBotAccount::from_channel_account(account)?;
    if !account.channel_account().enabled {
        return Err(ChannelError::permanent(
            "clawbot_account_disabled",
            "ClawBot 账号已停用",
        ));
    }
    let account_id = account.id().clone();
    let credentials = load_credentials(secrets, &account_id).await?;
    let expected_account_id = stable_account_id(credentials.bot_id(), credentials.user_id())?;
    if expected_account_id != account_id {
        return Err(invalid_account("ClawBot 账号与登录凭据不匹配，请重新扫码"));
    }

    let Some(context) = load_context(secrets, &account_id).await? else {
        return Ok(skipped_session_missing());
    };
    if context.user_id() != credentials.user_id() {
        return Ok(skipped_session_missing());
    }

    let client_id = message.client_id.trim().to_owned();
    let text = outbound_text(&message)?;
    let body = build_request_body(&credentials, &context, &client_id, &text)?;
    let response = transport
        .send_message(ClawBotSendRequest {
            base_url: credentials
                .base_url()
                .trim()
                .trim_end_matches('/')
                .to_owned(),
            bot_token: credentials.bot_token().to_owned(),
            body,
        })
        .await?;

    if is_retryable_http_status(response.status) {
        return Err(retryable_http_error());
    }
    if !(200..=299).contains(&response.status) {
        return Err(ChannelError::permanent(
            "clawbot_http_rejected",
            format!("ClawBot 请求被拒绝（HTTP {}）", response.status),
        ));
    }

    let status = parse_api_status(&response.body)?;
    match status {
        ApiStatus::Success => receipt_from_success(&response.body, &client_id),
        ApiStatus::InvalidAccount => {
            clear_context_if_matches(secrets, &account_id, context.context_token()).await?;
            mark_account_stale(secrets, accounts, &account_id, credentials.bot_token()).await?;
            Err(invalid_account("ClawBot 登录状态已失效，请重新扫码"))
        }
        ApiStatus::PrepareFailed => {
            clear_context_if_matches(secrets, &account_id, context.context_token()).await?;
            Ok(skipped_session_missing())
        }
        ApiStatus::Retryable => Err(retryable_server_error()),
        ApiStatus::Permanent => Err(ChannelError::permanent(
            "clawbot_send_rejected",
            "ClawBot 拒绝了消息，请检查消息内容或账号状态",
        )),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ApiStatus {
    Success,
    InvalidAccount,
    PrepareFailed,
    Retryable,
    Permanent,
}

fn parse_api_status(body: &[u8]) -> Result<ApiStatus, ChannelError> {
    let value = serde_json::from_slice::<serde_json::Value>(body).map_err(|_| {
        ChannelError::unknown(
            "clawbot_send_result_unknown",
            "ClawBot 返回了无法解析的发送结果",
        )
    })?;
    let ret = json_i32(&value, "ret")?.unwrap_or(0);
    let errcode = json_i32(&value, "errcode")?.unwrap_or(0);
    let errmsg = value
        .get("errmsg")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();

    if ret == -14 || errcode == -14 {
        return Ok(ApiStatus::InvalidAccount);
    }
    if (ret == -2 || errcode == -2) && errmsg.contains("prepare failed") {
        return Ok(ApiStatus::PrepareFailed);
    }
    if ret == 0 && errcode == 0 {
        return Ok(ApiStatus::Success);
    }
    if RETRYABLE_ERROR_MARKERS
        .iter()
        .any(|marker| errmsg.contains(marker))
    {
        return Ok(ApiStatus::Retryable);
    }
    Ok(ApiStatus::Permanent)
}

fn receipt_from_success(body: &[u8], client_id: &str) -> Result<DeliveryReceipt, ChannelError> {
    let ids = match parse_response_ids(body) {
        Ok(ids) => ids,
        Err(ChannelError::Unknown(error)) => {
            return Ok(DeliveryReceipt {
                external_message_id: None,
                external_thread_id: None,
                state: DeliveryState::Unknown,
                error: Some(error),
                raw_safe_metadata: BTreeMap::new(),
            });
        }
        Err(error) => return Err(error),
    };
    let Some(message_id) = ids.message_id else {
        let mut metadata = BTreeMap::new();
        metadata.insert("client_id".into(), client_id.into());
        return Ok(DeliveryReceipt {
            external_message_id: None,
            external_thread_id: None,
            state: DeliveryState::Unknown,
            error: Some(safe_error(
                "clawbot_message_id_missing",
                "ClawBot 未返回稳定消息 ID，无法建立回复路由",
            )),
            raw_safe_metadata: metadata,
        });
    };
    let message_id = ExternalMessageId::new(message_id).map_err(|_| {
        ChannelError::unknown(
            "clawbot_message_id_invalid",
            "ClawBot 返回的消息 ID 无效，无法确认投递结果",
        )
    })?;
    Ok(DeliveryReceipt::sent(message_id))
}

async fn load_credentials(
    secrets: &dyn SecretStore,
    account_id: &agentnotify_domain::ChannelAccountId,
) -> Result<ClawBotCredentials, ChannelError> {
    let secret = match secrets.get(account_id, SecretKind::BotToken).await {
        Ok(secret) => secret,
        Err(error) if secret_not_found(&error) => {
            return Err(invalid_account("ClawBot 登录凭据缺失，请重新扫码"));
        }
        Err(_) => return Err(secret_store_retryable()),
    };
    ClawBotCredentials::from_secret(&secret)
        .map_err(|_| invalid_account("ClawBot 登录凭据损坏，请重新扫码"))
}

async fn load_context(
    secrets: &dyn SecretStore,
    account_id: &agentnotify_domain::ChannelAccountId,
) -> Result<Option<ClawBotContext>, ChannelError> {
    let secret = match secrets.get(account_id, SecretKind::ContextToken).await {
        Ok(secret) => secret,
        Err(error) if secret_not_found(&error) => return Ok(None),
        Err(_) => return Err(secret_store_retryable()),
    };
    match ClawBotContext::from_secret(&secret) {
        Ok(context) => Ok(Some(context)),
        Err(_) => Ok(None),
    }
}

async fn clear_context_if_matches(
    secrets: &dyn SecretStore,
    account_id: &agentnotify_domain::ChannelAccountId,
    expected_context_token: &str,
) -> Result<(), ChannelError> {
    let secret = match secrets.get(account_id, SecretKind::ContextToken).await {
        Ok(secret) => secret,
        Err(error) if secret_not_found(&error) => return Ok(()),
        Err(_) => return Err(secret_store_retryable()),
    };
    let should_delete = ClawBotContext::from_secret(&secret)
        .map(|context| context.context_token() == expected_context_token)
        .unwrap_or(true);
    if !should_delete {
        return Ok(());
    }
    match secrets.delete(account_id, SecretKind::ContextToken).await {
        Ok(()) => Ok(()),
        Err(error) if secret_not_found(&error) => Ok(()),
        Err(_) => Err(secret_store_retryable()),
    }
}

async fn mark_account_stale(
    secrets: &dyn SecretStore,
    accounts: Option<&dyn ChannelAccountStore>,
    account_id: &agentnotify_domain::ChannelAccountId,
    used_bot_token: &str,
) -> Result<(), ChannelError> {
    let Some(accounts) = accounts else {
        return Ok(());
    };
    let Some(current) = accounts.get(account_id).await.map_err(store_retryable)? else {
        return Ok(());
    };
    if current_credentials_changed(secrets, account_id, used_bot_token).await? {
        return Ok(());
    }

    let parsed = ClawBotAccount::from_channel_account(current.clone())?;
    let mut state = parsed.state().clone();
    state.stale_at = Some(Timestamp::now_utc());
    let mut updated = current;
    updated.config = serde_json::to_value(state)
        .map_err(|_| permanent_account_state_error("ClawBot 账号状态编码失败"))?;
    updated.cursor = serde_json::to_value(ClawBotCursor::default())
        .map_err(|_| permanent_account_state_error("ClawBot 账号游标编码失败"))?;
    updated.updated_at = Timestamp::now_utc();
    accounts.upsert(updated).await.map_err(store_retryable)
}

async fn current_credentials_changed(
    secrets: &dyn SecretStore,
    account_id: &agentnotify_domain::ChannelAccountId,
    used_bot_token: &str,
) -> Result<bool, ChannelError> {
    let secret = match secrets.get(account_id, SecretKind::BotToken).await {
        Ok(secret) => secret,
        Err(error) if secret_not_found(&error) => return Ok(false),
        Err(_) => return Err(secret_store_retryable()),
    };
    let current = ClawBotCredentials::from_secret(&secret)
        .map_err(|_| invalid_account("ClawBot 登录凭据损坏，请重新扫码"))?;
    Ok(current.bot_token() != used_bot_token)
}

/// 有结构化通知信息时用渠道渲染器还原标题栏与页脚；否则按原始文本发送（向后兼容）。
fn outbound_text(message: &OutboundMessage) -> Result<String, ChannelError> {
    let Some(presentation) = message.notification.as_ref() else {
        return Ok(message.text.clone());
    };
    render_notification(NotificationRenderInput {
        agent_display_name: presentation.agent_display_name.clone(),
        session_name: presentation.session_name.clone(),
        body: message.text.clone(),
        occurred_at: presentation.occurred_at,
        include_footer: presentation.include_footer,
        replyable: presentation.replyable,
    })
}

fn build_request_body(
    credentials: &ClawBotCredentials,
    context: &ClawBotContext,
    client_id: &str,
    text: &str,
) -> Result<Vec<u8>, ChannelError> {
    let payload = SendMessageRequestBody {
        msg: SendMessage {
            from_user_id: String::new(),
            to_user_id: credentials.user_id().to_owned(),
            client_id: client_id.to_owned(),
            message_type: MESSAGE_TYPE_BOT,
            message_state: MESSAGE_STATE_FINISH,
            item_list: vec![MessageItem {
                item_type: ITEM_TYPE_TEXT,
                text_item: TextItem { text: text.into() },
            }],
            context_token: context.context_token().to_owned(),
        },
        base_info: BaseInfo {
            channel_version: CHANNEL_VERSION,
            bot_agent: format!("AgentNotify/{} (windows)", env!("CARGO_PKG_VERSION")),
        },
    };
    serde_json::to_vec(&payload).map_err(|_| {
        ChannelError::permanent("clawbot_request_encode_failed", "ClawBot 发送请求编码失败")
    })
}

#[derive(Serialize)]
struct SendMessageRequestBody {
    msg: SendMessage,
    base_info: BaseInfo,
}

#[derive(Serialize)]
struct SendMessage {
    from_user_id: String,
    to_user_id: String,
    client_id: String,
    message_type: i32,
    message_state: i32,
    item_list: Vec<MessageItem>,
    context_token: String,
}

#[derive(Serialize)]
struct MessageItem {
    #[serde(rename = "type")]
    item_type: i32,
    text_item: TextItem,
}

#[derive(Serialize)]
struct TextItem {
    text: String,
}

#[derive(Serialize)]
struct BaseInfo {
    channel_version: &'static str,
    bot_agent: String,
}

fn normalize_send_base_url(base_url: &str) -> Result<String, ChannelError> {
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

fn map_send_transport_error(_: reqwest::Error) -> ChannelError {
    ChannelError::unknown(
        "clawbot_send_result_unknown",
        "ClawBot 网络请求中断，无法确认投递结果",
    )
}

fn retryable_http_error() -> ChannelError {
    ChannelError::retryable(
        "clawbot_http_retryable",
        "ClawBot 服务暂时不可用，请稍后重试",
        Some(RETRY_AFTER),
    )
}

fn retryable_server_error() -> ChannelError {
    ChannelError::retryable(
        "clawbot_send_retryable",
        "ClawBot 暂时未完成发送，请稍后重试",
        Some(RETRY_AFTER),
    )
}

fn skipped_session_missing() -> DeliveryReceipt {
    DeliveryReceipt::skipped(safe_error(
        "session_missing",
        "ClawBot 主动推送会话已失效，请先给 ClawBot 发送一条消息",
    ))
}

fn invalid_account(message: &str) -> ChannelError {
    ChannelError::invalid_account("clawbot_invalid_account", message)
}

fn secret_not_found(error: &SecretError) -> bool {
    error.code() == "secret_not_found"
}

fn secret_store_retryable() -> ChannelError {
    ChannelError::retryable(
        "clawbot_secret_store_failed",
        "ClawBot 凭据读取失败，请稍后重试",
        None,
    )
}

fn store_retryable(_: agentnotify_application::StoreError) -> ChannelError {
    ChannelError::retryable(
        "clawbot_account_store_failed",
        "ClawBot 账号状态保存失败，请稍后重试",
        None,
    )
}

fn permanent_account_state_error(message: &str) -> ChannelError {
    ChannelError::permanent("clawbot_account_state_invalid", message)
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

fn json_string_field(body: &[u8], field: &str) -> Option<String> {
    let value = serde_json::from_slice::<serde_json::Value>(body).ok()?;
    value
        .get("msg")
        .and_then(|message| message.get(field))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
}

fn is_retryable_http_status(status: u16) -> bool {
    status == 408 || status == 429 || (500..=599).contains(&status)
}

fn random_wechat_uin() -> String {
    let value = uuid::Uuid::new_v4().as_u128() as u32;
    STANDARD.encode(value.to_string().as_bytes())
}

fn safe_error(code: &str, message: &str) -> SafeError {
    SafeError::new(code, message).expect("ClawBot 安全错误常量必须有效")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_accessors_read_standard_message_fields() {
        let credentials = ClawBotCredentials::new(
            "bot-secret",
            "bot-1",
            "user-1",
            "https://business.example.test",
        )
        .unwrap();
        let context = ClawBotContext::new("context-secret", "user-1").unwrap();
        let body =
            build_request_body(&credentials, &context, "client-1", "hello **world**").unwrap();
        let request = ClawBotSendRequest {
            base_url: credentials.base_url().into(),
            bot_token: credentials.bot_token().into(),
            body,
        };

        assert_eq!(request.client_id().as_deref(), Some("client-1"));
        assert_eq!(request.to_user_id().as_deref(), Some("user-1"));
        assert_eq!(request.text().as_deref(), Some("hello **world**"));
    }

    #[test]
    fn http_status_classification_matches_reference_client() {
        for status in [408, 429, 500, 503, 599] {
            assert!(is_retryable_http_status(status), "status={status}");
        }
        for status in [400, 401, 404, 413] {
            assert!(!is_retryable_http_status(status), "status={status}");
        }
    }
}
