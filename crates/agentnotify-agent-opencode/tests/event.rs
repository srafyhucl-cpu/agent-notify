use agentnotify_agent_opencode::OpenCodeAgent;
use agentnotify_agent_sdk::AgentAdapter;
use agentnotify_domain::{AgentId, RequestId};

fn agent() -> OpenCodeAgent {
    OpenCodeAgent::new(agentnotify_agent_opencode::OpenCodeReplyInbox::new(
        tempdir(),
    ))
}

fn tempdir() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("agentnotify-opencode-{}", uuid::Uuid::new_v4()))
}

fn envelope(payload: serde_json::Value) -> agentnotify_agent_sdk::AgentEventEnvelope {
    agentnotify_agent_sdk::AgentEventEnvelope {
        request_id: RequestId::new(uuid::Uuid::new_v4().to_string()).unwrap(),
        agent_id: AgentId::new("opencode").unwrap(),
        payload,
    }
}

#[test]
fn completed_event_normalizes_session_title_and_body() {
    let event = agent()
        .parse_event(envelope(serde_json::json!({
            "eventType": "session.completed",
            "idempotencyKey": "opencode:session-1:event-9",
            "occurredAt": "2026-09-19T10:20:30.123+08:00",
            "sessionId": "session-1",
            "title": "构建完成",
            "body": "Release 已生成"
        })))
        .unwrap();

    assert_eq!(event.session_id.unwrap().as_str(), "session-1");
    assert_eq!(event.session_title.as_deref(), Some("构建完成"));
    assert_eq!(event.title, "构建完成");
    assert_eq!(event.body, "Release 已生成");
    assert_eq!(
        event.idempotency_key.as_deref(),
        Some("opencode:session-1:event-9")
    );
}

#[test]
fn completed_event_rejects_missing_required_fields() {
    for payload in [
        serde_json::json!({"eventType": "session.completed", "title": "标题", "body": "正文"}),
        serde_json::json!({"eventType": "session.completed", "sessionId": "s", "body": "正文"}),
        serde_json::json!({"eventType": "session.completed", "sessionId": "s", "title": "标题"}),
    ] {
        assert!(agent().parse_event(envelope(payload)).is_err());
    }
}

#[test]
fn other_agent_and_unknown_event_are_rejected() {
    let mut wrong_agent = envelope(serde_json::json!({
        "eventType": "session.completed",
        "sessionId": "s",
        "title": "标题",
        "body": "正文"
    }));
    wrong_agent.agent_id = AgentId::new("codex").unwrap();
    assert!(agent().parse_event(wrong_agent).is_err());

    assert!(
        agent()
            .parse_event(envelope(
                serde_json::json!({"eventType": "session.started"})
            ))
            .is_err()
    );
}
