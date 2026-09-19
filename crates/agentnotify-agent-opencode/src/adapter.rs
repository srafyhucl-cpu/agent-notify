use agentnotify_agent_sdk::{
    AgentAdapter, AgentCapabilities, AgentDescriptor, AgentError, AgentEventEnvelope, AgentHealth,
    NormalizedAgentEvent, ResumeReceipt,
};
use agentnotify_domain::AgentSessionId;

use crate::{
    descriptor::{capabilities, descriptor},
    event::{parse_event, safe_error},
    reply_inbox::{OpenCodeReplyInbox, health_for},
};

#[derive(Clone)]
pub struct OpenCodeAgent {
    inbox: OpenCodeReplyInbox,
}

impl OpenCodeAgent {
    pub fn new(inbox: OpenCodeReplyInbox) -> Self {
        Self { inbox }
    }

    pub fn from_default_location() -> Result<Self, AgentError> {
        OpenCodeReplyInbox::from_default_location().map(Self::new)
    }

    pub fn inbox(&self) -> &OpenCodeReplyInbox {
        &self.inbox
    }
}

#[async_trait::async_trait]
impl AgentAdapter for OpenCodeAgent {
    fn descriptor(&self) -> AgentDescriptor {
        descriptor()
    }

    fn capabilities(&self) -> AgentCapabilities {
        capabilities()
    }

    fn parse_event(
        &self,
        envelope: AgentEventEnvelope,
    ) -> Result<NormalizedAgentEvent, AgentError> {
        parse_event(envelope)
    }

    async fn resume(
        &self,
        _session_id: &AgentSessionId,
        text: &str,
    ) -> Result<ResumeReceipt, AgentError> {
        if text.trim().is_empty() {
            return Err(AgentError::InvalidInput);
        }
        match safe_error("opencode_resume_unavailable", "OpenCode 原会话续聊尚未就绪") {
            AgentError::Failed(error) => Err(AgentError::Unknown(error)),
            other => Err(other),
        }
    }

    async fn inspect(&self) -> AgentHealth {
        health_for(self.inbox.inspect_state().await)
    }
}
