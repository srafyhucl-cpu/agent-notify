use std::{path::PathBuf, process::Stdio, sync::Arc, time::Duration};

use agentnotify_agent_sdk::AgentError;
use agentnotify_domain::{AgentSessionId, SafeError};
use async_trait::async_trait;

use crate::discovery::{LanguageServerDiscovery, resolve_language_server_binary};

/// `send-message` 的确认上限，与 Go 版的 15 秒一致。
pub const DEFAULT_SEND_TIMEOUT: Duration = Duration::from_secs(15);
/// `get-conversation-metadata` 探测上限，避免逐个候选端点长时间阻塞。
pub const DEFAULT_METADATA_TIMEOUT: Duration = Duration::from_secs(10);

const LOOPBACK_PREFIX: &str = "127.0.0.1:";
const AGENT_API_SUBCOMMAND: &str = "agentapi";
const METADATA_COMMAND: &str = "get-conversation-metadata";
const SEND_COMMAND: &str = "send-message";
const ADDRESS_ENV: &str = "ANTIGRAVITY_LS_ADDRESS";
const TOKEN_ENV: &str = "ANTIGRAVITY_CSRF_TOKEN";
/// agentapi 输出只用于解析 JSON；超过上限的部分丢弃，避免异常输出撑爆内存。
const MAX_AGENT_API_OUTPUT_BYTES: u64 = 256 * 1024;
/// 错误详情上限（按字节并在字符边界截断），保证仍能塞进 SafeError 的长度上限。
const MAX_ERROR_DETAIL_BYTES: usize = 256;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// 语言服务端点：地址固定为本机回环地址，令牌随桌面端每次启动变化，不落盘、不进错误消息。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LanguageServerEndpoint {
    pub address: String,
    pub token: String,
}

impl LanguageServerEndpoint {
    pub fn loopback(port: u16, token: impl Into<String>) -> Self {
        Self {
            address: format!("{LOOPBACK_PREFIX}{port}"),
            token: token.into(),
        }
    }

    /// 只接受 127.0.0.1 的显式端口与非空令牌，避免把内容发到本机之外的服务。
    pub fn is_loopback(&self) -> bool {
        let Some(port) = self.address.strip_prefix(LOOPBACK_PREFIX) else {
            return false;
        };
        port.parse::<u16>().is_ok_and(|port| port > 0) && !self.token.trim().is_empty()
    }
}

/// 引用回复目标只能是事件里的 conversationId；不存在“最近会话”兜底。
pub fn antigravity_resume_target(conversation_id: &str) -> AgentSessionId {
    AgentSessionId::new(conversation_id.trim())
        .expect("conversationId 在事件解析阶段已校验为非空且无首尾空白")
}

/// 端点发现边界；真实实现枚举本机语言服务进程与监听端口。
pub trait EndpointDiscovery: Send + Sync {
    fn discover(&self) -> Result<Vec<LanguageServerEndpoint>, AgentError>;
}

/// agentapi 的两项能力；真实实现启动语言服务自带的 agentapi 子命令。
#[async_trait]
pub trait AgentApi: Send + Sync {
    async fn conversation_exists(
        &self,
        endpoint: &LanguageServerEndpoint,
        conversation_id: &str,
    ) -> Result<bool, AgentError>;

    async fn send_message(
        &self,
        endpoint: &LanguageServerEndpoint,
        conversation_id: &str,
        text: &str,
    ) -> Result<(), AgentError>;
}

/// 精确回复边界：只在认识目标会话的那个本机端点上发送，绝不新建语言服务或回退最近会话。
#[derive(Clone)]
pub struct AntigravityReply {
    discovery: Arc<dyn EndpointDiscovery>,
    api: Arc<dyn AgentApi>,
    metadata_timeout: Duration,
    send_timeout: Duration,
}

impl AntigravityReply {
    pub fn new(discovery: Arc<dyn EndpointDiscovery>, api: Arc<dyn AgentApi>) -> Self {
        Self {
            discovery,
            api,
            metadata_timeout: DEFAULT_METADATA_TIMEOUT,
            send_timeout: DEFAULT_SEND_TIMEOUT,
        }
    }

    pub fn from_default_location() -> Self {
        Self::new(
            Arc::new(LanguageServerDiscovery::from_default_location()),
            Arc::new(AgentApiProcess::from_default_location()),
        )
    }

