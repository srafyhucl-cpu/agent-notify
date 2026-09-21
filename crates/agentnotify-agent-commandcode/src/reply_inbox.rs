use std::{
    env,
    path::{Path, PathBuf},
    time::Duration,
};

use agentnotify_agent_sdk::{AgentError, AgentHealth};
use agentnotify_domain::{AgentSessionId, SafeError, Timestamp};
use serde::{Deserialize, Serialize};

const REPLY_DIR_ENV: &str = "AGENT_NOTIFY_COMMANDCODE_REPLY_DIR";
const USER_PROFILE_ENV: &str = "USERPROFILE";
const HOME_ENV: &str = "HOME";
const CONFIG_SUBDIR: [&str; 2] = [".config", "agent-notify"];
const REPLY_DIR_NAME: &str = "commandcode-reply-inbox";
const HEARTBEAT_DIR: &str = "heartbeats";
const PENDING_DIR: &str = "pending";
const PROCESSING_DIR: &str = "processing";
const RESULT_DIR: &str = "results";
/// mod 每 5 秒写一次心跳；超过该时长视为离线（与 Go 版一致）。
const HEARTBEAT_MAX_AGE_SECONDS: i64 = 30;
const HEARTBEAT_FUTURE_SKEW_SECONDS: i64 = 5;
const RESULT_POLL_INTERVAL: Duration = Duration::from_millis(100);
/// 同步等待 mod 确认的上限；超时按 Unknown 处理，不自动重试（与 Go 版 10 秒一致）。
pub const DEFAULT_RESULT_WAIT: Duration = Duration::from_secs(10);
/// 任务有效期，与 Go 版 `spoolJobTTL` 一致。
pub const JOB_TTL: time::Duration = time::Duration::minutes(10);
/// 结果详情上限（按字节并在字符边界截断），保证仍能塞进 SafeError 的长度上限。
const MAX_RESULT_ERROR_BYTES: usize = 300;

/// 目标会话的 mod 状态；每个非 Ready 状态都有独立错误码，绝不回退到别的会话。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommandCodeInboxState {
    /// 目标会话的 mod 在线且回复窗口正开着。
    Ready,
    /// 目标会话在线，但回复窗口已过（通知发出后的等待时间结束）。
    WindowClosed,
    /// mod 在线，但当前 Command Code 不支持会话注入。
    Unsupported,
    /// 目标会话没有在运行的 mod（只有别的会话，或心跳已过期）。
    SessionNotRunning,
    /// 收件箱或心跳目录不存在：mod 没安装或没加载。
    NotRunning,
    /// 心跳文件不可读或时间无效。
    Invalid,
}

/// mod 是否已加载；只用于接入状态展示，不参与回复路由。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommandCodeModState {
    Loaded,
    NotRunning,
    Invalid,
}

/// 投递给 Command Code mod 的持久化回复任务；`session_id` 是唯一的认领依据。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandCodeReplyJob {
    pub id: String,
    pub session_id: AgentSessionId,
    pub text: String,
    pub created_at: Timestamp,
    pub expires_at: Timestamp,
}

/// Command Code 回复收件箱：mod 是唯一执行方，这里只负责投递与等待结果。
#[derive(Clone, Debug)]
pub struct CommandCodeReplyInbox {
    root: Option<PathBuf>,
}

