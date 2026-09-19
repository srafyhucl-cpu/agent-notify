use std::{
    env,
    path::{Path, PathBuf},
};

use agentnotify_agent_sdk::{AgentError, AgentHealth};
use agentnotify_domain::SafeError;
use serde::Deserialize;

const CONFIG_DIR: &str = ".config/agent-notify";
const INBOX_DIR: &str = "opencode-reply-inbox";
const HEARTBEAT_DIR: &str = "heartbeats";
const HEARTBEAT_MAX_AGE_SECONDS: i64 = 30;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OpenCodeInboxState {
    Ready,
    Waiting,
    Error,
    NotFound,
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
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Heartbeat {
    ready: bool,
    timestamp: String,
}

fn heartbeat_is_fresh(timestamp: &str, now: time::OffsetDateTime) -> bool {
    let Ok(timestamp) =
        time::OffsetDateTime::parse(timestamp, &time::format_description::well_known::Rfc3339)
    else {
        return false;
    };
    if timestamp > now + time::Duration::seconds(5) {
        return false;
    }
    timestamp >= now - time::Duration::seconds(HEARTBEAT_MAX_AGE_SECONDS)
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
