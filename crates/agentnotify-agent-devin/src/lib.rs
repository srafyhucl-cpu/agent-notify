//! Devin Agent 的事件标准化、桌面端会话元数据解析与 ACP 精确回复边界。

mod adapter;
mod descriptor;
mod event;
mod reply_inbox;
mod session;

pub use adapter::DevinAgent;
pub use descriptor::{DEVIN_AGENT_ID, capabilities, descriptor};
pub use event::{DEFAULT_BODY, DEFAULT_TITLE, parse_event};
pub use reply_inbox::{
    DEFAULT_RESULT_WAIT, DevinInboxState, DevinReplyInbox, DevinReplyJob, JOB_TTL,
};
pub use session::{DevinDesktopSessions, DevinSessions};
