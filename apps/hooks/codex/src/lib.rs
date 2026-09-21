//! Codex notify Hook 的纯逻辑：先把参数与 stdin 透传给上游 `codex-computer-use.exe`，
//! 再把完成事件按 `protocolVersion=1` 的 `agent.event` 提交给 `agentnotify-ingress.exe`。
//!
//! 顺序契约：上游先运行，ingress 后提交；ingress 失败只写诊断，绝不改变上游退出语义。

use std::{
    env, io,
    io::Write,
    path::{Path, PathBuf},
    process::Stdio,
    time::{Duration, SystemTime},
};

use async_trait::async_trait;
use serde::Serialize;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};

pub const AGENT_ID: &str = "codex";
pub const PROTOCOL_VERSION: u8 = 1;
pub const EVENT_KIND: &str = "agent.event";
/// 有界 stdin：与 ingress payload 上限同量级，超出部分丢弃。
pub const MAX_STDIN_BYTES: usize = 256 * 1024;
/// stdin 读取上限时长：Codex 写完事件后不关管道时，Hook 也不能挂住。
pub const STDIN_TIMEOUT: Duration = Duration::from_secs(2);
/// 上游总时长上限，与 Go 版旁路 helper 的 30 秒一致。
pub const UPSTREAM_TIMEOUT: Duration = Duration::from_secs(30);
/// ingress 提交上限；超时按失败吞掉，只写诊断。
pub const INGRESS_TIMEOUT: Duration = Duration::from_secs(10);

/// 旧 AgentNotify 入口的 Agent 选择参数，Hook 自身消费，不转给上游。
const SELECTOR_ARGUMENT: &str = "codex";
const UPSTREAM_ENV: &str = "AGENT_NOTIFY_CODEX_UPSTREAM";
const INGRESS_ENV: &str = "AGENT_NOTIFY_INGRESS";
const LOCAL_APP_DATA_ENV: &str = "LOCALAPPDATA";
const INGRESS_EXE_NAME: &str = "agentnotify-ingress.exe";
const CUA_EXE_NAME: &str = "codex-computer-use.exe";
/// Rust 桌面版默认安装目录（见 installer/agent-notify-rust.iss）。
const INGRESS_INSTALL_SUBDIR: [&str; 3] = ["Programs", "Agent-notify", "agentnotify-ingress.exe"];
/// ingress 对协议错误返回的退出码（见 apps/ingress/src/main.rs）。
const PROTOCOL_REJECTED_EXIT_CODE: i32 = 2;

/// 诊断日志沿用 Go 版路径：`%TEMP%\agent-notify\codex-notify-debug.log`。
const DEBUG_DIR_NAME: &str = "agent-notify";
const DEBUG_FILE_NAME: &str = "codex-notify-debug.log";
const TEMP_DIR_ENV: &str = "AGENT_NOTIFY_TEMP_DIR";
/// 诊断日志上限：超过后清空重写，只保留最新失败，避免日志无界增长。
pub const DEBUG_LOG_MAX_BYTES: u64 = 512 * 1024;
/// 单条诊断最大字符数，防止异常文本本身撑爆日志。
const DEBUG_MESSAGE_MAX_CHARS: usize = 500;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Hook 运行结果；`upstream_exit_code` 为 None 表示上游没找到、没启动起来或超时。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HookOutcome {
    pub upstream_exit_code: Option<i32>,
    pub ingress_failure: Option<IngressFailure>,
}

impl HookOutcome {
    /// 退出码原样返回上游结果，拿不到时按成功处理，绝不让通知故障改变 Codex 语义。
    pub fn exit_code(&self) -> u8 {
        match self.upstream_exit_code {
            Some(code) if (0..=255).contains(&code) => code as u8,
            _ => 0,
        }
    }
}

/// 上游失败原因，只用于诊断，不影响 Hook 退出语义。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UpstreamFailure {
    NotFound,
    SpawnFailed(String),
    WaitFailed(String),
    /// 上游结束但没有正常退出码（例如被终止）。
    NoExitCode,
}

impl UpstreamFailure {
    fn message(&self) -> String {
        match self {
            Self::NotFound => format!("未找到上游 {CUA_EXE_NAME}，未执行透传"),
            Self::SpawnFailed(error) => format!("上游启动失败：{error}"),
            Self::WaitFailed(error) => format!("等待上游失败：{error}"),
            Self::NoExitCode => "上游未返回正常退出码".to_owned(),
        }
    }
}

