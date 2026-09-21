use std::path::{Path, PathBuf};

use agentnotify_agent_sdk::{
    AgentAdapter, AgentCapabilities, AgentDescriptor, AgentError, AgentEventEnvelope, AgentHealth,
    NormalizedAgentEvent, ResumeReceipt,
};
use agentnotify_domain::{AgentSessionId, SafeError};

use crate::{
    descriptor::{capabilities, descriptor},
    event::parse_event,
    resume::CodexQueue,
    title::default_codex_home,
};

const CONFIG_FILE_NAME: &str = "config.toml";
const HOOK_EXE_NAME: &str = "agentnotify-codex-hook.exe";
const LEGACY_EXE_NAME: &str = "agent-notify.exe";

/// Codex 适配器：事件标准化、标题解析与原线程精确续聊。
#[derive(Clone)]
pub struct CodexAgent {
    codex_home: Option<PathBuf>,
    queue: CodexQueue,
}

impl CodexAgent {
    /// 使用显式 Codex 主目录；测试与自定义安装位置用。
    pub fn new(codex_home: impl Into<PathBuf>) -> Self {
        Self {
            codex_home: Some(codex_home.into()),
            queue: CodexQueue::from_default_location(),
        }
    }

    /// 使用 `%CODEX_HOME%` 或 `%USERPROFILE%\.codex`；都取不到时标题链降级，通知仍可用。
    pub fn from_default_location() -> Self {
        Self {
            codex_home: default_codex_home(),
            queue: CodexQueue::from_default_location(),
        }
    }

    /// 不读取任何用户目录的测试实例。
    pub fn new_test() -> Self {
        Self {
            codex_home: None,
            queue: CodexQueue::from_default_location(),
        }
    }

    pub fn with_queue(mut self, queue: CodexQueue) -> Self {
        self.queue = queue;
        self
    }

    pub fn codex_home(&self) -> Option<&Path> {
        self.codex_home.as_deref()
    }
}

#[async_trait::async_trait]
impl AgentAdapter for CodexAgent {
    fn descriptor(&self) -> AgentDescriptor {
        descriptor()
    }

    fn capabilities(&self) -> AgentCapabilities {
        capabilities()
    }

    fn parse_event(
        &self,
        envelope: AgentEventEnvelope,
    ) -> Result<NormalizedAgentEvent, AgentError> {
        parse_event(envelope, self.codex_home.as_deref())
    }

    async fn resume(
        &self,
        session_id: &AgentSessionId,
        text: &str,
    ) -> Result<ResumeReceipt, AgentError> {
        self.queue.enqueue(session_id.as_str(), text).await?;
        Ok(ResumeReceipt {
            session_id: session_id.clone(),
        })
    }

    async fn inspect(&self) -> AgentHealth {
        let Some(home) = self.codex_home.as_deref() else {
            return AgentHealth::unavailable(safe_error(
                "codex_home_missing",
                "无法确定 Codex 主目录，请检查 USERPROFILE 或 CODEX_HOME",
            ));
        };
        match std::fs::read_to_string(home.join(CONFIG_FILE_NAME)) {
            Err(_) => AgentHealth::unavailable(safe_error(
                "codex_config_not_found",
                "未找到 Codex 配置，请先运行 Codex 并安装 Hook",
            )),
            Ok(content) if notify_line_targets_hook(&content) => AgentHealth::healthy(),
            Ok(_) => AgentHealth::unavailable(safe_error(
                "codex_hook_not_installed",
                "Codex 未接入 AgentNotify Hook，请运行安装器",
            )),
        }
    }
}

/// notify 行里出现新 Hook 或旧 AgentNotify 都算已接入。
fn notify_line_targets_hook(content: &str) -> bool {
    content.lines().any(|line| {
        let trimmed = line.trim_start();
        let Some(rest) = trimmed.strip_prefix("notify") else {
            return false;
        };
        if !rest.trim_start().starts_with('=') {
            return false;
        }
        let lower = trimmed.to_ascii_lowercase();
        lower.contains(HOOK_EXE_NAME) || lower.contains(LEGACY_EXE_NAME)
    })
}

fn safe_error(code: &str, message: &str) -> SafeError {
    SafeError::new(code, message).expect("Codex 健康错误常量必须是有效安全错误")
}
