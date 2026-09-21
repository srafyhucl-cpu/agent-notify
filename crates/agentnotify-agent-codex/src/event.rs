use std::path::Path;

use agentnotify_agent_sdk::{AgentError, AgentEventEnvelope, NormalizedAgentEvent};
use agentnotify_domain::{AgentId, AgentSessionId, NotificationMetadata, Timestamp};
use serde_json::{Map, Value};

use crate::{descriptor::CODEX_AGENT_ID, title::resolve_title};

/// Codex 常见的完成事件类型，仅作信息字段。
///
/// Go 版只把 `type` 用于线程 ID 归类，从不因类型未知而跳过推送；这里同样不拒绝未知类型，
/// 避免 Codex 改名或新增事件类型时静默停止通知。
pub const TURN_COMPLETE_EVENT: &str = "agent-turn-complete";
/// 缺失 `last-assistant-message` 时的正文，与 Go 版默认正文一致。
pub const DEFAULT_BODY: &str = "任务已完成。";
const THREAD_ID_FIELD: &str = "thread-id";
/// `thread_id` 只是兼容别名，`thread-id` 存在时永远优先。
const THREAD_ID_ALIAS_FIELD: &str = "thread_id";
const BODY_FIELD: &str = "last-assistant-message";
const INPUT_MESSAGES_FIELD: &str = "input-messages";
const TURN_ID_FIELD: &str = "turn-id";
/// 正文里的降级标记，与 Go 版的 `> ⚠️` 提示一致。
const DEGRADATION_PREFIX: &str = "\n\n> ⚠️ ";

pub fn parse_event(
    envelope: AgentEventEnvelope,
    codex_home: Option<&Path>,
) -> Result<NormalizedAgentEvent, AgentError> {
    let expected_agent = AgentId::new(CODEX_AGENT_ID).expect("Codex Agent ID 是固定有效值");
    if envelope.agent_id != expected_agent {
        return Err(AgentError::InvalidEvent);
    }
    let payload = envelope
        .payload
        .as_object()
        .ok_or(AgentError::InvalidEvent)?;

    let thread_id = thread_id(payload);
    let summary = optional_text(payload, BODY_FIELD)?;
    let payload_title = first_input_message(payload)?;
    let resolution = resolve_title(codex_home, thread_id.as_deref(), payload_title.as_deref());

    let session_id = thread_id
        .as_deref()
        .map(|value| AgentSessionId::new(value).map_err(|_| AgentError::InvalidEvent))
        .transpose()?;
    let turn_id = optional_text(payload, TURN_ID_FIELD)?;

    Ok(NormalizedAgentEvent {
        idempotency_key: idempotency_key(thread_id.as_deref(), turn_id.as_deref()),
        occurred_at: Timestamp::now_utc(),
        session_id,
        session_title: Some(resolution.name.clone()),
        title: resolution.name,
        body: compose_body(summary.as_deref(), resolution.degradation.as_deref()),
        metadata: NotificationMetadata::default(),
    })
}

/// 缺失或非法的线程 ID 只会让通知失去引用路由，不会丢弃事件。
fn thread_id(payload: &Map<String, Value>) -> Option<String> {
    thread_id_value(payload.get(THREAD_ID_FIELD))
        .or_else(|| thread_id_value(payload.get(THREAD_ID_ALIAS_FIELD)))
}

fn thread_id_value(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(text) => clean_text(text),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

/// 标题链最后一级的 payload 来源：首条 `input-messages`。
fn first_input_message(payload: &Map<String, Value>) -> Result<Option<String>, AgentError> {
    match payload.get(INPUT_MESSAGES_FIELD) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(items)) => Ok(items.first().and_then(Value::as_str).and_then(clean_text)),
        Some(_) => Err(AgentError::InvalidEvent),
    }
}

fn compose_body(summary: Option<&str>, degradation: Option<&str>) -> String {
    let mut body = summary
        .map(str::to_owned)
        .unwrap_or_else(|| DEFAULT_BODY.to_owned());
    if let Some(degradation) = degradation {
        body.push_str(DEGRADATION_PREFIX);
        body.push_str(degradation);
    }
    body
}

/// 同一轮事件重复投递时按线程与 turn 去重；没有 turn ID 时交给入口 requestId。
fn idempotency_key(thread_id: Option<&str>, turn_id: Option<&str>) -> Option<String> {
    let turn_id = turn_id?;
    Some(match thread_id {
        Some(thread) => format!("codex:{thread}:{turn_id}"),
        None => format!("codex:turn:{turn_id}"),
    })
}

fn optional_text(payload: &Map<String, Value>, field: &str) -> Result<Option<String>, AgentError> {
    match payload.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(clean_text(value)),
        Some(_) => Err(AgentError::InvalidEvent),
    }
}

fn clean_text(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}
