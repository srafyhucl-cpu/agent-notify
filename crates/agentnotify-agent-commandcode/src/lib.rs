//! Command Code Agent 的事件标准化、会话标题解析与本地回复窗口边界。

mod adapter;
mod descriptor;
mod event;
mod reply_inbox;
mod title;
mod window;

pub use adapter::CommandCodeAgent;
pub use descriptor::{COMMANDCODE_AGENT_ID, capabilities, descriptor};
pub use event::{DEFAULT_BODY, RUN_END_EVENT, parse_event};
pub use reply_inbox::{
    CommandCodeInboxState, CommandCodeModState, CommandCodeReplyInbox, CommandCodeReplyJob,
    DEFAULT_RESULT_WAIT, JOB_TTL,
};
pub use title::{CommandCodeSessions, DEFAULT_TITLE, TitleResolution, TitleSource};
pub use window::{
    MAX_REPLY_WINDOW_SEC, WINDOW_FILE_NAME, clamp_reply_window_sec, resolve_reply_window_sec,
    write_reply_window,
};
