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
    reply::AntigravityReply,
    title::default_annotations_dir,
};

const HOOKS_FILE_ENV: &str = "AGENT_NOTIFY_ANTIGRAVITY_HOOKS";
const USER_PROFILE_ENV: &str = "USERPROFILE";
const HOME_ENV: &str = "HOME";
const GEMINI_DIR_NAME: &str = ".gemini";
const CONFIG_DIR_NAME: &str = "config";
const HOOKS_FILE_NAME: &str = "hooks.json";
const ANTIGRAVITY_DIR_NAME: &str = "antigravity";
const ANNOTATIONS_DIR_NAME: &str = "annotations";
/// 新 Hook、旧 AgentNotify 入口与同目录启动器都算已接入（与 Go 版识别规则一致）。
const HOOK_EXE_NAME: &str = "agentnotify-antigravity-hook.exe";
const LEGACY_EXE_NAME: &str = "agent-notify.exe";
const LAUNCHER_NAME: &str = "agent-notify-hook.cmd";
const TOP_LEVEL_KEY: &str = "agent-notify";
const STOP_EVENT: &str = "Stop";
const STOP_COMMAND: &str = "antigravity stop";

/// Antigravity 适配器：事件标准化、标题解析与原会话精确回复。
#[derive(Clone)]
pub struct AntigravityAgent {
    annotations_dir: Option<PathBuf>,
    hooks_path: Option<PathBuf>,
    reply: AntigravityReply,
}

impl AntigravityAgent {
    /// 使用显式 `.gemini` 目录推导 annotations 与 hooks.json；测试与自定义安装位置用。
    pub fn new(gemini_home: impl Into<PathBuf>) -> Self {
        let home = gemini_home.into();
        Self {
            annotations_dir: Some(home.join(ANTIGRAVITY_DIR_NAME).join(ANNOTATIONS_DIR_NAME)),
            hooks_path: Some(home.join(CONFIG_DIR_NAME).join(HOOKS_FILE_NAME)),
            reply: AntigravityReply::from_default_location(),
        }
    }

    /// 使用环境覆盖或 `%USERPROFILE%\.gemini`；取不到时标题链降级，通知仍可用。
    pub fn from_default_location() -> Self {
        let hooks_path = env_override(HOOKS_FILE_ENV)
            .or_else(|| gemini_home().map(|home| home.join(CONFIG_DIR_NAME).join(HOOKS_FILE_NAME)));
        Self {
            annotations_dir: default_annotations_dir(),
            hooks_path,
            reply: AntigravityReply::from_default_location(),
        }
    }

    /// 不读取任何用户目录、也不接触真实桌面端的测试实例。
    pub fn new_test() -> Self {
        Self {
            annotations_dir: None,
            hooks_path: None,
            reply: AntigravityReply::without_backend(),
        }
    }

    pub fn with_reply(mut self, reply: AntigravityReply) -> Self {
        self.reply = reply;
        self
    }

    /// 覆盖 annotations 目录；界面里配置的 `annotationsDir` 生效。
    pub fn with_annotations_dir(mut self, annotations_dir: impl Into<PathBuf>) -> Self {
        self.annotations_dir = Some(annotations_dir.into());
        self
    }

    /// 覆盖 hooks.json 路径；界面里配置的 `hooksPath` 生效。
    pub fn with_hooks_path(mut self, hooks_path: impl Into<PathBuf>) -> Self {
        self.hooks_path = Some(hooks_path.into());
        self
    }

    pub fn annotations_dir(&self) -> Option<&Path> {
        self.annotations_dir.as_deref()
    }

    pub fn hooks_path(&self) -> Option<&Path> {
        self.hooks_path.as_deref()
    }
}

#[async_trait::async_trait]
impl AgentAdapter for AntigravityAgent {
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
        parse_event(envelope, self.annotations_dir.as_deref())
    }

    async fn resume(
        &self,
        session_id: &AgentSessionId,
        text: &str,
    ) -> Result<ResumeReceipt, AgentError> {
        self.reply.send(session_id, text).await?;
        Ok(ResumeReceipt {
            session_id: session_id.clone(),
        })
    }

    async fn inspect(&self) -> AgentHealth {
        let Some(path) = self.hooks_path.as_deref() else {
            return AgentHealth::unavailable(safe_error(
                "antigravity_hooks_missing",
                "无法确定 Antigravity hooks.json 路径，请检查 USERPROFILE 或 AGENT_NOTIFY_ANTIGRAVITY_HOOKS",
            ));
        };
        let content = match std::fs::read_to_string(path) {
            Ok(content) => content,
            Err(_) => {
                return AgentHealth::unavailable(safe_error(
                    "antigravity_hooks_not_found",
                    "未找到 Antigravity hooks 配置，请先安装 Antigravity 与 Hook",
                ));
            }
        };
        let root: Value = match serde_json::from_str(&content) {
            Ok(root) => root,
            Err(_) => {
                return AgentHealth::unavailable(safe_error(
                    "antigravity_hooks_invalid",
                    "Antigravity hooks 配置格式无效，请修复 hooks.json 后重试",
                ));
            }
        };
        if hook_installed(&root) {
            AgentHealth::healthy()
        } else {
            AgentHealth::unavailable(safe_error(
                "antigravity_hook_not_installed",
                "Antigravity 未接入 AgentNotify Hook，请运行安装器",
            ))
        }
    }
}

/// 顶层 `agent-notify.Stop` 中是否已有指向 AgentNotify 的 `antigravity stop` 命令。
fn hook_installed(root: &Value) -> bool {
    let Some(stop) = root
        .get(TOP_LEVEL_KEY)
        .and_then(|group| group.get(STOP_EVENT))
    else {
        return false;
    };
    let handlers: Vec<&Value> = match stop {
        Value::Array(items) => items.iter().collect(),
        other => vec![other],
    };
    handlers.iter().any(|handler| {
        handler
            .get("command")
            .and_then(Value::as_str)
            .is_some_and(agent_notify_stop_command)
    })
}

/// 命令里同时出现 AgentNotify 入口与 `antigravity stop` 才算已接入，不修改其他 Hook。
fn agent_notify_stop_command(command: &str) -> bool {
    let normalized = command.replace('\\', "/").to_lowercase();
    if !(normalized.contains(HOOK_EXE_NAME)
        || normalized.contains(LEGACY_EXE_NAME)
        || normalized.contains(LAUNCHER_NAME))
    {
        return false;
    }
    normalized
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .contains(STOP_COMMAND)
}

fn gemini_home() -> Option<PathBuf> {
    let home = env_override(USER_PROFILE_ENV).or_else(|| env_override(HOME_ENV))?;
    Some(home.join(GEMINI_DIR_NAME))
}

fn env_override(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn safe_error(code: &str, message: &str) -> SafeError {
    SafeError::new(code, message).expect("Antigravity 健康错误常量必须是有效安全错误")
}
