use agentnotify_agent_sdk::{AgentError, AgentEventEnvelope, NormalizedAgentEvent};
use agentnotify_domain::{AgentId, AgentSessionId, NotificationMetadata, Timestamp};
use serde_json::Value;

use crate::descriptor::OPENCODE_AGENT_ID;

const COMPLETED_EVENT: &str = "session.completed";

pub fn parse_event(envelope: AgentEventEnvelope) -> Result<NormalizedAgentEvent, AgentError> {
    let expected_agent = AgentId::new(OPENCODE_AGENT_ID).expect("OpenCode Agent ID 是固定有效值");
    if envelope.agent_id != expected_agent {
        return Err(AgentError::InvalidEvent);
    }

    let payload = envelope
        .payload
        .as_object()
        .ok_or(AgentError::InvalidEvent)?;
    if payload.get("eventType").and_then(Value::as_str) != Some(COMPLETED_EVENT) {
        return Err(AgentError::InvalidEvent);
    }

    let session_id = required_text(payload, "sessionId")
        .ok_or(AgentError::InvalidEvent)
        .and_then(|value| AgentSessionId::new(value).map_err(|_| AgentError::InvalidEvent))?;
    let title = required_text(payload, "title").ok_or(AgentError::InvalidEvent)?;
    let body = required_text(payload, "body").ok_or(AgentError::InvalidEvent)?;
    let idempotency_key = optional_nonempty_text(payload, "idempotencyKey")?;
    let occurred_at = optional_timestamp(payload, "occurredAt")?.unwrap_or_else(Timestamp::now_utc);
    let metadata = parse_metadata(payload.get("metadata"))?;

    Ok(NormalizedAgentEvent {
        idempotency_key,
        occurred_at,
        session_id: Some(session_id),
        session_title: Some(title.clone()),
        title,
        body,
        metadata,
    })
}

fn required_text(payload: &serde_json::Map<String, Value>, field: &str) -> Option<String> {
    let value = payload.get(field)?.as_str()?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(value.to_owned())
}

fn optional_nonempty_text(
    payload: &serde_json::Map<String, Value>,
    field: &str,
) -> Result<Option<String>, AgentError> {
    match payload.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) if !value.trim().is_empty() => Ok(Some(value.clone())),
        Some(_) => Err(AgentError::InvalidEvent),
    }
}

fn optional_timestamp(
    payload: &serde_json::Map<String, Value>,
    field: &str,
) -> Result<Option<Timestamp>, AgentError> {
    match payload.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Timestamp::parse_rfc3339(value)
            .map(Some)
            .map_err(|_| AgentError::InvalidEvent),
        Some(_) => Err(AgentError::InvalidEvent),
    }
}

fn parse_metadata(value: Option<&Value>) -> Result<NotificationMetadata, AgentError> {
    let Some(value) = value else {
        return Ok(NotificationMetadata::default());
    };
    if value.is_null() {
        return Ok(NotificationMetadata::default());
    }
    let object = value.as_object().ok_or(AgentError::InvalidEvent)?;
    let mut entries = Vec::with_capacity(object.len());
    for (key, value) in object {
        let value = value.as_str().ok_or(AgentError::InvalidEvent)?;
        entries.push((key.clone(), value.to_owned()));
    }
    NotificationMetadata::new(entries).map_err(|_| AgentError::InvalidEvent)
}