    /// 占位实现：不做任何进程发现，测试必须显式注入假后端，避免误触真实桌面端。
    pub fn without_backend() -> Self {
        Self::new(Arc::new(NoDiscovery), Arc::new(NoApi))
    }

    pub fn with_timeouts(mut self, metadata_timeout: Duration, send_timeout: Duration) -> Self {
        self.metadata_timeout = metadata_timeout;
        self.send_timeout = send_timeout;
        self
    }

    /// 先验证目标会话确实存在于某个本机语言服务，再发送；任一步失败都明确报错。
    pub async fn send(&self, session_id: &AgentSessionId, text: &str) -> Result<(), AgentError> {
        let text = text.trim();
        if text.is_empty() {
            return Err(AgentError::InvalidInput);
        }
        let conversation_id = session_id.as_str();

        let discovered = self.discovery.discover()?;
        let endpoints: Vec<LanguageServerEndpoint> = discovered
            .iter()
            .filter(|endpoint| endpoint.is_loopback())
            .cloned()
            .collect();
        if endpoints.is_empty() {
            // 区分“没有语言服务”与“发现了非法端点”，错误提示必须让用户知道下一步做什么。
            return Err(if discovered.is_empty() {
                unavailable(
                    "antigravity_language_server_missing",
                    "未找到正在运行的 Antigravity 语言服务：请确认 Antigravity 桌面端已打开后重试",
                )
            } else {
                unavailable(
                    "antigravity_endpoint_rejected",
                    "Antigravity 语言服务端点无效，已拒绝连接：请重启 Antigravity 桌面端后重试",
                )
            });
        }

        let mut last_error = None;
        let mut target = None;
        for endpoint in &endpoints {
            match self.probe_conversation(endpoint, conversation_id).await {
                Ok(true) => {
                    target = Some(endpoint);
                    break;
                }
                Ok(false) => {}
                Err(error) => last_error = Some(error),
            }
        }
        let Some(target) = target else {
            return Err(last_error.unwrap_or_else(|| {
                unavailable(
                    "antigravity_conversation_unavailable",
                    "Antigravity 会话当前不可用：请确认该会话仍存在于桌面端后重试",
                )
            }));
        };

        match tokio::time::timeout(
            self.send_timeout,
            self.api.send_message(target, conversation_id, text),
        )
        .await
        {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => Err(error),
            Err(_) => Err(AgentError::Unknown(safe_error(
                "antigravity_send_unconfirmed",
                "Antigravity 语言服务响应超时，无法确认引用回复是否送达：请确认桌面端仍处于打开状态后重试",
            ))),
        }
    }

    async fn probe_conversation(
        &self,
        endpoint: &LanguageServerEndpoint,
        conversation_id: &str,
    ) -> Result<bool, AgentError> {
        match tokio::time::timeout(
            self.metadata_timeout,
            self.api.conversation_exists(endpoint, conversation_id),
        )
        .await
        {
            Ok(result) => result,
            Err(_) => Err(unavailable(
                "antigravity_metadata_timeout",
                "Antigravity 语言服务响应超时：请确认桌面端仍处于打开状态后重试",
            )),
        }
    }
}

/// 默认发现与默认 API 都不可用的占位实现，只用于测试隔离。
struct NoDiscovery;

impl EndpointDiscovery for NoDiscovery {
    fn discover(&self) -> Result<Vec<LanguageServerEndpoint>, AgentError> {
        Err(unavailable(
            "antigravity_test_backend",
            "Antigravity 测试后端未注入",
        ))
    }
}

struct NoApi;

#[async_trait]
impl AgentApi for NoApi {
    async fn conversation_exists(
        &self,
        _endpoint: &LanguageServerEndpoint,
        _conversation_id: &str,
    ) -> Result<bool, AgentError> {
        Err(unavailable(
            "antigravity_test_backend",
            "Antigravity 测试后端未注入",
        ))
    }

    async fn send_message(
        &self,
        _endpoint: &LanguageServerEndpoint,
        _conversation_id: &str,
        _text: &str,
    ) -> Result<(), AgentError> {
        Err(unavailable(
            "antigravity_test_backend",
            "Antigravity 测试后端未注入",
        ))
    }
}

