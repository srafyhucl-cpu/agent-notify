use std::path::{Path, PathBuf};

use agentnotify_agent_sdk::{
    AgentAdapter, AgentCapabilities, AgentDescriptor, AgentError, AgentEventEnvelope, AgentHealth,
    NormalizedAgentEvent, ResumeReceipt,
};
use agentnotify_domain::{AgentSessionId, SafeError};
use serde_json::Value;

use crate::{
    descriptor::{capabilities, descriptor},
    event::parse_event,
    reply_inbox::DevinReplyInbox,
    session::{DevinDesktopSessions, DevinSessions},
};

const CONFIG_PATH_ENV: &str = "AGENT_NOTIFY_DEVIN_CONFIG";
const APP_DATA_ENV: &str = "APPDATA";
const CONFIG_SUBDIR: [&str; 2] = ["devin", "config.json"];
const HOOKS_KEY: &str = "hooks";
const STOP_EVENT: &str = "Stop";
const HANDLERS_KEY: &str = "hooks";
const COMMAND_KEY: &str = "command";
const STOP_COMMAND: &str = "devin stop";
/// 新 Hook、旧 AgentNotify 入口都算已接入（与 Go 版识别规则一致）。
const HOOK_EXE_NAME: &str = "agentnotify-devin-hook.exe";
const LEGACY_EXE_NAME: &str = "agent-notify.exe";

/// Devin 适配器：事件标准化、会话标题解析与桌面端 ACP 原会话精确回复。
#[derive(Clone)]
pub struct DevinAgent {
    sessions: DevinSessions,
    desktop: DevinDesktopSessions,
    inbox: DevinReplyInbox,
    hooks_path: Option<PathBuf>,
}

impl DevinAgent {
    /// 显式数据源构造；测试与自定义安装位置用。
    pub fn new(
        sessions: DevinSessions,
        desktop: DevinDesktopSessions,
        inbox: DevinReplyInbox,
        hooks_path: Option<PathBuf>,
    ) -> Self {
        Self {
            sessions,
            desktop,
            inbox,
            hooks_path,
        }
    }

    /// 按环境覆盖与默认安装位置构造；取不到时标题与目标解析都会明确失败。
    pub fn from_default_location() -> Self {
        Self::new(
            DevinSessions::from_default_location(),
            DevinDesktopSessions::from_default_location(),
            DevinReplyInbox::from_default_location(),
            default_hooks_path(),
        )
    }

    /// 不读取任何用户目录、也不接触真实桌面扩展的测试实例。
    pub fn new_test() -> Self {
        Self::new(
            DevinSessions::without_database(),
            DevinDesktopSessions::without_database(),
            DevinReplyInbox::without_backend(),
            None,
        )
    }

    pub fn with_sessions(mut self, sessions: DevinSessions) -> Self {
        self.sessions = sessions;
        self
    }

    pub fn with_desktop(mut self, desktop: DevinDesktopSessions) -> Self {
        self.desktop = desktop;
        self
    }

    pub fn with_inbox(mut self, inbox: DevinReplyInbox) -> Self {
        self.inbox = inbox;
        self
    }

    pub fn with_hooks_path(mut self, hooks_path: impl Into<PathBuf>) -> Self {
        self.hooks_path = Some(hooks_path.into());
        self
    }

    pub fn hooks_path(&self) -> Option<&Path> {
        self.hooks_path.as_deref()
    }

    pub fn desktop_sessions(&self) -> &DevinDesktopSessions {
        &self.desktop
    }
}

#[async_trait::async_trait]
impl AgentAdapter for DevinAgent {
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
        parse_event(envelope, &self.sessions)
    }

    async fn resume(
        &self,
        session_id: &AgentSessionId,
        text: &str,
    ) -> Result<ResumeReceipt, AgentError> {
        if text.trim().is_empty() {
            return Err(AgentError::InvalidInput);
        }
        // 与 Go 版顺序一致：先确认扩展在线，再把会话号解析成精确 Cascade 标识。
        self.inbox.ensure_ready().await?;
        let target_id = self.desktop.resolve_cascade(session_id.as_str())?;
        self.inbox.resume(session_id, &target_id, text).await?;
        Ok(ResumeReceipt {
            session_id: session_id.clone(),
        })
    }

    async fn inspect(&self) -> AgentHealth {
        let Some(path) = self.hooks_path.as_deref() else {
            return AgentHealth::unavailable(safe_error(
                "devin_config_path_missing",
                "无法确定 Devin 配置路径，请检查 APPDATA 或 AGENT_NOTIFY_DEVIN_CONFIG",
            ));
        };
        let content = match std::fs::read_to_string(path) {
            Ok(content) => content,
            Err(_) => {
                return AgentHealth::unavailable(safe_error(
                    "devin_config_not_found",
                    "未找到 Devin 配置，请先运行 Devin 并运行安装器接入 Hook",
                ));
            }
        };
        let root: Value = match serde_json::from_str(&content) {
            Ok(root) => root,
            Err(_) => {
                return AgentHealth::unavailable(safe_error(
                    "devin_config_invalid",
                    "Devin 配置格式无效，请修复 config.json 后重试",
                ));
            }
        };
        if hook_installed(&root) {
            AgentHealth::healthy()
        } else {
            AgentHealth::unavailable(safe_error(
                "devin_hook_not_installed",
                "Devin 未接入 AgentNotify Hook，请运行安装器",
            ))
        }
    }
}

/// `hooks.Stop[].hooks[].command` 中是否已有指向 AgentNotify 的 `devin stop` 命令。
fn hook_installed(root: &Value) -> bool {
    let Some(stop) = root.get(HOOKS_KEY).and_then(|hooks| hooks.get(STOP_EVENT)) else {
        return false;
    };
    let groups: Vec<&Value> = match stop {
        Value::Array(items) => items.iter().collect(),
        other => vec![other],
    };
    groups.iter().any(|group| {
        let handlers: Vec<&Value> = match group.get(HANDLERS_KEY) {
            Some(Value::Array(items)) => items.iter().collect(),
            Some(other) => vec![other],
            None => return false,
        };
        handlers.iter().any(|handler| {
            handler
                .get(COMMAND_KEY)
                .and_then(Value::as_str)
                .is_some_and(agent_notify_stop_command)
        })
    })
}

/// 命令里同时出现 AgentNotify 入口与 `devin stop` 才算已接入，不修改其他 Hook。
fn agent_notify_stop_command(command: &str) -> bool {
    let normalized = command.replace('\\', "/").to_lowercase();
    if !(normalized.contains(HOOK_EXE_NAME) || normalized.contains(LEGACY_EXE_NAME)) {
        return false;
    }
    normalized
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .contains(STOP_COMMAND)
}

fn default_hooks_path() -> Option<PathBuf> {
    if let Some(configured) = std::env::var_os(CONFIG_PATH_ENV).filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(configured));
    }
    let app_data = std::env::var_os(APP_DATA_ENV).filter(|value| !value.is_empty())?;
    let mut path = PathBuf::from(app_data);
    for part in CONFIG_SUBDIR {
        path.push(part);
    }
    Some(path)
}

fn safe_error(code: &str, message: &str) -> SafeError {
    SafeError::new(code, message).expect("Devin 健康错误常量必须是有效安全错误")
}
