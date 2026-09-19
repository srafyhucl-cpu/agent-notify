//! OpenCode Agent 的事件标准化、健康检查与原会话续聊边界。

mod adapter;
mod descriptor;
mod event;
mod reply_inbox;

pub use adapter::OpenCodeAgent;
pub use descriptor::{OPENCODE_AGENT_ID, capabilities, descriptor};
pub use event::parse_event;
pub use reply_inbox::{OpenCodeInboxState, OpenCodeReplyInbox};