/// 真实 agentapi 客户端：启动语言服务自带的 `agentapi` 子命令，令牌只走环境变量。
pub struct AgentApiProcess {
    binary: Option<PathBuf>,
}

impl AgentApiProcess {
    pub fn from_default_location() -> Self {
        Self { binary: None }
    }

    async fn call(
        &self,
        endpoint: &LanguageServerEndpoint,
        args: &[&str],
    ) -> Result<String, AgentError> {
        if !endpoint.is_loopback() {
            return Err(unavailable(
                "antigravity_endpoint_rejected",
                "Antigravity 语言服务端点无效，已拒绝连接",
            ));
        }
        let binary = match &self.binary {
            Some(binary) => binary.clone(),
            None => resolve_language_server_binary()?,
        };

        let mut command = tokio::process::Command::new(&binary);
        command
            .arg(AGENT_API_SUBCOMMAND)
            .args(args)
            .env(ADDRESS_ENV, &endpoint.address)
            .env(TOKEN_ENV, &endpoint.token)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(CREATE_NO_WINDOW);

        let mut child = command.spawn().map_err(|_| {
            unavailable(
                "antigravity_language_server_spawn_failed",
                "无法启动 Antigravity 语言服务，请确认桌面端仍在运行后重试",
            )
        })?;
        let mut buffer = Vec::new();
        if let Some(stdout) = child.stdout.take() {
            let mut reader = tokio::io::AsyncReadExt::take(stdout, MAX_AGENT_API_OUTPUT_BYTES);
            // 读取失败只影响响应解析，真正的失败会在退出码里体现。
            let _ = tokio::io::AsyncReadExt::read_to_end(&mut reader, &mut buffer).await;
        }
        let status = child.wait().await.map_err(|_| {
            failed(
                "antigravity_language_server_wait_failed",
                "等待 Antigravity 语言服务失败，请重试",
            )
        })?;
        if !status.success() {
            return Err(failed(
                "antigravity_agent_api_failed",
                "Antigravity 语言服务命令执行失败，请确认桌面端仍在运行后重试",
            ));
        }
        let output = String::from_utf8_lossy(&buffer);
        extract_json_object(&output).ok_or_else(|| {
            failed(
                "antigravity_response_unparsable",
                "Antigravity 语言服务未返回可解析的响应，请重试",
            )
        })
    }
}

#[async_trait]
impl AgentApi for AgentApiProcess {
    async fn conversation_exists(
        &self,
        endpoint: &LanguageServerEndpoint,
        conversation_id: &str,
    ) -> Result<bool, AgentError> {
        let output = self
            .call(endpoint, &[METADATA_COMMAND, conversation_id])
            .await?;
        let response: AgentApiResponse = serde_json::from_str(&output).map_err(|_| {
            failed(
                "antigravity_response_invalid",
                "Antigravity 语言服务响应无法解析，请重试",
            )
        })?;
        if !response.error.trim().is_empty() {
            return Err(language_server_error(&response.error, endpoint));
        }
        let metadata = &response.response.conversation_metadata.metadata;
        Ok(metadata.root_conversation_id == conversation_id
            || metadata.parent_conversation_id == conversation_id)
    }

    async fn send_message(
        &self,
        endpoint: &LanguageServerEndpoint,
        conversation_id: &str,
        text: &str,
    ) -> Result<(), AgentError> {
        self.call(endpoint, &[SEND_COMMAND, conversation_id, text])
            .await
            .map(|_| ())
    }
}

#[derive(Default, serde::Deserialize)]
struct AgentApiResponse {
    #[serde(default)]
    response: AgentApiBody,
    #[serde(default)]
    error: String,
}

#[derive(Default, serde::Deserialize)]
struct AgentApiBody {
    #[serde(default, rename = "conversationMetadata")]
    conversation_metadata: ConversationMetadata,
}

#[derive(Default, serde::Deserialize)]
struct ConversationMetadata {
    #[serde(default)]
    metadata: ConversationIds,
}

#[derive(Default, serde::Deserialize)]
struct ConversationIds {
    #[serde(default, rename = "rootConversationId")]
    root_conversation_id: String,
    #[serde(default, rename = "parentConversationId")]
    parent_conversation_id: String,
}

