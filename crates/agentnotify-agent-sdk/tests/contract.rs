use std::sync::Arc;

use agentnotify_agent_sdk::{
    AgentAdapter, AgentCapabilities, AgentDescriptor, AgentError, AgentEventEnvelope, AgentHealth,
    NormalizedAgentEvent, ResumeReceipt, assert_agent_contract,
};
use agentnotify_domain::{AgentId, AgentSessionId, NotificationMetadata, RequestId, Timestamp};

struct FakeAgent {
    capabilities: AgentCapabilities,
}

#[async_trait::async_trait]
impl AgentAdapter for FakeAgent {
    fn descriptor(&self) -> AgentDescriptor {
        AgentDescriptor {
            id: AgentId::new("opencode").unwrap(),
            display_name: "OpenCode".into(),
            description: "测试适配器".into(),
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
        if !self.capabilities.resume {
            return Err(AgentError::UnsupportedCapability);
        }
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
async fn adapter_contract_accepts_valid_resume_adapter() {
    let adapter: Arc<dyn AgentAdapter> = Arc::new(FakeAgent {
        capabilities: AgentCapabilities {
            notify: true,
            resume: true,
            session_title: true,
            hook_installer: false,
            reply_window: false,
        },
    });
    assert_agent_contract(adapter).await;
}

#[tokio::test]
async fn adapter_contract_rejects_empty_resume_text() {
    assert_agent_contract(Arc::new(FakeAgent {
        capabilities: AgentCapabilities {
            notify: true,
            resume: true,
            session_title: true,
            hook_installer: false,
            reply_window: false,
        },
    }))
    .await;
}

#[tokio::test]
async fn adapter_contract_checks_unsupported_resume() {
    assert_agent_contract(Arc::new(FakeAgent {
        capabilities: AgentCapabilities {
            notify: true,
            resume: false,
            session_title: true,
            hook_installer: false,
            reply_window: false,
        },
    }))
    .await;
}

#[test]
fn envelope_keeps_request_identity() {
    let envelope = AgentEventEnvelope {
        request_id: RequestId::new("request-1").unwrap(),
        agent_id: AgentId::new("opencode").unwrap(),
        payload: serde_json::json!({"eventType": "session.completed"}),
    };
    assert_eq!(envelope.request_id.as_str(), "request-1");
}