/// ingress 失败原因，只用于诊断，不影响 Hook 退出语义。
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
    /// 事件本身无法编码（正常路径不会发生）。
    Envelope(String),
}

impl IngressFailure {
    fn message(&self) -> String {
        match self {
            Self::NotFound => format!("未找到 {INGRESS_EXE_NAME}，Codex 事件未提交"),
            Self::SpawnFailed(error) => format!("ingress 启动失败：{error}"),
            Self::WriteFailed(error) => format!("写入 ingress 失败：{error}"),
            Self::WaitFailed(error) => format!("等待 ingress 失败：{error}"),
            Self::Exit {
                code: Some(PROTOCOL_REJECTED_EXIT_CODE),
            } => format!(
                "ingress 退出码 {PROTOCOL_REJECTED_EXIT_CODE}：事件被协议拒绝（正文或 payload 超限），Codex 事件未提交"
            ),
            Self::Exit { code: Some(code) } => format!("ingress 退出码 {code}，Codex 事件未提交"),
            Self::Exit { code: None } => "ingress 异常退出，Codex 事件未提交".to_owned(),
            Self::Timeout => "ingress 提交超时，Codex 事件未提交".to_owned(),
            Self::Envelope(error) => format!("无法编码 Codex 事件：{error}"),
        }
    }
}

/// Hook 的时间上限；默认值用于生产，测试可缩短以覆盖超时路径。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HookLimits {
    pub upstream_timeout: Duration,
    pub ingress_timeout: Duration,
}

impl Default for HookLimits {
    fn default() -> Self {
        Self {
            upstream_timeout: UPSTREAM_TIMEOUT,
            ingress_timeout: INGRESS_TIMEOUT,
        }
    }
}

/// 进程边界：真实实现启动上游与 ingress，测试注入记录型实现。
#[async_trait]
pub trait HookPrograms: Send + Sync {
    /// 原样透传参数与 stdin 给上游；返回上游退出码。
    async fn run_upstream(&self, args: &[String], stdin: &[u8]) -> Result<i32, UpstreamFailure>;

    /// 把标准事件提交给 ingress。
    async fn run_ingress(&self, envelope: &[u8]) -> Result<(), IngressFailure>;

    /// 追加一条脱敏诊断；调用方吞掉错误，日志失败绝不影响推送与退出码。
    async fn log_diagnostic(&self, message: &str) -> io::Result<()>;
}

/// 真实进程实现：上游 codex-computer-use.exe + agentnotify-ingress.exe。
pub struct RealPrograms;

#[async_trait]
impl HookPrograms for RealPrograms {
    async fn run_upstream(&self, args: &[String], stdin: &[u8]) -> Result<i32, UpstreamFailure> {
        let upstream = resolve_upstream()?;
        let mut command = tokio::process::Command::new(&upstream);
        command
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(CREATE_NO_WINDOW);

        let mut child = command
            .spawn()
            .map_err(|error| UpstreamFailure::SpawnFailed(error.to_string()))?;
        if let Some(mut child_stdin) = child.stdin.take() {
            let payload = stdin.to_vec();
            // 上游可能不读 stdin：写入放到独立任务里，避免管道写满时卡住等待。
            tokio::spawn(async move {
                let _ = child_stdin.write_all(&payload).await;
            });
        }
        let status = child
            .wait()
            .await
            .map_err(|error| UpstreamFailure::WaitFailed(error.to_string()))?;
        status.code().ok_or(UpstreamFailure::NoExitCode)
    }

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

/// 执行顺序契约：先上游（受上限控制），后 ingress；ingress 失败只写诊断、不影响退出码。
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
    let forwarded = forward_args(args);
    let upstream_exit_code = match tokio::time::timeout(
        limits.upstream_timeout,
        programs.run_upstream(&forwarded, stdin),
    )
    .await
    {
        Ok(Ok(code)) => Some(code),
        Ok(Err(failure)) => {
            log(programs, &failure.message()).await;
            None
        }
        Err(_) => {
            log(
                programs,
                &format!(
                    "上游超时（上限 {:?}），已按上限终止",
                    limits.upstream_timeout
                ),
            )
            .await;
            None
        }
    };

