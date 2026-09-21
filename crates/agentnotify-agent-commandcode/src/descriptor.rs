use agentnotify_agent_sdk::{AgentCapabilities, AgentDescriptor};
use agentnotify_domain::AgentId;

pub const COMMANDCODE_AGENT_ID: &str = "commandcode";

pub fn descriptor() -> AgentDescriptor {
    AgentDescriptor {
        id: AgentId::new(COMMANDCODE_AGENT_ID).expect("Command Code Agent ID 是固定有效值"),
        display_name: "CommandCode".into(),
        description: "Command Code run_end 事件、会话标题解析与本地回复窗口".into(),
        config_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "commandCodeReplyWindowSec": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 600
                }
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
        // Command Code 是唯一由 mod 撑开等待窗口的 Agent，回复能力受窗口约束。
        reply_window: true,
    }
}
