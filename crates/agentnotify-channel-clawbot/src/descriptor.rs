use agentnotify_channel_sdk::{ChannelCapabilities, ChannelDescriptor, InboundMode};
use agentnotify_domain::ChannelId;

pub const CLAWBOT_CHANNEL_ID: &str = "clawbot";
pub const MAX_TEXT_BYTES: usize = 32 * 1024;

pub fn descriptor() -> ChannelDescriptor {
    ChannelDescriptor {
        id: ChannelId::new(CLAWBOT_CHANNEL_ID).expect("ClawBot 渠道 ID 是固定有效值"),
        display_name: "ClawBot 微信".into(),
        config_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "baseUrl": {
                    "type": "string",
                    "minLength": 1
                }
            },
            "additionalProperties": false
        }),
    }
}

pub fn capabilities() -> ChannelCapabilities {
    ChannelCapabilities {
        send_text: true,
        receive: true,
        reply_routing: true,
        edit_message: false,
        attachments: false,
        markdown: true,
        max_text_bytes: Some(MAX_TEXT_BYTES),
        inbound_modes: vec![InboundMode::LongPolling],
    }
}
