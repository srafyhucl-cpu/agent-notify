use agentnotify_agent_sdk::{AgentCapabilities, AgentDescriptor};
use agentnotify_domain::AgentId;

pub const ANTIGRAVITY_AGENT_ID: &str = "antigravity";

pub fn descriptor() -> AgentDescriptor {
    AgentDescriptor {
        id: AgentId::new(ANTIGRAVITY_AGENT_ID).expect("Antigravity Agent ID 是固定有效值"),
        display_name: "Antigravity".into(),
        description: "Antigravity 完成事件、会话标题解析与原会话精确回复".into(),
        config_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "annotationsDir": {"type": "string"},
                "hooksPath": {"type": "string"}
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
