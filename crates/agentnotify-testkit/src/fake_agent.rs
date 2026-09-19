use std::sync::{Arc, Mutex};

use agentnotify_agent_sdk::{
    AgentAdapter, AgentCapabilities, AgentDescriptor, AgentError, AgentEventEnvelope, AgentHealth,
    NormalizedAgentEvent, ResumeReceipt,
};
use agentnotify_domain::{AgentId, AgentSessionId, NotificationMetadata, SafeError, Timestamp};

#[derive(Clone, Debug)]
pub enum FakeAgentMode {
    Success,
    Failure(SafeError),
    Unknown(SafeError),
    Unavailable(SafeError),
}

#[derive(Clone)]
pub struct FakeAgent {
    id: AgentId,
    capabilities: AgentCapabilities,
    mode: FakeAgentMode,
    delay: Option<time::Duration>,
    resume_count: Arc<Mutex<u64>>,
    last_session: Arc<Mutex<Option<AgentSessionId>>>,
}

impl FakeAgent {
    pub fn new(id: &str) -> Self {
        Self {
            id: AgentId::new(id).unwrap(),
            capabilities: AgentCapabilities {
                notify: true,
                resume: true,
                session_title: true,
                hook_installer: false,
                reply_window: false,
            },
            mode: FakeAgentMode::Success,
            delay: None,
            resume_count: Arc::new(Mutex::new(0)),
            last_session: Arc::new(Mutex::new(None)),
        }
    }

    pub fn without_resume(mut self) -> Self {
        self.capabilities.resume = false;
        self
    }

    pub fn with_mode(mut self, mode: FakeAgentMode) -> Self {
        self.mode = mode;
        self
    }

    pub fn with_delay(mut self, delay: time::Duration) -> Self {
        self.delay = Some(delay);
        self
    }

    pub fn resume_count(&self) -> u64 {
        *self.resume_count.lock().expect("FakeAgent 计数锁不应失败")
    }

    pub fn last_session(&self) -> Option<AgentSessionId> {
        self.last_session
            .lock()
            .expect("FakeAgent 会话锁不应失败")
            .clone()
    }
}

#[async_trait::async_trait]
impl AgentAdapter for FakeAgent {
    fn descriptor(&self) -> AgentDescriptor {
        AgentDescriptor {
            id: self.id.clone(),
            display_name: self.id.as_str().into(),
            description: "测试 Agent".into(),
            config_schema: serde_json::json!({"type": "object"}),
        }
    }

    fn capabilities(&self) -> AgentCapabilities {
        self.capabilities
    }

    fn parse_event(
        &self,
        envelope: AgentEventEnvelope,
    ) -> Result<NormalizedAgentEvent, AgentError> {
        if envelope.agent_id != self.id {
            return Err(AgentError::InvalidEvent);
        }
        let event_type = envelope
            .payload
            .get("eventType")
            .and_then(serde_json::Value::as_str)
            .ok_or(AgentError::InvalidEvent)?;
        if event_type != "session.completed" {
            return Err(AgentError::InvalidEvent);
        }
        let session_id = envelope
            .payload
            .get("sessionId")
            .and_then(serde_json::Value::as_str)
            .map(AgentSessionId::new)
            .transpose()
            .map_err(|_| AgentError::InvalidEvent)?;
        let title = envelope
            .payload
            .get("title")
            .and_then(serde_json::Value::as_str)
            .ok_or(AgentError::InvalidEvent)?
            .to_owned();
        let body = envelope
            .payload
            .get("body")
            .and_then(serde_json::Value::as_str)
            .ok_or(AgentError::InvalidEvent)?
            .to_owned();
        Ok(NormalizedAgentEvent {
            idempotency_key: envelope
                .payload
                .get("idempotencyKey")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned),
            occurred_at: envelope
                .payload
                .get("occurredAt")
                .and_then(serde_json::Value::as_str)
                .map(Timestamp::parse_rfc3339)
                .transpose()
                .map_err(|_| AgentError::InvalidEvent)?
                .unwrap_or_else(Timestamp::now_utc),
            session_id,
            session_title: None,
            title,
            body,
            metadata: NotificationMetadata::default(),
        })
    }

    async fn resume(
        &self,
        session_id: &AgentSessionId,
        text: &str,
    ) -> Result<ResumeReceipt, AgentError> {
        if let Some(delay) = self.delay {
            let delay =
                std::time::Duration::try_from(delay).expect("FakeAgent 延迟必须可转换为标准时长");
            tokio::time::sleep(delay).await;
        }
        if !self.capabilities.resume {
            return Err(AgentError::UnsupportedCapability);
        }
        if text.trim().is_empty() {
            return Err(AgentError::InvalidInput);
        }
        *self.resume_count.lock().expect("FakeAgent 计数锁不应失败") += 1;
        *self.last_session.lock().expect("FakeAgent 会话锁不应失败") = Some(session_id.clone());
        match &self.mode {
            FakeAgentMode::Success => Ok(ResumeReceipt {
                session_id: session_id.clone(),
            }),
            FakeAgentMode::Failure(error) => Err(AgentError::Failed(error.clone())),
            FakeAgentMode::Unknown(error) => Err(AgentError::Unknown(error.clone())),
            FakeAgentMode::Unavailable(error) => Err(AgentError::Unavailable(error.clone())),
        }
    }

    async fn inspect(&self) -> AgentHealth {
        AgentHealth::healthy()
    }
}