/// 从 agentapi 输出中截取唯一的 JSON 对象，避免日志前缀干扰（与 Go 版一致）。
fn extract_json_object(output: &str) -> Option<String> {
    let start = output.find('{')?;
    let end = output.rfind('}')?;
    (end > start).then(|| output[start..=end].to_owned())
}

/// 只做分类与提示，不透传可能包含用户内容、令牌或端口的原始输出。
fn language_server_error(detail: &str, endpoint: &LanguageServerEndpoint) -> AgentError {
    // 先剔除令牌与端口再截断，保证最终消息既安全又不超长。
    let detail = compact_detail(&scrub_endpoint(detail.to_owned(), endpoint));
    let normalized = detail.to_lowercase();
    if normalized.contains("csrf") || normalized.contains("token") {
        return failed(
            "antigravity_token_expired",
            "Antigravity 语言服务令牌已失效：请在桌面端重新打开该会话后重试",
        );
    }
    if normalized.contains("not found") || normalized.contains("no such conversation") {
        return unavailable(
            "antigravity_conversation_unavailable",
            "Antigravity 会话当前不可用：请确认该会话仍存在于桌面端后重试",
        );
    }
    if detail.is_empty() {
        return failed(
            "antigravity_language_server_error",
            "Antigravity 语言服务返回错误，请重新打开该会话后重试",
        );
    }
    failed(
        "antigravity_language_server_error",
        &format!("Antigravity 语言服务返回错误：{detail}"),
    )
}

fn compact_detail(output: &str) -> String {
    let collapsed = output.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut detail = String::new();
    for character in collapsed.chars() {
        if detail.len() + character.len_utf8() > MAX_ERROR_DETAIL_BYTES {
            break;
        }
        detail.push(character);
    }
    detail
}

/// 令牌与端口不进入错误消息；即使服务端把它们回显出来也要先剔除。
fn scrub_endpoint(text: String, endpoint: &LanguageServerEndpoint) -> String {
    let mut text = text;
    for secret in [endpoint.token.trim(), endpoint.address.trim()] {
        if !secret.is_empty() {
            text = text.replace(secret, "[已隐藏]");
        }
    }
    text
}

fn unavailable(code: &str, message: &str) -> AgentError {
    AgentError::Unavailable(safe_error(code, message))
}

fn failed(code: &str, message: &str) -> AgentError {
    AgentError::Failed(safe_error(code, message))
}

fn safe_error(code: &str, message: &str) -> SafeError {
    SafeError::new(code, message).expect("Antigravity 错误常量必须是有效安全错误")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_endpoint() -> LanguageServerEndpoint {
        LanguageServerEndpoint::loopback(62957, "secret-token")
    }

    #[test]
    fn token_errors_are_classified_without_leaking_secrets() {
        let error = language_server_error(
            "CSRF token rejected for secret-token on 127.0.0.1:62957",
            &test_endpoint(),
        );

        assert_eq!(error.code(), "antigravity_token_expired");
        let message = error.to_string();
        assert!(!message.contains("secret-token"), "{message}");
        assert!(!message.contains("62957"), "{message}");
        assert!(message.contains("令牌已失效"), "{message}");
    }

    #[test]
    fn conversation_errors_map_to_unavailable_conversation() {
        let error = language_server_error("conversation not found", &test_endpoint());

        assert_eq!(error.code(), "antigravity_conversation_unavailable");
    }

    #[test]
    fn unknown_errors_are_scrubbed_and_bounded() {
        let detail = format!("rpc failed {} secret-token", "字".repeat(400));
        let error = language_server_error(&detail, &test_endpoint());

        assert_eq!(error.code(), "antigravity_language_server_error");
        let message = error.to_string();
        assert!(!message.contains("secret-token"), "{message}");
        assert!(
            message.len() <= MAX_ERROR_DETAIL_BYTES + 64,
            "错误消息必须满足 SafeError 长度上限：{} 字节",
            message.len()
        );
    }

    #[test]
    fn json_object_is_extracted_from_noisy_output() {
        assert_eq!(
            extract_json_object("log line\r\n{\"error\":\"\"}\n").as_deref(),
            Some("{\"error\":\"\"}")
        );
        assert_eq!(extract_json_object("no json here"), None);
        assert_eq!(extract_json_object("}{"), None);
    }
}
