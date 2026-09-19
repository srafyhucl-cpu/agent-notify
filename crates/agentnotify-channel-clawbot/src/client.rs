use std::{collections::BTreeSet, time::Duration};

use agentnotify_channel_sdk::ChannelError;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};

use crate::state::DEFAULT_BASE_URL;

const CHANNEL_VERSION: &str = "2.4.6";
const APP_ID: &str = "bot";
const APP_CLIENT_VERSION: &str = "132102";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const QR_STATUS_TIMEOUT: Duration = Duration::from_secs(35);
const MAX_LOCAL_TOKENS: usize = 10;
const API_RETRY_AFTER: time::Duration = time::Duration::seconds(1);

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct QrCodeResponse {
    #[serde(default)]
    pub qrcode: String,
    #[serde(default)]
    pub qrcode_img_content: String,
    #[serde(default)]
    pub ret: i32,
    #[serde(default)]
    pub errcode: i32,
    #[serde(default)]
    pub errmsg: String,
}

impl QrCodeResponse {
    pub fn display_content(&self) -> &str {
        let image_content = self.qrcode_img_content.trim();
        if image_content.is_empty() {
            self.qrcode.trim()
        } else {
            image_content
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct QrStatusResponse {
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub bot_token: String,
    #[serde(default)]
    pub ilink_bot_id: String,
    #[serde(default)]
    pub baseurl: String,
    #[serde(default)]
    pub ilink_user_id: String,
    #[serde(default)]
    pub redirect_host: String,
    #[serde(default)]
    pub binded_redirect: bool,
    #[serde(default)]
    pub need_verifycode: bool,
    #[serde(default)]
    pub verify_code_blocked: bool,
    #[serde(default)]
    pub ret: i32,
    #[serde(default)]
    pub errcode: i32,
    #[serde(default)]
    pub errmsg: String,
}

/// ClawBot 登录 HTTP 传输边界，测试和运行时可替换。
#[async_trait::async_trait]
pub trait ClawBotAuthTransport: Send + Sync {
    async fn fetch_qr_code(
        &self,
        base_url: &str,
        local_tokens: &[String],
    ) -> Result<QrCodeResponse, ChannelError>;

    async fn poll_qr_status(
        &self,
        base_url: &str,
        qr_code: &str,
        verify_code: Option<&str>,
    ) -> Result<QrStatusResponse, ChannelError>;
}

#[derive(Clone)]
pub struct ClawBotHttpClient {
    client: reqwest::Client,
}

impl ClawBotHttpClient {
    pub fn new() -> Result<Self, ChannelError> {
        let client = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(|_| http_error("ClawBot HTTP 客户端初始化失败"))?;
        Ok(Self { client })
    }

    async fn fetch_qr_code_once(
        &self,
        base_url: &str,
        local_tokens: &[String],
    ) -> Result<QrCodeResponse, ChannelError> {
        let endpoint = format!(
            "{}/ilink/bot/get_bot_qrcode?bot_type=3",
            normalize_base_url(base_url)?
        );
        let body = serde_json::json!({
            "local_token_list": unique_tokens(local_tokens),
            "base_info": base_info(),
        });
        let request = auth_headers(self.client.post(endpoint))
            .header("Content-Type", "application/json")
            .json(&body);
        let response = request.send().await.map_err(map_reqwest_error)?;
        let response = parse_json_response::<QrCodeResponse>(response).await?;
        check_api_status(&response.ret, &response.errcode)?;
        Ok(response)
    }
}

#[async_trait::async_trait]
impl ClawBotAuthTransport for ClawBotHttpClient {
    async fn fetch_qr_code(
        &self,
        base_url: &str,
        local_tokens: &[String],
    ) -> Result<QrCodeResponse, ChannelError> {
        let response = self.fetch_qr_code_once(base_url, local_tokens).await?;
        if !response.qrcode.trim().is_empty() {
            return Ok(response);
        }

        // 兼容只响应旧 GET 形态的服务端。
        let endpoint = format!(
            "{}/ilink/bot/get_bot_qrcode?bot_type=3",
            normalize_base_url(base_url)?
        );
        let request = auth_headers(self.client.get(endpoint));
        let response = request.send().await.map_err(map_reqwest_error)?;
        let response = parse_json_response::<QrCodeResponse>(response).await?;
        check_api_status(&response.ret, &response.errcode)?;
        if response.qrcode.trim().is_empty() {
            return Err(ChannelError::permanent(
                "clawbot_qr_response_invalid",
                "ClawBot 登录服务未返回二维码",
            ));
        }
        Ok(response)
    }

    async fn poll_qr_status(
        &self,
        base_url: &str,
        qr_code: &str,
        verify_code: Option<&str>,
    ) -> Result<QrStatusResponse, ChannelError> {
        let qr_code = qr_code.trim();
        if qr_code.is_empty() {
            return Err(ChannelError::permanent(
                "clawbot_qr_empty",
                "ClawBot 登录二维码内容为空",
            ));
        }

        let endpoint = format!(
            "{}/ilink/bot/get_qrcode_status",
            normalize_base_url(base_url)?
        );
        let mut query = vec![("qrcode", qr_code)];
        if let Some(verify_code) = verify_code.map(str::trim).filter(|value| !value.is_empty()) {
            query.push(("verify_code", verify_code));
        }
        let request =
            auth_headers(self.client.get(endpoint).query(&query)).timeout(QR_STATUS_TIMEOUT);
        let response = request.send().await.map_err(map_reqwest_error)?;
        let response = parse_json_response::<QrStatusResponse>(response).await?;
        check_api_status(&response.ret, &response.errcode)?;
        Ok(response)
    }
}

fn normalize_base_url(base_url: &str) -> Result<String, ChannelError> {
    let base_url = base_url.trim().trim_end_matches('/');
    if base_url.is_empty() {
        return Ok(DEFAULT_BASE_URL.into());
    }
    if base_url.chars().any(char::is_whitespace) {
        return Err(ChannelError::permanent(
            "clawbot_base_url_invalid",
            "ClawBot 服务地址格式无效",
        ));
    }
    Ok(base_url.into())
}

fn auth_headers(request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
    request
        .header("Accept", "application/json")
        .header("User-Agent", "Agent-notify")
        .header("X-WECHAT-UIN", random_wechat_uin())
        .header("iLink-App-Id", APP_ID)
        .header("iLink-App-ClientVersion", APP_CLIENT_VERSION)
}

fn base_info() -> serde_json::Value {
    serde_json::json!({
        "channel_version": CHANNEL_VERSION,
        "bot_agent": format!("AgentNotify/{} (windows)", env!("CARGO_PKG_VERSION")),
    })
}

fn unique_tokens(tokens: &[String]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut result = Vec::new();
    for token in tokens {
        let token = token.trim();
        if token.is_empty() || !seen.insert(token.to_owned()) {
            continue;
        }
        result.push(token.into());
        if result.len() == MAX_LOCAL_TOKENS {
            break;
        }
    }
    result
}

fn random_wechat_uin() -> String {
    let value = uuid::Uuid::new_v4().as_u128() as u32;
    STANDARD.encode(value.to_string().as_bytes())
}

async fn parse_json_response<T>(response: reqwest::Response) -> Result<T, ChannelError>
where
    T: for<'de> Deserialize<'de>,
{
    let status = response.status();
    if status.is_success() {
        return response
            .json::<T>()
            .await
            .map_err(|_| http_error("ClawBot 服务返回了无法解析的数据"));
    }

    if status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
        return Err(ChannelError::retryable(
            "clawbot_http_retryable",
            "ClawBot 服务暂时不可用，请稍后重试",
            Some(API_RETRY_AFTER),
        ));
    }

    Err(ChannelError::permanent(
        "clawbot_http_rejected",
        format!("ClawBot 请求被拒绝（HTTP {}）", status.as_u16()),
    ))
}

fn check_api_status(ret: &i32, errcode: &i32) -> Result<(), ChannelError> {
    if *ret == -14 || *errcode == -14 {
        return Err(ChannelError::invalid_account(
            "clawbot_invalid_account",
            "ClawBot 登录状态已失效，请重新扫码",
        ));
    }
    if *ret != 0 || *errcode != 0 {
        return Err(ChannelError::retryable(
            "clawbot_api_error",
            "ClawBot 服务暂时未完成请求，请稍后重试",
            Some(API_RETRY_AFTER),
        ));
    }
    Ok(())
}

fn map_reqwest_error(error: reqwest::Error) -> ChannelError {
    if error.is_timeout() || error.is_connect() || error.is_request() {
        ChannelError::retryable(
            "clawbot_network_retryable",
            "无法连接 ClawBot 服务，请检查网络后重试",
            Some(API_RETRY_AFTER),
        )
    } else {
        ChannelError::unknown(
            "clawbot_network_unknown",
            "ClawBot 网络请求结果无法确认，请稍后重试",
        )
    }
}

fn http_error(message: &'static str) -> ChannelError {
    ChannelError::retryable("clawbot_http_retryable", message, Some(API_RETRY_AFTER))
}
