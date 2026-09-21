//! Antigravity Agent 的事件标准化、会话标题解析与原会话精确回复边界。

mod adapter;
mod descriptor;
mod discovery;
mod event;
mod reply;
mod title;
mod transcript;

pub use adapter::AntigravityAgent;
pub use descriptor::{ANTIGRAVITY_AGENT_ID, capabilities, descriptor};
pub use event::{DEFAULT_BODY, parse_event};
pub use reply::{
    AgentApi, AntigravityReply, DEFAULT_METADATA_TIMEOUT, DEFAULT_SEND_TIMEOUT, EndpointDiscovery,
    LanguageServerEndpoint, antigravity_resume_target,
};
pub use title::{
    DEFAULT_TITLE, TitleResolution, TitleSource, default_annotations_dir, resolve_title,
};
pub use transcript::{SUMMARY_MAX_CHARS, TRANSCRIPT_TAIL_BYTES, read_transcript_summary};
