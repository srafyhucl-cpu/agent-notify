use std::{
    env, io,
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::{Duration, SystemTime},
};

use agentnotify_agent_sdk::AgentError;
use agentnotify_domain::SafeError;
use async_trait::async_trait;

/// Codex queue 的默认确认上限：30 秒未确认按 Unknown 处理，不自动重试。
pub const DEFAULT_QUEUE_TIMEOUT: Duration = Duration::from_secs(30);
const CODEX_BINARY_ENV: &str = "AGENT_NOTIFY_CODEX_BIN";
const CODEX_BINARY_NAME: &str = "codex.exe";
const CODEX_BINARY_NAME_PLAIN: &str = "codex";
const CODEX_INSTALL_SUBDIR: [&str; 3] = ["OpenAI", "Codex", "bin"];
const LOCAL_APP_DATA_ENV: &str = "LOCALAPPDATA";
/// 错误详情只保留前若干字符，避免把整段 CLI 输出塞进通知。
const MAX_ERROR_DETAIL_CHARS: usize = 300;
/// 归档提示里最多回显的线程 ID 字符数，防止超出 SafeError 长度上限。
const MAX_THREAD_ID_IN_MESSAGE_CHARS: usize = 64;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// 命令执行结果；`status` 为 None 表示进程没有正常退出码。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandOutput {
    pub status: Option<i32>,
    pub output: String,
}

/// 进程边界：真实实现启动 codex.exe，测试注入记录型实现。
#[async_trait]
pub trait CommandExecutor: Send + Sync {
    /// 执行命令并返回合并输出；超时与终止由调用方负责。
    async fn execute(&self, binary: &Path, args: &[String]) -> io::Result<CommandOutput>;
}

pub struct TokioCommandExecutor;

#[async_trait]
impl CommandExecutor for TokioCommandExecutor {
    async fn execute(&self, binary: &Path, args: &[String]) -> io::Result<CommandOutput> {
        let mut command = tokio::process::Command::new(binary);
        command.args(args).stdin(Stdio::null());
        #[cfg(windows)]
        command.creation_flags(CREATE_NO_WINDOW);
        let output = command.output().await?;

        let mut combined = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stderr.trim().is_empty() {
            if !combined.is_empty() {
                combined.push('\n');
            }
            combined.push_str(&stderr);
        }
        Ok(CommandOutput {
            status: output.status.code(),
            output: combined,
        })
    }
}

/// 精确入队边界：只执行 `codex queue --thread=<id> --message=<text>`，不经过 shell。
#[derive(Clone)]
pub struct CodexQueue {
    binary: Option<PathBuf>,
    timeout: Duration,
    executor: Arc<dyn CommandExecutor>,
}

impl CodexQueue {
    pub fn from_default_location() -> Self {
        Self {
            binary: None,
            timeout: DEFAULT_QUEUE_TIMEOUT,
            executor: Arc::new(TokioCommandExecutor),
        }
    }

    pub fn with_binary(binary: impl Into<PathBuf>) -> Self {
        Self {
            binary: Some(binary.into()),
            ..Self::from_default_location()
        }
    }

