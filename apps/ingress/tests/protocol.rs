use agentnotify_ingress::{
    IngressError,
    protocol::{IngressEvent, MAX_BODY_BYTES},
};

const REQUEST_ID: &str = "75fe53aa-2314-4c21-b12e-773efce521d9";

fn valid_event(body: &str) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "protocolVersion": 1,
        "kind": "agent.event",
        "requestId": REQUEST_ID,
        "agentId": "opencode",
        "payload": {
            "eventType": "session.completed",
            "sessionId": "session-1",
            "body": body,
            "customField": { "preserved": true }
        }
    }))
    .unwrap()
}

#[test]
fn protocol_rejects_unknown_version_and_kind() {
    let unknown_version = serde_json::to_vec(&serde_json::json!({
        "protocolVersion": 2,
        "kind": "agent.event",
        "requestId": REQUEST_ID,
        "agentId": "opencode",
        "payload": {}
    }))
    .unwrap();
    assert_eq!(
        IngressEvent::parse(&unknown_version).unwrap_err(),
        IngressError::UnsupportedVersion
    );

    let unknown_kind = serde_json::to_vec(&serde_json::json!({
        "protocolVersion": 1,
        "kind": "status.query",
        "requestId": REQUEST_ID,
        "agentId": "opencode",
        "payload": {}
    }))
    .unwrap();
    assert_eq!(
        IngressEvent::parse(&unknown_kind).unwrap_err(),
        IngressError::UnsupportedKind
    );
}

#[test]
fn protocol_requires_uuid_and_agent_identity() {
    let invalid_uuid = serde_json::to_vec(&serde_json::json!({
        "protocolVersion": 1,
        "kind": "agent.event",
        "requestId": "request-1",
        "agentId": "opencode",
        "payload": {}
    }))
    .unwrap();
    assert_eq!(
        IngressEvent::parse(&invalid_uuid).unwrap_err(),
        IngressError::InvalidRequestId
    );

    let invalid_agent = serde_json::to_vec(&serde_json::json!({
        "protocolVersion": 1,
        "kind": "agent.event",
        "requestId": REQUEST_ID,
        "agentId": " ",
        "payload": {}
    }))
    .unwrap();
    assert_eq!(
        IngressEvent::parse(&invalid_agent).unwrap_err(),
        IngressError::InvalidAgentId
    );
}

#[test]
fn protocol_accepts_object_payload_and_preserves_adapter_fields() {
    let envelope = IngressEvent::parse(&valid_event("构建完成")).unwrap();
    assert_eq!(envelope.request_id.as_str(), REQUEST_ID);
    assert_eq!(envelope.agent_id.as_str(), "opencode");
    assert_eq!(
        envelope.payload["customField"]["preserved"],
        serde_json::Value::Bool(true)
    );
}

#[test]
fn protocol_rejects_non_object_payload_and_oversized_body() {
    let non_object = serde_json::to_vec(&serde_json::json!({
        "protocolVersion": 1,
        "kind": "agent.event",
        "requestId": REQUEST_ID,
        "agentId": "opencode",
        "payload": []
    }))
    .unwrap();
    assert_eq!(
        IngressEvent::parse(&non_object).unwrap_err(),
        IngressError::InvalidPayload
    );

    let oversized_body = valid_event(&"x".repeat(MAX_BODY_BYTES + 1));
    assert_eq!(
        IngressEvent::parse(&oversized_body).unwrap_err(),
        IngressError::BodyTooLarge
    );
}
