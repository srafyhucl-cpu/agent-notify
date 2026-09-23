use agentnotify_agent_sdk::{AgentError, AgentEventEnvelope, NormalizedAgentEvent};
use agentnotify_domain::{AgentId, AgentSessionId, NotificationMetadata, Timestamp};
use serde_json::Value;

use crate::descriptor::OPENCODE_AGENT_ID;

/// 插件当前提交的终态事件类型。
const COMPLETED_EVENT: &str = "session.completed";
/// OpenCode 原生终态事件名：插件改用原生名或事件名漂移时照样推送，
/// 与 Codex / Devin / Command Code 的容错口径对称，避免改名导致通知静默停止。
const KNOWN_TERMINAL_EVENTS: [&str; 4] = [
    "session.idle",
    "session.error",
    "session.execution.succeeded",
    "session.execution.failed",
];
const EVENT_TYPE_FIELD: &str = "eventType";

pub fn parse_event(envelope: AgentEventEnvelope) -> Result<NormalizedAgentEvent, AgentError> {
    let expected_agent = AgentId::new(OPENCODE_AGENT_ID).expect("OpenCode Agent ID 是固定有效值");
    if envelope.agent_id != expected_agent {
        return Err(AgentError::InvalidEvent);
    }

    let payload = envelope
        .payload
        .as_object()
        .ok_or(AgentError::InvalidEvent)?;
    if !is_terminal_event(payload)? {
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

/// `eventType` 缺失或为空时按 `session.completed` 处理；已知终态类型放行；
/// 存在但未知（如 `session.started`）时拒绝，避免误收非终态事件。
fn is_terminal_event(payload: &serde_json::Map<String, Value>) -> Result<bool, AgentError> {
    match payload.get(EVENT_TYPE_FIELD) {
        None | Some(Value::Null) => Ok(true),
        Some(Value::String(value)) => {
            let trimmed = value.trim();
            Ok(trimmed.is_empty()
                || trimmed == COMPLETED_EVENT
                || KNOWN_TERMINAL_EVENTS.contains(&trimmed))
        }
        Some(_) => Err(AgentError::InvalidEvent),
    }
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
