//! Devin Stop Hook 的纯逻辑：读取有界 stdin，把原始 Stop 事件按 `protocolVersion=1`
//! 的 `agent.event` 提交给 `agentnotify-ingress.exe`，并且**始终**输出 Devin 要求的 `{}`。
//!
//! 顺序契约：Hook 只提交事件与写诊断，不改变 Devin 的退出语义；
//! 解析失败或 ingress 失败只留下诊断，绝不阻塞 Agent。

use std::{
    env, io,
    io::Write,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use async_trait::async_trait;
use serde::Serialize;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};

pub const AGENT_ID: &str = "devin";
pub const PROTOCOL_VERSION: u8 = 1;
pub const EVENT_KIND: &str = "agent.event";
/// Devin 要求 Stop Hook 输出的空 JSON 对象。
pub const STOP_RESPONSE: &str = "{}";
/// 有界 stdin：与 ingress payload 上限同量级，超出部分丢弃。
pub const MAX_STDIN_BYTES: usize = 256 * 1024;
/// stdin 读取上限时长：Devin 写完事件后不关管道时，Hook 也不能挂住。
pub const STDIN_TIMEOUT: Duration = Duration::from_secs(2);
/// ingress 提交上限；超时按失败吞掉，只写诊断。
pub const INGRESS_TIMEOUT: Duration = Duration::from_secs(10);

/// 旧 AgentNotify 入口的 Agent 选择参数，Hook 自身消费。
const SELECTOR_ARGUMENT: &str = "devin";
/// 只有被当作 Stop Hook 调用（`devin stop`）时才提交事件。
const STOP_ARGUMENT: &str = "stop";
const INGRESS_ENV: &str = "AGENT_NOTIFY_INGRESS";
const LOCAL_APP_DATA_ENV: &str = "LOCALAPPDATA";
const INGRESS_EXE_NAME: &str = "agentnotify-ingress.exe";
/// Rust 桌面版默认安装目录（见 installer/agent-notify-rust.iss）。
const INGRESS_INSTALL_SUBDIR: [&str; 3] = ["Programs", "Agent-notify", "agentnotify-ingress.exe"];
/// ingress 对协议错误返回的退出码（见 apps/ingress/src/main.rs）。
const PROTOCOL_REJECTED_EXIT_CODE: i32 = 2;

/// 诊断日志沿用 Go 版目录习惯：`%TEMP%\agent-notify\devin-notify-debug.log`。
const DEBUG_DIR_NAME: &str = "agent-notify";
const DEBUG_FILE_NAME: &str = "devin-notify-debug.log";
const TEMP_DIR_ENV: &str = "AGENT_NOTIFY_TEMP_DIR";
/// 诊断日志上限：超过后清空重写，只保留最新失败，避免日志无界增长。
pub const DEBUG_LOG_MAX_BYTES: u64 = 512 * 1024;
/// 单条诊断最大字符数，防止异常文本本身撑爆日志。
const DEBUG_MESSAGE_MAX_CHARS: usize = 500;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Hook 运行结果；`stop_handled=false` 表示本次调用不是 Stop Hook，什么都没提交。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HookOutcome {
    pub stop_handled: bool,
    pub ingress_failure: Option<IngressFailure>,
}

/// ingress 失败原因，只用于诊断，不影响 Hook 的输出与退出语义。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IngressFailure {
    NotFound,
    SpawnFailed(String),
    WriteFailed(String),
    WaitFailed(String),
    Exit {
        code: Option<i32>,
    },
    Timeout,
    /// 事件无法编码（正常路径不会发生）。
    Envelope(String),
}

