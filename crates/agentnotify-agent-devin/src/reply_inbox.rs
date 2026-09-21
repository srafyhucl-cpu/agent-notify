use std::{
    env,
    path::{Path, PathBuf},
    time::Duration,
};

use agentnotify_agent_sdk::AgentError;
use agentnotify_domain::{AgentSessionId, SafeError, Timestamp};
use serde::{Deserialize, Serialize};

const REPLY_DIR_ENV: &str = "AGENT_NOTIFY_DEVIN_REPLY_DIR";
const USER_PROFILE_ENV: &str = "USERPROFILE";
const HOME_ENV: &str = "HOME";
const CONFIG_DIR: [&str; 2] = [".config", "agent-notify"];
const REPLY_DIR_NAME: &str = "devin-reply-inbox";
const HEARTBEAT_DIR: &str = "heartbeats";
const PENDING_DIR: &str = "pending";
const PROCESSING_DIR: &str = "processing";
const RESULT_DIR: &str = "results";
/// 扩展每 5 秒写一次心跳；超过该时长视为离线（与 Go 版一致）。
const HEARTBEAT_MAX_AGE_SECONDS: i64 = 30;
const HEARTBEAT_FUTURE_SKEW_SECONDS: i64 = 5;
const RESULT_POLL_INTERVAL: Duration = Duration::from_millis(100);
/// 同步等待扩展确认的上限；超时按 Unknown 处理，不自动重试（与 Go 版 10 秒一致）。
pub const DEFAULT_RESULT_WAIT: Duration = Duration::from_secs(10);
/// 任务有效期，与 Go 版 `spoolJobTTL` 一致。
pub const JOB_TTL: time::Duration = time::Duration::minutes(10);
/// 结果详情上限（按字节并在字符边界截断），保证仍能塞进 SafeError 的长度上限。
const MAX_RESULT_ERROR_BYTES: usize = 256;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DevinInboxState {
    Ready,
    /// 扩展在线，但本机桌面端没有精确回复能力。
    Unsupported,
    /// 心跳过期：扩展运行过，但当前不可用。
    Offline,
    /// 收件箱或心跳目录不存在：扩展没安装或没加载。
    NotRunning,
    Error,
}

/// 投递给 Devin 扩展的持久化回复任务；`target_id` 是桌面端 Cascade 标识。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DevinReplyJob {
    pub id: String,
    pub session_id: AgentSessionId,
    pub target_id: String,
    pub text: String,
    pub created_at: Timestamp,
    pub expires_at: Timestamp,
}

/// Devin 回复收件箱：扩展是唯一执行方，这里只负责投递与等待结果。
#[derive(Clone, Debug)]
pub struct DevinReplyInbox {
    root: Option<PathBuf>,
}