impl CommandCodeReplyInbox {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: Some(root.into()),
        }
    }

    /// `AGENT_NOTIFY_COMMANDCODE_REPLY_DIR` 优先，其次
    /// `%USERPROFILE%\.config\agent-notify\commandcode-reply-inbox`（与 Go 版一致）。
    pub fn from_default_location() -> Self {
        if let Some(configured) = non_empty_env(REPLY_DIR_ENV) {
            return Self::new(configured);
        }
        let Some(home) = non_empty_env(USER_PROFILE_ENV).or_else(|| non_empty_env(HOME_ENV)) else {
            return Self { root: None };
        };
        let mut root = home;
        for part in CONFIG_SUBDIR {
            root.push(part);
        }
        root.push(REPLY_DIR_NAME);
        Self::new(root)
    }

    /// 占位实现：不读盘、不写盘，只用于测试隔离。
    pub fn without_backend() -> Self {
        Self { root: None }
    }

    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    /// 按目标会话判断回复窗口是否可用；会话不匹配或窗口已关都明确失败。
    pub async fn inspect_state(&self, session_id: &str) -> CommandCodeInboxState {
        match self.inspect_inner(session_id).await {
            Ok(state) => state,
            Err(_) => CommandCodeInboxState::Invalid,
        }
    }

    /// 只关心 mod 是否加载；不参与回复路由。
    pub async fn inspect_mod_state(&self) -> CommandCodeModState {
        match self.inspect_mod_inner().await {
            Ok(state) => state,
            Err(_) => CommandCodeModState::Invalid,
        }
    }

    /// 目标会话不可回复时立即给出用户可读的原因，不写任何任务。
    pub async fn ensure_ready(&self, session_id: &str) -> Result<(), AgentError> {
        let state = self.inspect_state(session_id).await;
        if state == CommandCodeInboxState::Ready {
            return Ok(());
        }
        Err(state_error(state))
    }

    /// 收件箱只负责投递；回复窗口开关由适配器在调用前判定。
    pub async fn resume(&self, session_id: &AgentSessionId, text: &str) -> Result<(), AgentError> {
        self.resume_with_timeout(session_id, text, DEFAULT_RESULT_WAIT)
            .await
    }

    pub async fn resume_with_timeout(
        &self,
        session_id: &AgentSessionId,
        text: &str,
        timeout: Duration,
    ) -> Result<(), AgentError> {
        let text = text.trim();
        if text.is_empty() {
            return Err(AgentError::InvalidInput);
        }
        self.ensure_ready(session_id.as_str()).await?;
        self.ensure_layout().await?;

        let now = Timestamp::now_utc();
        let expires_at = now.checked_add(JOB_TTL).ok_or_else(|| {
            failed(
                "commandcode_job_time_invalid",
                "引用回复任务时间无效，请重新发送",
            )
        })?;
        let job = CommandCodeReplyJob {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session_id.clone(),
            text: text.to_owned(),
            created_at: now,
            expires_at,
        };
        self.write_job_atomic(&job).await?;
        self.wait_for_result(&job.id, timeout).await
    }

    pub async fn pending_count(&self) -> usize {
        match self.root() {
            Some(root) => count_json_files(root.join(PENDING_DIR)).await,
            None => 0,
        }
    }

    pub async fn processing_count(&self) -> usize {
        match self.root() {
            Some(root) => count_json_files(root.join(PROCESSING_DIR)).await,
            None => 0,
        }
    }

    async fn inspect_inner(
        &self,
        session_id: &str,
    ) -> Result<CommandCodeInboxState, std::io::Error> {
        let Some(root) = self.root.as_deref() else {
            // 没有配置路径时按“mod 未运行”暴露，绝不猜测用户目录。
            return Ok(CommandCodeInboxState::NotRunning);
        };
        let target = session_id.trim();
        let mut entries = match tokio::fs::read_dir(root.join(HEARTBEAT_DIR)).await {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(CommandCodeInboxState::NotRunning);
            }
            Err(error) => return Err(error),
        };

        let now = time::OffsetDateTime::now_utc();
        let mut saw_unsupported = false;
        let mut saw_window_closed = false;
        let mut saw_stale = false;
        let mut saw_invalid = false;
        let mut saw_other_session = false;
        while let Some(entry) = entries.next_entry().await? {
            if !entry.file_type().await?.is_file()
                || entry.path().extension().and_then(|value| value.to_str()) != Some("json")
            {
                continue;
            }
            let Ok(bytes) = tokio::fs::read(entry.path()).await else {
                continue;
            };
            let Ok(heartbeat) = serde_json::from_slice::<Heartbeat>(&bytes) else {
                continue;
            };
            // 会话号对不上说明这条心跳属于别的会话：只记录，绝不改投。
            if heartbeat.session_id.trim() != target {
                saw_other_session = true;
                continue;
            }
            if !heartbeat_time_is_valid(&heartbeat.timestamp, now) {
                saw_invalid = true;
                continue;
            }
            if !heartbeat_is_fresh(&heartbeat.timestamp, now) {
                saw_stale = true;
                continue;
            }
            if !heartbeat.ready {
                saw_unsupported = true;
                continue;
            }
            if !heartbeat.window_open {
                saw_window_closed = true;
                continue;
            }
            return Ok(CommandCodeInboxState::Ready);
        }

        // 必须扫完整个目录再判定：进程重启会留下僵死心跳，提前返回会把存活实例误判成离线。
        Ok(if saw_unsupported {
            CommandCodeInboxState::Unsupported
        } else if saw_window_closed {
            CommandCodeInboxState::WindowClosed
        } else if saw_stale {
            CommandCodeInboxState::SessionNotRunning
        } else if saw_invalid {
            CommandCodeInboxState::Invalid
        } else if saw_other_session {
            CommandCodeInboxState::SessionNotRunning
        } else {
            CommandCodeInboxState::NotRunning
        })
    }

    async fn inspect_mod_inner(&self) -> Result<CommandCodeModState, std::io::Error> {
        let Some(root) = self.root.as_deref() else {
            return Ok(CommandCodeModState::NotRunning);
        };
        let mut entries = match tokio::fs::read_dir(root.join(HEARTBEAT_DIR)).await {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(CommandCodeModState::NotRunning);
            }
            Err(error) => return Err(error),
        };

        let now = time::OffsetDateTime::now_utc();
        while let Some(entry) = entries.next_entry().await? {
            if !entry.file_type().await?.is_file()
                || entry.path().extension().and_then(|value| value.to_str()) != Some("json")
            {
                continue;
            }
            let Ok(bytes) = tokio::fs::read(entry.path()).await else {
                continue;
            };
            let Ok(heartbeat) = serde_json::from_slice::<Heartbeat>(&bytes) else {
                continue;
            };
            if heartbeat_time_is_valid(&heartbeat.timestamp, now)
                && heartbeat_is_fresh(&heartbeat.timestamp, now)
            {
                return Ok(CommandCodeModState::Loaded);
            }
        }
        Ok(CommandCodeModState::NotRunning)
    }

    async fn ensure_layout(&self) -> Result<(), AgentError> {
        let root = self
            .root
            .as_deref()
            .ok_or_else(|| state_error(CommandCodeInboxState::NotRunning))?;
        for directory in [
            root.to_path_buf(),
            root.join(PENDING_DIR),
            root.join(PROCESSING_DIR),
            root.join(RESULT_DIR),
            root.join(HEARTBEAT_DIR),
        ] {
            tokio::fs::create_dir_all(&directory).await.map_err(|_| {
                failed(
                    "commandcode_inbox_unwritable",
                    "无法创建 Command Code 回复收件箱，请检查目录权限",
                )
            })?;
        }
        Ok(())
    }

    async fn write_job_atomic(&self, job: &CommandCodeReplyJob) -> Result<(), AgentError> {
        let root = self
            .root
            .as_deref()
            .ok_or_else(|| state_error(CommandCodeInboxState::NotRunning))?;
        let destination = root.join(PENDING_DIR).join(format!("{}.json", job.id));
        let temporary = root.join(format!(".{}.{}.tmp", job.id, uuid::Uuid::new_v4()));
        let bytes = serde_json::to_vec(&WireJob::from(job)).map_err(|_| {
            failed(
                "commandcode_job_encode_failed",
                "无法编码 Command Code 引用回复任务",
            )
        })?;

        let temporary_for_write = temporary.clone();
        tokio::task::spawn_blocking(move || -> std::io::Result<()> {
            use std::{fs::File, io::Write};
            let mut file = File::create(&temporary_for_write)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            drop(file);
            std::fs::rename(&temporary_for_write, &destination)?;
            Ok(())
        })
        .await
        .map_err(|_| {
            AgentError::Unknown(safe_error(
                "commandcode_job_write_unknown",
                "Command Code 回复任务写入结果无法确认，未自动重试",
            ))
        })?
        .map_err(|_| {
            AgentError::Unknown(safe_error(
                "commandcode_job_write_unknown",
                "Command Code 回复任务写入结果无法确认，未自动重试",
            ))
        })?;
        Ok(())
    }

    async fn wait_for_result(&self, job_id: &str, timeout: Duration) -> Result<(), AgentError> {
        let root = self
            .root
            .as_deref()
            .ok_or_else(|| state_error(CommandCodeInboxState::NotRunning))?;
        let result_path = root.join(RESULT_DIR).join(format!("{job_id}.json"));
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            match tokio::fs::read(&result_path).await {
                Ok(bytes) => {
                    let result: WireResult = serde_json::from_slice(&bytes).map_err(|_| {
                        failed(
                            "commandcode_reply_result_invalid",
                            "Command Code mod 返回了无效结果，请重试",
                        )
                    })?;
                    if result.ok {
                        return Ok(());
                    }
                    return Err(result_error(
                        result.code.as_deref().unwrap_or_default(),
                        result.error.as_deref().unwrap_or_default(),
                    ));
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => {
                    return Err(AgentError::Unknown(safe_error(
                        "commandcode_reply_result_unreadable",
                        "无法读取 Command Code 引用回复结果，未自动重试",
                    )));
                }
            }

            if tokio::time::Instant::now() >= deadline {
                return Err(AgentError::Unknown(safe_error(
                    "commandcode_reply_unconfirmed",
                    "Command Code mod 未在等待时间内确认引用回复，任务不会自动重试，请稍后在会话中确认结果",
                )));
            }
            tokio::time::sleep(RESULT_POLL_INTERVAL.min(timeout)).await;
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Heartbeat {
    ready: bool,
    timestamp: String,
    /// mod 尚未绑定会话时为空串，等同于“不是目标会话”。
    #[serde(default)]
    session_id: String,
    #[serde(default)]
    window_open: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WireJob<'a> {
    id: &'a str,
    // mod（plugin/commandcode-v2/agent-notify.ts）按 OpenCode/Devin 的既有约定读取
    // `sessionID`；camelCase 会写成 `sessionId`，mod 会判定“任务字段不完整”并拒绝执行。
    #[serde(rename = "sessionID")]
    session_id: &'a str,
    text: &'a str,
    created_at: String,
    expires_at: String,
}

impl<'a> From<&'a CommandCodeReplyJob> for WireJob<'a> {
    fn from(job: &'a CommandCodeReplyJob) -> Self {
        Self {
            id: &job.id,
            session_id: job.session_id.as_str(),
            text: &job.text,
            created_at: job.created_at.to_rfc3339(),
            expires_at: job.expires_at.to_rfc3339(),
        }
    }
}

#[derive(Deserialize)]
struct WireResult {
    ok: bool,
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

/// 把 mod 返回的稳定错误码映射成微信可读提示；未知细节只做脱敏限长，不回显回复正文。
fn result_error(code: &str, detail: &str) -> AgentError {
    let detail = sanitize_result_error(detail);
    match code.trim().to_ascii_lowercase().as_str() {
        "window_closed" => failed(
            "commandcode_reply_window_expired",
            "Command Code 回复窗口已过，请等下一次通知发出后再引用回复；回复不会留到下一次运行",
        ),
        "invalid_job" => failed(
            "commandcode_reply_invalid_job",
            "Command Code 引用回复任务无效，请重新引用原通知后再试",
        ),
        "inject_failed" => {
            if detail.is_empty() {
                failed(
                    "commandcode_inject_failed",
                    "Command Code 会话注入失败，请确认目标会话仍在运行后重试",
                )
            } else {
                failed(
                    "commandcode_inject_failed",
                    &format!("Command Code 会话注入失败：{detail}"),
                )
            }
        }
        _ if !detail.is_empty() => failed(
            "commandcode_reply_failed",
            &format!("Command Code 引用回复失败：{detail}"),
        ),
        _ => failed(
            "commandcode_reply_failed",
            "Command Code 引用回复失败，请确认目标会话仍在运行后重试",
        ),
    }
}

/// 每个状态都给用户可执行的下一步，不暴露内部路径。
fn state_error(state: CommandCodeInboxState) -> AgentError {
    match state {
        CommandCodeInboxState::Ready => AgentError::Unknown(safe_error(
            "commandcode_inbox_ready",
            "Command Code 引用回复已就绪",
        )),
        CommandCodeInboxState::WindowClosed => failed(
            "commandcode_reply_window_expired",
            "Command Code 回复窗口已过，请等下一次通知发出后再引用回复；回复不会留到下一次运行",
        ),
        CommandCodeInboxState::Unsupported => unavailable(
            "commandcode_inject_unsupported",
            "当前 Command Code 版本不支持会话注入，请更新 Command Code 后重启",
        ),
        CommandCodeInboxState::SessionNotRunning => unavailable(
            "commandcode_session_not_running",
            "Command Code 目标会话未在运行，请先打开该会话；回复不会改投到其他会话",
        ),
        CommandCodeInboxState::NotRunning => unavailable(
            "commandcode_mod_not_running",
            "未检测到 Command Code 的 AgentNotify mod，请运行安装器并重启 Command Code",
        ),
        CommandCodeInboxState::Invalid => unavailable(
            "commandcode_heartbeat_invalid",
            "Command Code mod 心跳无效，请检查回复收件箱目录权限后重启 Command Code",
        ),
    }
}

pub fn health_for(state: CommandCodeModState) -> AgentHealth {
    match state {
        CommandCodeModState::Loaded => AgentHealth::healthy(),
        CommandCodeModState::NotRunning => AgentHealth::unavailable(safe_error(
            "commandcode_mod_not_found",
            "未检测到 Command Code mod，请运行安装器并重启 Command Code",
        )),
        CommandCodeModState::Invalid => AgentHealth::unavailable(safe_error(
            "commandcode_inbox_unreadable",
            "Command Code 回复收件箱不可读，请检查目录权限",
        )),
    }
}

fn heartbeat_time_is_valid(timestamp: &str, now: time::OffsetDateTime) -> bool {
    parse_timestamp(timestamp).is_some_and(|timestamp| {
        timestamp <= now + time::Duration::seconds(HEARTBEAT_FUTURE_SKEW_SECONDS)
    })
}

fn heartbeat_is_fresh(timestamp: &str, now: time::OffsetDateTime) -> bool {
    parse_timestamp(timestamp).is_some_and(|timestamp| {
        timestamp >= now - time::Duration::seconds(HEARTBEAT_MAX_AGE_SECONDS)
    })
}

fn parse_timestamp(value: &str) -> Option<time::OffsetDateTime> {
    time::OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339).ok()
}

fn sanitize_result_error(value: &str) -> String {
    let collapsed = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut detail = String::new();
    for character in collapsed.chars() {
        if detail.len() + character.len_utf8() > MAX_RESULT_ERROR_BYTES {
            break;
        }
        detail.push(character);
    }
    detail
}

async fn count_json_files(directory: PathBuf) -> usize {
    let Ok(mut entries) = tokio::fs::read_dir(directory).await else {
        return 0;
    };
    let mut count = 0;
    while let Ok(Some(entry)) = entries.next_entry().await {
        if entry
            .path()
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("json"))
        {
            count += 1;
        }
    }
    count
}

fn non_empty_env(name: &str) -> Option<PathBuf> {
    env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn unavailable(code: &str, message: &str) -> AgentError {
    AgentError::Unavailable(safe_error(code, message))
}

fn failed(code: &str, message: &str) -> AgentError {
    AgentError::Failed(safe_error(code, message))
}

fn safe_error(code: &str, message: &str) -> SafeError {
    SafeError::new(code, message).expect("Command Code 错误常量必须是有效安全错误")
}
