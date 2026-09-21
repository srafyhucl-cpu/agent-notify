use std::{
    env,
    path::{Path, PathBuf},
    time::Duration,
};

use agentnotify_agent_sdk::{AgentError, AgentHealth};
use agentnotify_domain::{AgentSessionId, SafeError, Timestamp};
use serde::{Deserialize, Serialize};

const CONFIG_DIR: &str = ".config/agent-notify";
const INBOX_DIR: &str = "opencode-reply-inbox";
const HEARTBEAT_DIR: &str = "heartbeats";
const PENDING_DIR: &str = "pending";
const PROCESSING_DIR: &str = "processing";
const RESULT_DIR: &str = "results";
const HEARTBEAT_MAX_AGE_SECONDS: i64 = 30;
const HEARTBEAT_FUTURE_SKEW_SECONDS: i64 = 5;
const RESULT_POLL_INTERVAL: Duration = Duration::from_millis(100);
pub const DEFAULT_RESULT_WAIT: Duration = Duration::from_secs(10);
pub const JOB_TTL: time::Duration = time::Duration::minutes(10);
const MAX_RESULT_ERROR_CHARS: usize = 300;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OpenCodeInboxState {
    Ready,
    Waiting,
    Error,
    NotFound,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpenCodeReplyJob {
    pub id: String,
    pub session_id: AgentSessionId,
    pub text: String,
    pub created_at: Timestamp,
    pub expires_at: Timestamp,
}

#[derive(Clone, Debug)]
pub struct OpenCodeReplyInbox {
    root: PathBuf,
}

impl OpenCodeReplyInbox {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn from_default_location() -> Result<Self, AgentError> {
        let home = env::var_os("USERPROFILE")
            .or_else(|| env::var_os("HOME"))
            .ok_or_else(|| {
                AgentError::Unavailable(safe_error(
                    "opencode_home_missing",
                    "无法确定用户目录，不能检查 OpenCode 插件状态",
                ))
            })?;
        Ok(Self::new(
            PathBuf::from(home).join(CONFIG_DIR).join(INBOX_DIR),
        ))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub async fn inspect_state(&self) -> OpenCodeInboxState {
        match self.inspect_inner().await {
            Ok(state) => state,
            Err(_) => OpenCodeInboxState::Error,
        }
    }

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
        if self.inspect_state().await != OpenCodeInboxState::Ready {
            return Err(AgentError::Unavailable(safe_error(
                "opencode_plugin_not_ready",
                "OpenCode 插件未连接，请启动 OpenCode 后重试",
            )));
        }
        self.ensure_layout().await?;

        let now = Timestamp::now_utc();
        let expires_at = now.checked_add(JOB_TTL).ok_or_else(|| {
            AgentError::Failed(safe_error(
                "opencode_job_time_invalid",
                "引用回复任务时间无效，请重新发送",
            ))
        })?;
        let job = OpenCodeReplyJob {
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
        count_json_files(self.root.join(PENDING_DIR)).await
    }

    pub async fn processing_count(&self) -> usize {
        count_json_files(self.root.join(PROCESSING_DIR)).await
    }

    async fn inspect_inner(&self) -> Result<OpenCodeInboxState, std::io::Error> {
        let heartbeat_dir = self.root.join(HEARTBEAT_DIR);
        let mut entries = match tokio::fs::read_dir(&heartbeat_dir).await {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let root_exists = tokio::fs::metadata(&self.root).await.is_ok();
                return Ok(if root_exists {
                    OpenCodeInboxState::Waiting
                } else {
                    OpenCodeInboxState::NotFound
                });
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
            let bytes = match tokio::fs::read(entry.path()).await {
                Ok(bytes) => bytes,
                Err(_) => continue,
            };
            let Ok(heartbeat) = serde_json::from_slice::<Heartbeat>(&bytes) else {
                continue;
            };
            if heartbeat.ready && heartbeat_is_fresh(&heartbeat.timestamp, now) {
                return Ok(OpenCodeInboxState::Ready);
            }
        }

        Ok(OpenCodeInboxState::Waiting)
    }

    async fn ensure_layout(&self) -> Result<(), AgentError> {
        for directory in [
            self.root.clone(),
            self.root.join(PENDING_DIR),
            self.root.join(PROCESSING_DIR),
            self.root.join(RESULT_DIR),
            self.root.join(HEARTBEAT_DIR),
        ] {
            tokio::fs::create_dir_all(&directory).await.map_err(|_| {
                AgentError::Failed(safe_error(
                    "opencode_inbox_unwritable",
                    "无法创建 OpenCode 回复收件箱，请检查目录权限",
                ))
            })?;
        }
        Ok(())
    }

    async fn write_job_atomic(&self, job: &OpenCodeReplyJob) -> Result<(), AgentError> {
        let directory = self.root.join(PENDING_DIR);
        let destination = directory.join(format!("{}.json", job.id));
        let temporary = self
            .root
            .join(format!(".{}.{}.tmp", job.id, uuid::Uuid::new_v4()));
        let bytes = serde_json::to_vec(&WireJob::from(job)).map_err(|_| {
            AgentError::Failed(safe_error(
                "opencode_job_encode_failed",
                "无法编码 OpenCode 引用回复任务",
            ))
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
                "opencode_job_write_unknown",
                "OpenCode 回复任务写入结果无法确认，未自动重试",
            ))
        })?
        .map_err(|_| {
            AgentError::Unknown(safe_error(
                "opencode_job_write_unknown",
                "OpenCode 回复任务写入结果无法确认，未自动重试",
            ))
        })?;
        Ok(())
    }

    async fn wait_for_result(&self, job_id: &str, timeout: Duration) -> Result<(), AgentError> {
        let result_path = self.root.join(RESULT_DIR).join(format!("{job_id}.json"));
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            match tokio::fs::read(&result_path).await {
                Ok(bytes) => {
                    let result: WireResult = serde_json::from_slice(&bytes).map_err(|_| {
                        AgentError::Failed(safe_error(
                            "opencode_resume_result_invalid",
                            "OpenCode 插件返回了无效结果，请重试",
                        ))
                    })?;
                    if result.ok {
                        return Ok(());
                    }
                    let detail = result
                        .error
                        .as_deref()
                        .map(sanitize_result_error)
                        .filter(|value| !value.is_empty())
                        .unwrap_or_else(|| "OpenCode 未能继续目标会话".to_owned());
                    return Err(AgentError::Failed(safe_error(
                        "opencode_resume_failed",
                        &detail,
                    )));
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => {
                    return Err(AgentError::Unknown(safe_error(
                        "opencode_resume_result_unreadable",
                        "无法读取 OpenCode 引用回复结果，未自动重试",
                    )));
                }
            }

            if tokio::time::Instant::now() >= deadline {
                return Err(AgentError::Unknown(safe_error(
                    "opencode_resume_unconfirmed",
                    "OpenCode 尚未确认引用回复，任务不会自动重试",
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
    // 插件（plugin/rust/agent-notify.ts）按 OpenCode 自身约定读取 `sessionID`。
    // camelCase 会写成 `sessionId`，插件据此判定“引用回复任务字段不完整”并拒绝执行，
    // 真实链路曾因此整条失败，故此处显式锁定字段名。
    #[serde(rename = "sessionID")]
    session_id: &'a str,
    text: &'a str,
    created_at: String,
    expires_at: String,
}

impl<'a> From<&'a OpenCodeReplyJob> for WireJob<'a> {
    fn from(job: &'a OpenCodeReplyJob) -> Self {
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
    error: Option<String>,
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
    collapsed.chars().take(MAX_RESULT_ERROR_CHARS).collect()
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

pub fn health_for(state: OpenCodeInboxState) -> AgentHealth {
    match state {
        OpenCodeInboxState::Ready | OpenCodeInboxState::Waiting => AgentHealth::healthy(),
        OpenCodeInboxState::Error => AgentHealth::unavailable(safe_error(
            "opencode_inbox_unreadable",
            "OpenCode 回复收件箱不可读，请检查目录权限",
        )),
        OpenCodeInboxState::NotFound => AgentHealth::unavailable(safe_error(
            "opencode_plugin_not_found",
            "未检测到 OpenCode 插件，请先安装并重启 OpenCode",
        )),
    }
}

fn safe_error(code: &str, message: &str) -> SafeError {
    SafeError::new(code, message).expect("OpenCode 健康错误常量必须是有效安全错误")
}