    /// 注入自定义执行器（测试与诊断用），避免真实启动 codex。
    pub fn with_executor(mut self, executor: Arc<dyn CommandExecutor>) -> Self {
        self.executor = executor;
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub async fn enqueue(&self, thread_id: &str, text: &str) -> Result<(), AgentError> {
        let thread_id = thread_id.trim();
        let text = text.trim();
        if thread_id.is_empty() || text.is_empty() {
            return Err(AgentError::InvalidInput);
        }

        let binary = self.resolve_binary().ok_or_else(|| {
            AgentError::Unavailable(safe_error(
                "codex_binary_missing",
                "找不到 codex 命令，请确认 Codex CLI 已安装",
            ))
        })?;
        let args = queue_args(thread_id, text);

        match tokio::time::timeout(self.timeout, self.executor.execute(&binary, &args)).await {
            Err(_) => Err(AgentError::Unknown(safe_error(
                "codex_queue_unconfirmed",
                "Codex 未在 30 秒内确认引用回复，任务不会自动重试",
            ))),
            Ok(Err(_)) => Err(AgentError::Unavailable(safe_error(
                "codex_queue_spawn_failed",
                "无法启动 codex queue，请确认 Codex CLI 可用后重试",
            ))),
            Ok(Ok(output)) if output.status == Some(0) => Ok(()),
            Ok(Ok(output)) => Err(queue_failure(thread_id, &output.output)),
        }
    }

    fn resolve_binary(&self) -> Option<PathBuf> {
        if let Some(binary) = &self.binary {
            return Some(binary.clone());
        }
        if let Some(configured) = env::var_os(CODEX_BINARY_ENV).filter(|value| !value.is_empty()) {
            return Some(PathBuf::from(configured));
        }
        if let Some(found) =
            find_on_path(CODEX_BINARY_NAME).or_else(|| find_on_path(CODEX_BINARY_NAME_PLAIN))
        {
            return Some(found);
        }
        find_installed_cli()
    }
}

/// 使用参数数组传递线程与正文：正文里带引号、`$` 或 `--` 也不会被 shell 解释。
pub fn queue_args(thread_id: &str, text: &str) -> Vec<String> {
    vec![
        "queue".to_owned(),
        format!("--thread={}", thread_id.trim()),
        format!("--message={}", text.trim()),
    ]
}

/// 把 `codex queue` 的失败文本映射成用户能照做的错误，绝不回退到“最近会话”。
fn queue_failure(thread_id: &str, output: &str) -> AgentError {
    let detail = compact_detail(output);
    let lower = detail.to_lowercase();

    if lower.contains("no active session found") {
        return failed(
            "codex_queue_target_missing",
            "Codex 未找到可续聊的目标线程：请确认线程 ID 正确，并更新 Codex 后重试",
        );
    }
    if lower.contains("no rollout found for thread id") || lower.contains("thread not found") {
        return failed(
            "codex_thread_not_found",
            "目标 Codex 线程不存在或已删除：请在 Codex 中确认该线程后重试",
        );
    }
    if lower.contains("ephemeral thread does not support queued submissions") {
        return failed(
            "codex_thread_ephemeral",
            "目标 Codex 会话是临时会话，不支持引用续聊",
        );
    }
    if lower.contains("user message queue is unavailable") {
        return failed(
            "codex_queue_unavailable",
            "当前 Codex 未启用可持久化的消息队列，无法引用续聊：请更新 Codex 并重启会话后重试",
        );
    }
    if lower.contains("cannot queue through an embedded app server") {
        return failed(
            "codex_app_server_conflict",
            "Codex 本地服务状态冲突：请重启 Codex 后重新引用通知回复",
        );
    }
    if lower.contains("invalid thread id") {
        return failed(
            "codex_thread_id_invalid",
            "目标 Codex 线程 ID 无效，无法续聊",
        );
    }
    if lower.contains("is archived") {
        // 线程 ID 来自事件本身，截断后再拼进错误信息，避免超出 SafeError 长度上限。
        let short_thread_id: String = thread_id
            .chars()
            .take(MAX_THREAD_ID_IN_MESSAGE_CHARS)
            .collect();
        return failed(
            "codex_thread_archived",
            &format!(
                "目标 Codex 线程已归档：请先在 Codex 中恢复（解档），或运行 codex unarchive {short_thread_id} 后重新引用回复"
            ),
        );
    }
    if lower.contains("does not support thread/queue/add") {
        return failed(
            "codex_queue_unsupported",
            "当前 Codex 会话不支持 queue：请更新 Codex 并重启会话后重试",
        );
    }
    if detail.is_empty() {
        return failed(
            "codex_queue_failed",
            "codex queue 失败，请确认 Codex CLI 可用后重试",
        );
    }
    failed("codex_queue_failed", &format!("codex queue 失败：{detail}"))
}

fn compact_detail(output: &str) -> String {
    output
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(MAX_ERROR_DETAIL_CHARS)
        .collect()
}

fn failed(code: &str, message: &str) -> AgentError {
    AgentError::Failed(safe_error(code, message))
}

fn safe_error(code: &str, message: &str) -> SafeError {
    SafeError::new(code, message).expect("Codex 错误常量必须是有效安全错误")
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    env::split_paths(&path)
        .map(|directory| directory.join(name))
        .find(|candidate| candidate.is_file())
}

/// 在 `%LOCALAPPDATA%\OpenAI\Codex\bin` 下按修改时间取最新的 codex CLI。
fn find_installed_cli() -> Option<PathBuf> {
    let root = PathBuf::from(env::var_os(LOCAL_APP_DATA_ENV).filter(|value| !value.is_empty())?)
        .join(CODEX_INSTALL_SUBDIR[0])
        .join(CODEX_INSTALL_SUBDIR[1])
        .join(CODEX_INSTALL_SUBDIR[2]);

    let mut candidates = Vec::new();
    for name in [CODEX_BINARY_NAME, CODEX_BINARY_NAME_PLAIN] {
        push_candidate(&mut candidates, root.join(name));
    }
    if let Ok(entries) = std::fs::read_dir(&root) {
        for entry in entries.flatten() {
            if !entry.path().is_dir() {
                continue;
            }
            for name in [CODEX_BINARY_NAME, CODEX_BINARY_NAME_PLAIN] {
                push_candidate(&mut candidates, entry.path().join(name));
            }
        }
    }
    candidates.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    candidates.into_iter().next().map(|(path, _)| path)
}

fn push_candidate(candidates: &mut Vec<(PathBuf, SystemTime)>, path: PathBuf) {
    let Ok(metadata) = std::fs::metadata(&path) else {
        return;
    };
    if !metadata.is_file() {
        return;
    }
    candidates.push((path, metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH)));
}
