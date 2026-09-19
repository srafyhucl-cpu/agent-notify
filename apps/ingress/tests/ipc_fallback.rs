#![cfg(windows)]

use std::time::Duration;

use agentnotify_agent_sdk::AgentEventEnvelope;
use agentnotify_domain::{AgentId, RequestId};
use agentnotify_ingress::{
    PIPE_NAME_PREFIX, Spool, SpoolLimits, SubmitResult, submit_with_fallback,
};

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

#[tokio::test]
async fn client_spools_when_pipe_is_unavailable() {
    let temp = tempfile::tempdir().unwrap();
    let spool = Spool::open(temp.path().join("spool"), SpoolLimits::default()).unwrap();
    let name = format!("{PIPE_NAME_PREFIX}does-not-exist");

    let result = submit_with_fallback(
        &fixture_event(),
        Some(&name),
        &spool,
        Duration::from_millis(150),
    )
    .await
    .unwrap();

    assert_eq!(result, SubmitResult::Spooled);
    assert_eq!(spool.queued_count().unwrap(), 1);
}