impl IngressFailure {
    fn message(&self) -> String {
        match self {
            Self::NotFound => format!("未找到 {INGRESS_EXE_NAME}，Devin 事件未提交"),
            Self::SpawnFailed(error) => format!("ingress 启动失败：{error}"),
            Self::WriteFailed(error) => format!("写入 ingress 失败：{error}"),
            Self::WaitFailed(error) => format!("等待 ingress 失败：{error}"),
            Self::Exit {
                code: Some(PROTOCOL_REJECTED_EXIT_CODE),
            } => format!(
                "ingress 退出码 {PROTOCOL_REJECTED_EXIT_CODE}：事件被协议拒绝（正文或 payload 超限），Devin 事件未提交"
            ),
            Self::Exit { code: Some(code) } => {
                format!("ingress 退出码 {code}，Devin 事件未提交")
            }
            Self::Exit { code: None } => "ingress 异常退出，Devin 事件未提交".to_owned(),
            Self::Timeout => "ingress 提交超时，Devin 事件未提交".to_owned(),
            Self::Envelope(error) => format!("无法编码 Devin 事件：{error}"),
        }
    }
}

/// Hook 的时间上限；默认值用于生产，测试可缩短以覆盖超时路径。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HookLimits {
    pub ingress_timeout: Duration,
}

impl Default for HookLimits {
    fn default() -> Self {
        Self {
            ingress_timeout: INGRESS_TIMEOUT,
        }
    }
}

/// 进程边界：真实实现启动 ingress，测试注入记录型实现。
#[async_trait]
pub trait HookPrograms: Send + Sync {
    /// 把标准事件提交给 ingress。
    async fn run_ingress(&self, envelope: &[u8]) -> Result<(), IngressFailure>;

    /// 追加一条脱敏诊断；调用方吞掉错误，日志失败绝不影响推送。
    async fn log_diagnostic(&self, message: &str) -> io::Result<()>;
}

/// 真实进程实现：只启动 agentnotify-ingress.exe。
pub struct RealPrograms;

#[async_trait]
impl HookPrograms for RealPrograms {
    async fn run_ingress(&self, envelope: &[u8]) -> Result<(), IngressFailure> {
        let ingress = resolve_ingress_path().ok_or(IngressFailure::NotFound)?;
        let mut command = tokio::process::Command::new(&ingress);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(CREATE_NO_WINDOW);

        let mut child = command
            .spawn()
            .map_err(|error| IngressFailure::SpawnFailed(error.to_string()))?;
        if let Some(mut child_stdin) = child.stdin.take() {
            child_stdin
                .write_all(envelope)
                .await
                .map_err(|error| IngressFailure::WriteFailed(error.to_string()))?;
        }
        let status = child
            .wait()
            .await
            .map_err(|error| IngressFailure::WaitFailed(error.to_string()))?;
        if status.success() {
            Ok(())
        } else {
            Err(IngressFailure::Exit {
                code: status.code(),
            })
        }
    }

    async fn log_diagnostic(&self, message: &str) -> io::Result<()> {
        let message = message.to_owned();
        tokio::task::spawn_blocking(move || append_debug_line(&message))
            .await
            .unwrap_or_else(|_| Err(io::Error::other("诊断日志写入任务失败")))
    }
}

/// 只在 `devin stop` 调用下提交事件；其余调用什么都不做。
pub async fn run_hook(args: &[String], stdin: &[u8], programs: &dyn HookPrograms) -> HookOutcome {
    run_hook_with_limits(args, stdin, programs, HookLimits::default()).await
}

/// 与 `run_hook` 相同，但可注入更短的时间上限以覆盖超时诊断路径。
pub async fn run_hook_with_limits(
    args: &[String],
    stdin: &[u8],
    programs: &dyn HookPrograms,
    limits: HookLimits,
) -> HookOutcome {
    if !is_stop_invocation(args) {
        return HookOutcome {
            stop_handled: false,
            ingress_failure: None,
        };
    }

    let ingress_failure = match build_envelope(stdin) {
        Ok(envelope) => {
            match tokio::time::timeout(limits.ingress_timeout, programs.run_ingress(&envelope))
                .await
            {
                Ok(Ok(())) => None,
                Ok(Err(failure)) => Some(failure),
                Err(_) => Some(IngressFailure::Timeout),
            }
        }
        Err(error) => Some(IngressFailure::Envelope(error)),
    };
    if let Some(failure) = &ingress_failure {
        let _ = programs.log_diagnostic(&failure.message()).await;
    }

    HookOutcome {
        stop_handled: true,
        ingress_failure,
    }
}