impl DevinReplyInbox {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: Some(root.into()),
        }
    }

    /// `AGENT_NOTIFY_DEVIN_REPLY_DIR` 优先，其次 `%USERPROFILE%\.config\agent-notify\devin-reply-inbox`。
    pub fn from_default_location() -> Self {
        if let Some(configured) = non_empty_env(REPLY_DIR_ENV) {
            return Self::new(configured);
        }
        let Some(home) = non_empty_env(USER_PROFILE_ENV).or_else(|| non_empty_env(HOME_ENV)) else {
            return Self { root: None };
        };
        let mut root = home;
        for part in CONFIG_DIR {
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

    pub async fn inspect_state(&self) -> DevinInboxState {
        match self.inspect_inner().await {
            Ok(state) => state,
            Err(_) => DevinInboxState::Error,
        }
    }

    /// 扩展不在线时立即给出用户可读的原因，不写任何任务。
    pub async fn ensure_ready(&self) -> Result<(), AgentError> {
        let state = self.inspect_state().await;
        if state == DevinInboxState::Ready {
            return Ok(());
        }
        Err(state_error(state))
    }

    pub async fn resume(
        &self,
        session_id: &AgentSessionId,
        target_id: &str,
        text: &str,
    ) -> Result<(), AgentError> {
        self.resume_with_timeout(session_id, target_id, text, DEFAULT_RESULT_WAIT)
            .await
    }

    pub async fn resume_with_timeout(
        &self,
        session_id: &AgentSessionId,
        target_id: &str,
        text: &str,
        timeout: Duration,
    ) -> Result<(), AgentError> {
        let text = text.trim();
        if text.is_empty() {
            return Err(AgentError::InvalidInput);
        }
        let target_id = target_id.trim();
        if target_id.is_empty() {
            return Err(failed(
                "devin_reply_target_missing",
                "引用回复缺少精确会话标识，请重新引用原通知后再试；回复不会改投到最近会话",
            ));
        }
        self.ensure_ready().await?;
        self.ensure_layout().await?;

        let now = Timestamp::now_utc();
        let expires_at = now
            .checked_add(JOB_TTL)
            .ok_or_else(|| failed("devin_job_time_invalid", "引用回复任务时间无效，请重新发送"))?;
        let job = DevinReplyJob {
            // 扩展只接受 32 位十六进制任务 ID，因此用无连字符 UUID。
            id: uuid::Uuid::new_v4().simple().to_string(),
            session_id: session_id.clone(),
            target_id: target_id.to_owned(),
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

    async fn inspect_inner(&self) -> Result<DevinInboxState, std::io::Error> {
        let Some(root) = self.root.as_deref() else {
            // 没有配置路径时按“扩展未运行”暴露，绝不猜测用户目录。
            return Ok(DevinInboxState::NotRunning);
        };
        let mut entries = match tokio::fs::read_dir(root.join(HEARTBEAT_DIR)).await {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(DevinInboxState::NotRunning);
            }
            Err(error) => return Err(error),
        };

        let now = time::OffsetDateTime::now_utc();
        let mut saw_unsupported = false;
        let mut saw_offline = false;
        let mut saw_invalid = false;
        while let Some(entry) = entries.next_entry().await? {
            if !entry.file_type().await?.is_file()
                || entry.path().extension().and_then(|value| value.to_str()) != Some("json")
            {
                continue;
            }
            let bytes = match tokio::fs::read(entry.path()).await {
                Ok(bytes) => bytes,
                Err(_) => {
                    saw_invalid = true;
                    continue;
                }
            };
            let Ok(heartbeat) = serde_json::from_slice::<Heartbeat>(&bytes) else {
                saw_invalid = true;
                continue;
            };
            let fresh = heartbeat_is_fresh(&heartbeat.timestamp, now);
            if heartbeat.ready {
                if fresh {
                    return Ok(DevinInboxState::Ready);
                }
                saw_offline = true;
            } else if fresh {
                saw_unsupported = true;
            }
        }

        if saw_unsupported {
            return Ok(DevinInboxState::Unsupported);
        }
        if saw_offline {
            return Ok(DevinInboxState::Offline);
        }
        if saw_invalid {
            return Ok(DevinInboxState::Error);
        }
        // 心跳目录存在但没有可用心跳：扩展已安装但当前没有实例在线。
        Ok(DevinInboxState::Offline)
    }

    async fn ensure_layout(&self) -> Result<(), AgentError> {
        let root = self
            .root
            .as_deref()
            .ok_or_else(|| state_error(DevinInboxState::NotRunning))?;
        for directory in [
            root.to_path_buf(),
            root.join(PENDING_DIR),
            root.join(PROCESSING_DIR),
            root.join(RESULT_DIR),
            root.join(HEARTBEAT_DIR),
        ] {
            tokio::fs::create_dir_all(&directory).await.map_err(|_| {
                failed(
                    "devin_inbox_unwritable",
                    "无法创建 Devin 回复收件箱，请检查目录权限",
                )
            })?;
        }
        Ok(())
    }

    async fn write_job_atomic(&self, job: &DevinReplyJob) -> Result<(), AgentError> {
        let root = self
            .root
            .as_deref()
            .ok_or_else(|| state_error(DevinInboxState::NotRunning))?;
        let destination = root.join(PENDING_DIR).join(format!("{}.json", job.id));
        let temporary = root.join(format!(".{}.{}.tmp", job.id, uuid::Uuid::new_v4()));
        let bytes = serde_json::to_vec(&WireJob::from(job))
            .map_err(|_| failed("devin_job_encode_failed", "无法编码 Devin 引用回复任务"))?;

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
                "devin_job_write_unknown",
                "Devin 回复任务写入结果无法确认，未自动重试",
            ))
        })?
        .map_err(|_| {
            AgentError::Unknown(safe_error(
                "devin_job_write_unknown",
                "Devin 回复任务写入结果无法确认，未自动重试",
            ))
        })?;
        Ok(())
    }

    async fn wait_for_result(&self, job_id: &str, timeout: Duration) -> Result<(), AgentError> {
        let root = self
            .root
            .as_deref()
            .ok_or_else(|| state_error(DevinInboxState::NotRunning))?;
        let result_path = root.join(RESULT_DIR).join(format!("{job_id}.json"));
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            match tokio::fs::read(&result_path).await {
                Ok(bytes) => {
                    let result: WireResult = serde_json::from_slice(&bytes).map_err(|_| {
                        failed(
                            "devin_reply_result_invalid",
                            "Devin 扩展返回了无效结果，请重试",
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
                        "devin_reply_result_unreadable",
                        "无法读取 Devin 引用回复结果，未自动重试",
                    )));
                }
            }

            if tokio::time::Instant::now() >= deadline {
                return Err(AgentError::Unknown(safe_error(
                    "devin_reply_unconfirmed",
                    "Devin 扩展未在 10 秒内确认引用回复，任务不会自动重试",
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
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WireJob<'a> {
    id: &'a str,
    // 扩展（plugin/devin-extension-v2/extension.js）按 Devin/OpenCode 的既有约定
    // 读取 `sessionID` 与桌面端 Cascade 标识 `targetID`；camelCase 会写成
    // `sessionId`/`targetId`，扩展会判定“任务字段不完整”并拒绝执行。
    #[serde(rename = "sessionID")]
    session_id: &'a str,
    #[serde(rename = "targetID")]
    target_id: &'a str,
    text: &'a str,
    created_at: String,
    expires_at: String,
}

impl<'a> From<&'a DevinReplyJob> for WireJob<'a> {
    fn from(job: &'a DevinReplyJob) -> Self {
        Self {
            id: &job.id,
            session_id: job.session_id.as_str(),
            target_id: &job.target_id,
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

/// 把扩展返回的稳定错误码映射成微信可读提示；未知细节只做脱敏限长，不回显回复正文。
fn result_error(code: &str, detail: &str) -> AgentError {
    let detail = sanitize_result_error(detail);
    match code.trim().to_ascii_lowercase().as_str() {
        "desktop_unavailable" => unavailable(
            "devin_desktop_unavailable",
            "当前 Devin 桌面端未提供精确回复能力，请更新 Devin 桌面端后重启；回复不会改投到新会话",
        ),
        "invalid_job" => failed(
            "devin_reply_invalid_job",
            "回复任务无效，请重新引用原通知后再试",
        ),
        "session_not_found" => unavailable(
            "devin_reply_session_not_found",
            "目标 Devin 会话不存在或已删除，请确认会话后再试；回复不会改投到最近会话",
        ),
        // 以下错误码只由旧版扩展返回，保留映射避免混装时提示退化成原始报错。
        "agent_missing" => unavailable(
            "devin_agent_missing",
            "未找到 Devin 桌面端自带的 Agent，请更新或重启 Devin 后重试",
        ),
        "not_authenticated" => unavailable(
            "devin_not_authenticated",
            "Devin 桌面端登录状态已失效，请在 Devin 中重新登录后重试",
        ),
        "session_locked" => unavailable(
            "devin_session_locked",
            "目标会话正被 Devin 占用，请等本轮结束后再回复；若已无运行任务，请在 Devin 中关闭该会话或重启 Devin 后重试",
        ),
        "workspace_untrusted" => unavailable(
            "devin_workspace_untrusted",
            "目标工作区在 Devin 中未受信任，请先在 Devin 桌面端信任该工作区",
        ),
        "turn_failed" => {
            if detail.is_empty() {
                failed(
                    "devin_reply_failed",
                    "回复未执行成功，请查看 Devin 窗口中的错误提示",
                )
            } else {
                failed("devin_reply_failed", &format!("回复未执行成功：{detail}"))
            }
        }
        _ if !detail.is_empty() => {
            failed("devin_reply_failed", &format!("回复未执行成功：{detail}"))
        }
        _ => failed(
            "devin_reply_failed",
            "回复未执行成功，请查看 Devin 窗口中的错误提示",
        ),
    }
}

/// 每个状态都给用户可执行的下一步，不暴露内部路径。
fn state_error(state: DevinInboxState) -> AgentError {
    match state {
        DevinInboxState::Ready => {
            AgentError::Unknown(safe_error("devin_inbox_ready", "Devin 引用回复扩展已就绪"))
        }
        DevinInboxState::Unsupported => unavailable(
            "devin_desktop_unsupported",
            "当前 Devin 桌面端未提供精确回复能力，请更新 Devin 桌面端后重启；回复不会改投到新会话",
        ),
        DevinInboxState::Offline => unavailable(
            "devin_extension_offline",
            "Devin 引用回复扩展已离线，请重新打开 Devin 桌面端",
        ),
        DevinInboxState::NotRunning => unavailable(
            "devin_extension_not_running",
            "Devin 引用回复扩展未运行，请重新打开 Devin 桌面端",
        ),
        DevinInboxState::Error => unavailable(
            "devin_inbox_unreadable",
            "Devin 引用回复扩展状态无效，请检查收件箱目录权限后重启 Devin",
        ),
    }
}

fn heartbeat_is_fresh(timestamp: &str, now: time::OffsetDateTime) -> bool {
    let Ok(timestamp) =
        time::OffsetDateTime::parse(timestamp, &time::format_description::well_known::Rfc3339)
    else {
        return false;
    };
    if timestamp > now + time::Duration::seconds(HEARTBEAT_FUTURE_SKEW_SECONDS) {
        return false;
    }
    timestamp >= now - time::Duration::seconds(HEARTBEAT_MAX_AGE_SECONDS)
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
    SafeError::new(code, message).expect("Devin 错误常量必须是有效安全错误")
}
