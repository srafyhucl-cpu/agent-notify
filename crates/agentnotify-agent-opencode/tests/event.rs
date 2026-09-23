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

#[test]
fn native_terminal_event_names_and_missing_type_are_accepted() {
    // 插件改用 OpenCode 原生终态名、或不带 eventType 时都要照常推送，不能静默拒收。
    for event_type in [
        Some("session.completed"),
        Some("session.idle"),
        Some("session.error"),
        Some("session.execution.succeeded"),
        Some("session.execution.failed"),
        Some(""),
        Some("   "),
        None,
    ] {
        let mut payload = serde_json::json!({
            "sessionId": "session-1",
            "title": "构建完成",
            "body": "Release 已生成"
        });
        if let Some(event_type) = event_type {
            payload["eventType"] = serde_json::Value::String(event_type.to_owned());
        }
        let event = agent()
            .parse_event(envelope(payload))
            .unwrap_or_else(|_| panic!("终态事件应被接受：{event_type:?}"));
        assert_eq!(event.session_id.unwrap().as_str(), "session-1");
    }
}

#[test]
fn non_string_event_type_is_rejected() {
    assert!(
        agent()
            .parse_event(envelope(serde_json::json!({
                "eventType": 7,
                "sessionId": "session-1",
                "title": "标题",
                "body": "正文"
            })))
            .is_err()
    );
}
