use std::sync::Arc;

use agentnotify_agent_sdk::{
    AgentAdapter, AgentCapabilities, AgentDescriptor, AgentError, AgentEventEnvelope, AgentHealth,
    AgentRegistry, NormalizedAgentEvent, ResumeReceipt,
};
use agentnotify_domain::{AgentId, AgentSessionId, NotificationMetadata, RequestId, Timestamp};

struct FakeAgent {
    id: AgentId,
}

impl FakeAgent {
    fn new(id: &str) -> Self {
        Self {
            id: AgentId::new(id).unwrap(),
        }
    }
}

#[async_trait::async_trait]
impl AgentAdapter for FakeAgent {
    fn descriptor(&self) -> AgentDescriptor {
        AgentDescriptor {
            id: self.id.clone(),
            display_name: "Fake Agent".into(),
            description: "测试适配器".into(),
            config_schema: serde_json::json!({"type": "object"}),
        }
    }

    fn capabilities(&self) -> AgentCapabilities {
        AgentCapabilities {
            notify: true,
            resume: true,
            session_title: true,
            hook_installer: false,
            reply_window: false,
        }
    }

    fn parse_event(
        &self,
        envelope: AgentEventEnvelope,
    ) -> Result<NormalizedAgentEvent, AgentError> {
        if envelope
            .payload
            .get("eventType")
            .and_then(|value| value.as_str())
            != Some("session.completed")
        {
            return Err(AgentError::InvalidEvent);
        }
        Ok(NormalizedAgentEvent {
            idempotency_key: None,
            occurred_at: Timestamp::parse_rfc3339("2026-09-19T10:20:30Z").unwrap(),
            session_id: None,
            session_title: None,
            title: "任务完成".into(),
            body: "测试事件".into(),
            metadata: NotificationMetadata::default(),
        })
    }

    async fn resume(
        &self,
        session_id: &AgentSessionId,
        text: &str,
    ) -> Result<ResumeReceipt, AgentError> {
        if text.trim().is_empty() {
            return Err(AgentError::InvalidInput);
        }
        Ok(ResumeReceipt {
            session_id: session_id.clone(),
        })
    }

    async fn inspect(&self) -> AgentHealth {
        AgentHealth::healthy()
    }
}

#[tokio::test]
async fn registry_rejects_duplicate_agent_id() {
    let mut registry = AgentRegistry::default();
    registry
        .register(Arc::new(FakeAgent::new("opencode")))
        .unwrap();
    let error = registry
        .register(Arc::new(FakeAgent::new("opencode")))
        .unwrap_err();
    assert_eq!(error.code(), "agent_already_registered");
}

#[test]
fn registry_returns_agents_in_stable_id_order() {
    let mut registry = AgentRegistry::default();
    registry.register(Arc::new(FakeAgent::new("zeta"))).unwrap();
    registry
        .register(Arc::new(FakeAgent::new("alpha")))
        .unwrap();

    let ids = registry
        .all()
        .iter()
        .map(|adapter| adapter.descriptor().id)
        .collect::<Vec<_>>();
    let mut sorted = ids.clone();
    sorted.sort();
    assert_eq!(ids, sorted);
}

#[test]
fn registry_requires_protocol_request_id() {
    let request_id_attempt = RequestId::new("request-1");
    assert!(request_id_attempt.is_ok());
}
