use agentnotify_agent_sdk::{AgentCapabilities, AgentDescriptor};
use agentnotify_domain::AgentId;

pub const CODEX_AGENT_ID: &str = "codex";

pub fn descriptor() -> AgentDescriptor {
    AgentDescriptor {
        id: AgentId::new(CODEX_AGENT_ID).expect("Codex Agent ID 是固定有效值"),
        display_name: "Codex".into(),
        description: "Codex 完成事件、会话标题解析与原线程精确续聊".into(),
        config_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "codexHome": {"type": "string"}
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