/// 消费首个 `devin` 选择参数后，首个参数必须是 `stop`。
fn is_stop_invocation(args: &[String]) -> bool {
    let mut remaining = args.iter().map(String::as_str);
    let first = remaining.next();
    let candidate = match first {
        Some(value) if value.trim().eq_ignore_ascii_case(SELECTOR_ARGUMENT) => remaining.next(),
        other => other,
    };
    candidate.is_some_and(|value| value.trim().eq_ignore_ascii_case(STOP_ARGUMENT))
}

/// 生成标准 `agent.event`；stdin 为空或不是 JSON 对象时按空对象提交，由适配器决定跳过。
pub fn build_envelope(stdin: &[u8]) -> Result<Vec<u8>, String> {
    let payload = parse_payload(stdin);
    let envelope = WireEnvelope {
        protocol_version: PROTOCOL_VERSION,
        kind: EVENT_KIND,
        request_id: uuid::Uuid::new_v4().to_string(),
        agent_id: AGENT_ID,
        payload: &payload,
    };
    serde_json::to_vec(&envelope).map_err(|error| format!("无法编码 Devin 事件：{error}"))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WireEnvelope<'a> {
    protocol_version: u8,
    kind: &'static str,
    request_id: String,
    agent_id: &'static str,
    payload: &'a serde_json::Value,
}

/// 容忍 BOM 与空白；无法解析时按空对象处理，适配器会把缺字段的事件标记为 Ignored。
fn parse_payload(stdin: &[u8]) -> serde_json::Value {
    let text = std::str::from_utf8(stdin)
        .unwrap_or_default()
        .trim_start_matches('\u{feff}')
        .trim();
    if text.is_empty() {
        return serde_json::json!({});
    }
    match serde_json::from_str::<serde_json::Value>(text) {
        Ok(value) if value.is_object() => value,
        _ => serde_json::json!({}),
    }
}

/// 有界读取：最多 `MAX_STDIN_BYTES`，超出的部分丢弃。
pub async fn read_bounded<R>(reader: R) -> io::Result<Vec<u8>>
where
    R: AsyncRead + Unpin,
{
    let mut buffer = Vec::new();
    reader
        .take(MAX_STDIN_BYTES as u64 + 1)
        .read_to_end(&mut buffer)
        .await?;
    buffer.truncate(MAX_STDIN_BYTES);
    Ok(buffer)
}

/// 按 env → Hook 同目录 → 正式安装目录的顺序查找 ingress。
pub fn resolve_ingress_path() -> Option<PathBuf> {
    if let Some(configured) = env::var_os(INGRESS_ENV).filter(|value| !value.is_empty()) {
        let path = PathBuf::from(configured);
        if path.is_file() {
            return Some(path);
        }
    }
    if let Some(directory) = env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
    {
        let candidate = directory.join(INGRESS_EXE_NAME);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    let local_app_data =
        PathBuf::from(env::var_os(LOCAL_APP_DATA_ENV).filter(|value| !value.is_empty())?);
    let candidate = local_app_data
        .join(INGRESS_INSTALL_SUBDIR[0])
        .join(INGRESS_INSTALL_SUBDIR[1])
        .join(INGRESS_INSTALL_SUBDIR[2]);
    candidate.is_file().then_some(candidate)
}

/// 诊断日志路径：`AGENT_NOTIFY_TEMP_DIR` 优先，其次 `%TEMP%\agent-notify\devin-notify-debug.log`。
pub fn debug_log_path() -> Option<PathBuf> {
    if let Some(configured) = env::var_os(TEMP_DIR_ENV).filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(configured).join(DEBUG_FILE_NAME));
    }
    let temp = env::var_os("TEMP")
        .or_else(|| env::var_os("TMP"))
        .filter(|value| !value.is_empty())?;
    Some(
        PathBuf::from(temp)
            .join(DEBUG_DIR_NAME)
            .join(DEBUG_FILE_NAME),
    )
}

