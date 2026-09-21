use agentnotify_agent_sdk::{AgentCapabilities, AgentDescriptor};
use agentnotify_domain::AgentId;

pub const DEVIN_AGENT_ID: &str = "devin";

pub fn descriptor() -> AgentDescriptor {
    AgentDescriptor {
        id: AgentId::new(DEVIN_AGENT_ID).expect("Devin Agent ID 是固定有效值"),
        display_name: "Devin".into(),
        description: "Devin 桌面端完成事件、会话标题解析与 ACP 原会话精确回复".into(),
        config_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "sessionsDatabase": {"type": "string"},
                "desktopStateDatabase": {"type": "string"},
                "replyInbox": {"type": "string"}
            },
            "additionalProperties": false
        }),
    }
}

pub const fn capabilities() -> AgentCapabilities {
    AgentCapabilities {
        notify: true,
        resume: true,
        session_title: true,
        hook_installer: true,
        reply_window: false,
    }
}
