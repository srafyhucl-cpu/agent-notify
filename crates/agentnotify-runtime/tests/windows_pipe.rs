#![cfg(windows)]

use std::{
    process,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use agentnotify_agent_sdk::AgentEventEnvelope;
use agentnotify_domain::{AgentId, RequestId};
use agentnotify_ingress::{
    HandlerError, IngressHandler, PIPE_NAME_PREFIX, SubmitResult, connect_and_submit,
    current_user_sddl, pipe_name, serve_on,
};
use tokio::sync::{mpsc, watch};

static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(1);

struct CapturingHandler {
    sender: mpsc::UnboundedSender<AgentEventEnvelope>,
}

#[async_trait::async_trait]
impl IngressHandler for CapturingHandler {
    async fn handle(&self, envelope: AgentEventEnvelope) -> Result<(), HandlerError> {
        self.sender
            .send(envelope)
            .map_err(|_| HandlerError::new("handler_closed", "测试处理器已关闭"))
    }
}

fn fixture_event() -> AgentEventEnvelope {
    AgentEventEnvelope {
        request_id: RequestId::new("75fe53aa-2314-4c21-b12e-773efce521d9").unwrap(),
        agent_id: AgentId::new("opencode").unwrap(),
        payload: serde_json::json!({
            "eventType": "session.completed",
            "body": "任务完成"
        }),
    }
}

fn fixture_payload() -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "protocolVersion": 1,
        "kind": "agent.event",
        "requestId": "75fe53aa-2314-4c21-b12e-773efce521d9",
        "agentId": "opencode",
        "payload": {
            "eventType": "session.completed",
            "body": "任务完成"
        }
    }))
    .unwrap()
}

#[test]
fn pipe_name_is_stable_for_current_user_and_has_no_sid_text() {
    let first = pipe_name().unwrap();
    let second = pipe_name().unwrap();
    assert_eq!(first, second);
    assert!(first.starts_with(PIPE_NAME_PREFIX));
    assert!(!first.contains("S-1-5-"));
}

#[test]
fn current_user_acl_grants_only_the_current_user() {
    let sddl = current_user_sddl().unwrap();
    assert!(sddl.starts_with("D:P(A;;GA;;;S-1-"));
    assert!(!sddl.contains(";;;WD)"));
    assert!(!sddl.contains(";;;AU)"));
    assert!(!sddl.contains(";;;BA)"));
    assert!(!sddl.contains(";;;SY)"));
}

#[tokio::test]
async fn current_user_pipe_accepts_one_acknowledged_event() {
    let name = format!(
        "{PIPE_NAME_PREFIX}test-{}-{}",
        process::id(),
        TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    );
    let (sender, mut receiver) = mpsc::unbounded_channel();
    let handler = Arc::new(CapturingHandler { sender });
    let (cancel_sender, cancel_receiver) = watch::channel(false);
    let server = tokio::spawn(serve_on(name.clone(), handler, cancel_receiver));

    let result = loop {
        match connect_and_submit(&name, &fixture_payload(), Duration::from_millis(50)).await {
            Ok(result) => break result,
            Err(_) => tokio::time::sleep(Duration::from_millis(10)).await,
        }
    };
    assert_eq!(result, SubmitResult::Submitted);

    let envelope = tokio::time::timeout(Duration::from_secs(1), receiver.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(envelope, fixture_event());

    cancel_sender.send(true).unwrap();
    server.await.unwrap().unwrap();
}
