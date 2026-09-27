use agentnotify_agent_sdk::{AgentCapabilities, AgentDescriptor};
use agentnotify_domain::AgentId;

pub const OPENCODE_AGENT_ID: &str = "opencode";

pub fn descriptor() -> AgentDescriptor {
    AgentDescriptor {
        id: AgentId::new(OPENCODE_AGENT_ID).expect("OpenCode Agent ID 是固定有效值"),
        display_name: "OpenCode".into(),
        description: "OpenCode 会话完成事件、原会话续聊与发起新会话开工".into(),
        config_schema: serde_json::json!({
            "type": "object",
            "properties": {
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
        session_open: true,
        session_title: true,
        hook_installer: true,
        reply_window: false,
    }
}
