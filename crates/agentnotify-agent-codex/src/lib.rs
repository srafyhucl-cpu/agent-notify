//! Codex Agent 的事件标准化、会话标题解析与原线程精确续聊边界。

mod adapter;
mod descriptor;
mod event;
mod resume;
mod title;

pub use adapter::CodexAgent;
pub use descriptor::{CODEX_AGENT_ID, capabilities, descriptor};
pub use event::{DEFAULT_BODY, TURN_COMPLETE_EVENT, parse_event};
pub use resume::{CodexQueue, CommandExecutor, CommandOutput, DEFAULT_QUEUE_TIMEOUT, queue_args};
pub use title::{DEFAULT_TITLE, TitleResolution, TitleSource, default_codex_home, resolve_title};