    let ingress_failure = match build_envelope(args, stdin) {
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
        log(programs, &failure.message()).await;
    }

    HookOutcome {
        upstream_exit_code,
        ingress_failure,
    }
}

/// 诊断日志失败绝不影响推送路径与退出码。
async fn log(programs: &dyn HookPrograms, message: &str) {
    let _ = programs.log_diagnostic(message).await;
}

/// 兼容旧 AgentNotify 入口：消费首个 `codex` 选择参数，其余原样透传。
fn forward_args(args: &[String]) -> Vec<String> {
    let mut forwarded = args.to_vec();
    if forwarded
        .first()
        .is_some_and(|arg| arg.trim().eq_ignore_ascii_case(SELECTOR_ARGUMENT))
    {
        forwarded.remove(0);
    }
    forwarded
}

/// 生成标准 `agent.event`；事件 JSON 优先取参数里的对象，其次取 stdin，都没有时按空对象处理。
pub fn build_envelope(args: &[String], stdin: &[u8]) -> Result<Vec<u8>, String> {
    let payload = extract_payload(args, stdin).unwrap_or_else(|| serde_json::json!({}));
    let envelope = WireEnvelope {
        protocol_version: PROTOCOL_VERSION,
        kind: EVENT_KIND,
        request_id: uuid::Uuid::new_v4().to_string(),
        agent_id: AGENT_ID,
        payload: &payload,
    };
    serde_json::to_vec(&envelope).map_err(|error| format!("无法编码 Codex 事件：{error}"))
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

fn extract_payload(args: &[String], stdin: &[u8]) -> Option<serde_json::Value> {
    for arg in args {
        let trimmed = arg.trim_start();
        if !trimmed.starts_with('{') {
            continue;
        }
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
            if value.is_object() {
                return Some(value);
            }
        }
    }
    let value: serde_json::Value = serde_json::from_slice(stdin).ok()?;
    value.is_object().then_some(value)
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

/// 查找当前 Codex 使用的 `codex-computer-use.exe`：取 `cua_node` 下最新的运行时。
pub fn find_codex_computer_use_exe() -> Option<PathBuf> {
    let root = PathBuf::from(env::var_os(LOCAL_APP_DATA_ENV).filter(|value| !value.is_empty())?);
    find_codex_computer_use_in(&root)
}

/// 在给定 `%LOCALAPPDATA%` 根下发现上游，便于隔离测试。
pub fn find_codex_computer_use_in(local_app_data: &Path) -> Option<PathBuf> {
    let runtimes = local_app_data
        .join("OpenAI")
        .join("Codex")
        .join("runtimes")
        .join("cua_node");
    let entries = std::fs::read_dir(&runtimes).ok()?;

    let mut newest: Option<(SystemTime, PathBuf)> = None;
    for entry in entries.flatten() {
        let candidate = entry
            .path()
            .join("bin")
            .join("node_modules")
            .join("@oai")
            .join("sky")
            .join("bin")
            .join("windows")
            .join(CUA_EXE_NAME);
        let Ok(metadata) = std::fs::metadata(&candidate) else {
            continue;
        };
        if !metadata.is_file() {
            continue;
        }
        let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        if newest.as_ref().is_none_or(|(best, _)| modified > *best) {
            newest = Some((modified, candidate));
        }
    }
    newest.map(|(_, path)| path)
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

/// 上游解析：显式配置了 `AGENT_NOTIFY_CODEX_UPSTREAM` 就只认它（缺失即报错，不猜测回退），
/// 否则动态发现当前 Codex 使用的 codex-computer-use.exe。
fn resolve_upstream() -> Result<PathBuf, UpstreamFailure> {
    if let Some(configured) = env::var_os(UPSTREAM_ENV).filter(|value| !value.is_empty()) {
        let path = PathBuf::from(configured);
        return if path.is_file() {
            Ok(path)
        } else {
            Err(UpstreamFailure::NotFound)
        };
    }
    find_codex_computer_use_exe().ok_or(UpstreamFailure::NotFound)
}

/// 诊断日志路径：`AGENT_NOTIFY_TEMP_DIR` 优先，其次 `%TEMP%\agent-notify\codex-notify-debug.log`。
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
