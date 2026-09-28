use agentnotify_agent_sdk::{
    AgentAdapter, AgentCapabilities, AgentDescriptor, AgentError, AgentEventEnvelope, AgentHealth,
    DispatchOptions, NormalizedAgentEvent, ResumeReceipt,
};
use agentnotify_domain::AgentSessionId;

use crate::{
    descriptor::{capabilities, descriptor},
    event::parse_event,
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

    pub async fn resume_with_timeout(
        &self,
        session_id: &AgentSessionId,
        text: &str,
        timeout: std::time::Duration,
    ) -> Result<ResumeReceipt, AgentError> {
        self.inbox
            .resume_with_timeout(session_id, text, timeout)
            .await?;
        Ok(ResumeReceipt {
            session_id: session_id.clone(),
        })
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
        session_id: &AgentSessionId,
        text: &str,
    ) -> Result<ResumeReceipt, AgentError> {
        self.inbox.resume(session_id, text).await?;
        Ok(ResumeReceipt {
            session_id: session_id.clone(),
        })
    }

    /// 新会话开工（编排派活 Step 1）：插件以该 session_id 发起新会话（A2A 新任务语义）。
    async fn open(
        &self,
        session_id: &AgentSessionId,
        text: &str,
    ) -> Result<ResumeReceipt, AgentError> {
        self.inbox.open(session_id, text).await?;
        Ok(ResumeReceipt {
            session_id: session_id.clone(),
        })
    }

    /// 派活透传（§4）：job 带 `model`/`location`/`unattended`，插件按会话映射落盘并按需
    /// 创建会话（location/model）或续聊（unattended 更新 + switchModel）。
    async fn dispatch_with_options(
        &self,
        session_id: &AgentSessionId,
        text: &str,
        open: bool,
        options: &DispatchOptions,
    ) -> Result<ResumeReceipt, AgentError> {
        self.inbox
            .dispatch_with_options(session_id, text, open, options)
            .await?;
        Ok(ResumeReceipt {
            session_id: session_id.clone(),
        })
    }

    async fn inspect(&self) -> AgentHealth {
        health_for(self.inbox.inspect_state().await)
    }
}