/// 追加一条诊断；超过上限时清空重写，保证最近一次失败一定留下痕迹。
pub fn append_debug_line(message: &str) -> io::Result<()> {
    let path = debug_log_path()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "无法确定诊断日志路径"))?;
    append_debug_line_to(&path, message)
}

/// 指定路径的追加实现，供测试与隔离诊断使用。
pub fn append_debug_line_to(path: &Path, message: &str) -> io::Result<()> {
    if let Some(directory) = path.parent() {
        std::fs::create_dir_all(directory)?;
    }
    if std::fs::metadata(path)
        .map(|metadata| metadata.len() >= DEBUG_LOG_MAX_BYTES)
        .unwrap_or(false)
    {
        // 超过上限时清空重写：策略简单可解释，且不会无界增长。
        std::fs::write(path, b"")?;
    }

    let timestamp = time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "unknown-time".to_owned());
    // 折叠空白并限长，防止异常文本把多行内容或超长内容写进日志。
    let message: String = message.chars().take(DEBUG_MESSAGE_MAX_CHARS).collect();
    let message = message.split_whitespace().collect::<Vec<_>>().join(" ");
    let line = format!("{timestamp} {message}\n");

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    file.write_all(line.as_bytes())
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn stop_args() -> Vec<String> {
        args(&["devin", "stop"])
    }

    #[derive(Default)]
    struct FakePrograms {
        envelopes: Mutex<Vec<String>>,
        failure: Option<IngressFailure>,
        delay: Option<Duration>,
        diagnostics: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl HookPrograms for FakePrograms {
        async fn run_ingress(&self, envelope: &[u8]) -> Result<(), IngressFailure> {
            self.envelopes
                .lock()
                .unwrap()
                .push(String::from_utf8_lossy(envelope).into_owned());
            if let Some(delay) = self.delay {
                tokio::time::sleep(delay).await;
            }
            match &self.failure {
                Some(failure) => Err(failure.clone()),
                None => Ok(()),
            }
        }

        async fn log_diagnostic(&self, message: &str) -> io::Result<()> {
            self.diagnostics.lock().unwrap().push(message.to_owned());
            Ok(())
        }
    }

    #[tokio::test]
    async fn stop_invocation_submits_exactly_one_event() {
        let programs = FakePrograms::default();

        let outcome = run_hook(
            &stop_args(),
            br#"{"session_id":"session-1","hook_event_name":"Stop","stop_hook_active":false,"last_assistant_message":"done"}"#,
            &programs,
        )
        .await;

        assert!(outcome.stop_handled);
        assert!(outcome.ingress_failure.is_none());
        let envelopes = programs.envelopes.lock().unwrap();
        assert_eq!(envelopes.len(), 1);
        let envelope: serde_json::Value = serde_json::from_str(&envelopes[0]).unwrap();
        assert_eq!(envelope["agentId"], AGENT_ID);
        assert_eq!(envelope["payload"]["session_id"], "session-1");
        assert_eq!(envelope["payload"]["last_assistant_message"], "done");
        assert!(programs.diagnostics.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn non_stop_invocation_never_touches_ingress() {
        let programs = FakePrograms::default();

        let outcome = run_hook(&args(&["devin", "status"]), b"{}", &programs).await;

        assert!(!outcome.stop_handled);
        assert!(outcome.ingress_failure.is_none());
        assert!(programs.envelopes.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn ingress_failure_is_returned_and_logged_once() {
        let programs = FakePrograms {
            failure: Some(IngressFailure::Exit { code: Some(2) }),
            ..FakePrograms::default()
        };

        let outcome = run_hook(&stop_args(), b"{}", &programs).await;

        assert_eq!(
            outcome.ingress_failure,
            Some(IngressFailure::Exit { code: Some(2) })
        );
        let diagnostics = programs.diagnostics.lock().unwrap();
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].contains("协议拒绝"), "{diagnostics:?}");
    }

    #[tokio::test]
    async fn ingress_timeout_is_bounded_and_logged() {
        let programs = FakePrograms {
            delay: Some(Duration::from_secs(5)),
            ..FakePrograms::default()
        };
        let limits = HookLimits {
            ingress_timeout: Duration::from_millis(20),
        };

        let outcome = run_hook_with_limits(&stop_args(), b"{}", &programs, limits).await;

        assert_eq!(outcome.ingress_failure, Some(IngressFailure::Timeout));
        let diagnostics = programs.diagnostics.lock().unwrap();
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].contains("超时"), "{diagnostics:?}");
    }

    #[test]
    fn stop_response_is_the_empty_json_object() {
        assert_eq!(STOP_RESPONSE, "{}");
    }

    #[test]
    fn only_stop_invocations_are_handled() {
        assert!(is_stop_invocation(&args(&["devin", "stop"])));
        assert!(is_stop_invocation(&args(&["stop"])));
        assert!(is_stop_invocation(&args(&["DEVIN", "STOP"])));
        assert!(!is_stop_invocation(&args(&[])));
        assert!(!is_stop_invocation(&args(&["devin", "status"])));
        assert!(!is_stop_invocation(&args(&["codex", "stop"])));
    }

    #[test]
    fn envelope_carries_the_raw_stop_payload() {
        let stdin = r#"{"session_id":"session-1","prompt_id":"prompt-1","hook_event_name":"Stop","stop_hook_active":true,"last_assistant_message":"完成"}"#;

        let bytes = build_envelope(stdin.as_bytes()).unwrap();
        let envelope: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(envelope["protocolVersion"], PROTOCOL_VERSION);
        assert_eq!(envelope["kind"], EVENT_KIND);
        assert_eq!(envelope["agentId"], AGENT_ID);
        let request_id = envelope["requestId"].as_str().unwrap();
        assert_eq!(
            uuid::Uuid::parse_str(request_id).unwrap().to_string(),
            request_id
        );
        assert_eq!(envelope["payload"]["stop_hook_active"], true);
        assert_eq!(envelope["payload"]["last_assistant_message"], "完成");
    }

    #[test]
    fn stdin_without_json_or_with_bom_falls_back_to_an_empty_payload() {
        for stdin in [
            b"".as_slice(),
            b"   ".as_slice(),
            b"\xef\xbb\xbf".as_slice(),
        ] {
            let bytes = build_envelope(stdin).unwrap();
            let envelope: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(envelope["payload"], serde_json::json!({}));
        }

        let bytes = build_envelope("\u{feff}{\"session_id\":\"session-1\"}".as_bytes()).unwrap();
        let envelope: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(envelope["payload"]["session_id"], "session-1");
    }

    #[tokio::test]
    async fn stdin_read_is_bounded() {
        let oversized = vec![b'x'; MAX_STDIN_BYTES + 1024];

        let buffer = read_bounded(oversized.as_slice()).await.unwrap();

        assert_eq!(buffer.len(), MAX_STDIN_BYTES);
    }

    #[test]
    fn debug_log_is_bounded_and_contains_reason() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory
            .path()
            .join("agent-notify")
            .join("devin-notify-debug.log");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, vec![b'x'; DEBUG_LOG_MAX_BYTES as usize + 1]).unwrap();

        append_debug_line_to(&path, "ingress 退出码 2：事件被协议拒绝").unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("ingress 退出码 2"), "{content}");
        assert!((content.len() as u64) < DEBUG_LOG_MAX_BYTES);
    }

    #[test]
    fn debug_log_write_failure_is_reported_to_caller() {
        let directory = tempfile::tempdir().unwrap();
        let blocker = directory.path().join("blocker");
        std::fs::write(&blocker, b"file").unwrap();

        let result = append_debug_line_to(&blocker.join("devin-notify-debug.log"), "x");

        assert!(result.is_err(), "父路径是文件时必须返回错误");
    }
}
