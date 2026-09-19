use agentnotify_domain::ChannelId;

/// 对外公开的渠道身份、说明和配置结构。
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct ChannelDescriptor {
    pub id: ChannelId,
    pub display_name: String,
    pub config_schema: serde_json::Value,
}

/// 渠道入站连接的实现方式。
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum InboundMode {
    LongPolling,
    WebSocket,
    Webhook,
    LocalEvent,
}

/// 渠道当前可被核心调用的能力。
#[derive(Clone, Debug, Default, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct ChannelCapabilities {
    pub send_text: bool,
    pub receive: bool,
    pub reply_routing: bool,
    pub edit_message: bool,
    pub attachments: bool,
    pub markdown: bool,
    pub max_text_bytes: Option<usize>,
    pub inbound_modes: Vec<InboundMode>,
}

impl ChannelCapabilities {
    pub fn is_consistent(&self) -> bool {
        (!self.reply_routing || self.send_text) && (!self.receive || !self.inbound_modes.is_empty())
    }
}
