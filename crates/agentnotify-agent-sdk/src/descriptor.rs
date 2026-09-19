use agentnotify_domain::{AgentId, SafeError};

/// 对外公开的 Agent 身份、说明和配置结构。
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct AgentDescriptor {
    pub id: AgentId,
    pub display_name: String,
    pub description: String,
    pub config_schema: serde_json::Value,
}

/// Agent 当前可被核心调用的能力。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct AgentCapabilities {
    pub notify: bool,
    pub resume: bool,
    pub session_title: bool,
    pub hook_installer: bool,
    pub reply_window: bool,
}

/// Agent 的健康快照，只包含可展示且已脱敏的信息。
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct AgentHealth {
    pub available: bool,
    pub detail: Option<SafeError>,
}

impl AgentHealth {
    pub const fn healthy() -> Self {
        Self {
            available: true,
            detail: None,
        }
    }

    pub const fn unavailable(detail: SafeError) -> Self {
        Self {
            available: false,
            detail: Some(detail),
        }
    }
}
