use agentnotify_agent_sdk::{AgentError, AgentEventEnvelope, NormalizedAgentEvent};
use agentnotify_domain::{AgentId, AgentSessionId, NotificationMetadata, Timestamp};
use serde_json::{Map, Value};

use crate::{descriptor::COMMANDCODE_AGENT_ID, title::CommandCodeSessions};

/// mod 在每轮任务结束时提交的事件类型。
pub const RUN_END_EVENT: &str = "run_end";
/// 缺失正文时的默认正文，避免通知正文为空。
pub const DEFAULT_BODY: &str = "任务已完成。";
const EVENT_TYPE_FIELD: &str = "eventType";
const SESSION_ID_FIELD: &str = "sessionId";
const TITLE_FIELD: &str = "title";
const BODY_FIELD: &str = "body";
/// 正文里的降级标记，与 Go 版渲染出的 `> ⚠️` 提示一致。
const DEGRADATION_PREFIX: &str = "\n\n> ⚠️ ";

/// 只处理 Command Code `run_end`；会话号缺失时仍推送，但不生成引用路由。
///
/// 标题链与 Go 版 mod 一致：`session_titled` 标题 → meta.json → transcript 首条用户消息 →
/// 默认标题；`run_end` 没有稳定轮次 ID，去重交给入口 requestId（与 Devin Stop 事件相同）。
pub fn parse_event(
    envelope: AgentEventEnvelope,
    sessions: &CommandCodeSessions,
) -> Result<NormalizedAgentEvent, AgentError> {
    let expected_agent =
        AgentId::new(COMMANDCODE_AGENT_ID).expect("Command Code Agent ID 是固定有效值");
    if envelope.agent_id != expected_agent {
        return Err(AgentError::InvalidEvent);
    }
    let payload = envelope
        .payload
        .as_object()
        .ok_or(AgentError::InvalidEvent)?;
    if !is_run_end(payload)? {
        return Err(AgentError::InvalidEvent);
    }

    let session_id = optional_text(payload, SESSION_ID_FIELD)?
        .map(|value| AgentSessionId::new(value).map_err(|_| AgentError::InvalidEvent))
        .transpose()?;
    let title_hint = optional_text(payload, TITLE_FIELD)?;
    let resolution = sessions.resolve_title(
        session_id
            .as_ref()
            .map(AgentSessionId::as_str)
            .unwrap_or_default(),
        title_hint.as_deref(),
    );
    let body = compose_body(
        optional_text(payload, BODY_FIELD)?.as_deref(),
        resolution.degradation.as_deref(),
    );

    Ok(NormalizedAgentEvent {
        // run_end 没有轮次 ID，同一 run 的重复投递由入口 requestId 处理。
        idempotency_key: None,
        occurred_at: Timestamp::now_utc(),
        session_id,
        session_title: Some(resolution.name.clone()),
        title: resolution.name,
        body,
        metadata: NotificationMetadata::default(),
    })
}

/// `eventType` 缺失或为空时按 `run_end` 处理；存在但未知时拒绝，避免误收其他事件。
fn is_run_end(payload: &Map<String, Value>) -> Result<bool, AgentError> {
    match payload.get(EVENT_TYPE_FIELD) {
        None | Some(Value::Null) => Ok(true),
        Some(Value::String(value)) => Ok(value.trim().is_empty() || value.trim() == RUN_END_EVENT),
        Some(_) => Err(AgentError::InvalidEvent),
    }
}

fn optional_text(payload: &Map<String, Value>, field: &str) -> Result<Option<String>, AgentError> {
    match payload.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(clean_text(value)),
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

fn clean_text(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}
