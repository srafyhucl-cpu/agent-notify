use agentnotify_agent_sdk::{AgentError, AgentEventEnvelope, NormalizedAgentEvent};
use agentnotify_domain::{AgentId, AgentSessionId, NotificationMetadata, SafeError, Timestamp};
use serde_json::{Map, Value};

use crate::{descriptor::DEVIN_AGENT_ID, session::DevinSessions};

/// 标题链最后一级的默认标题，与 Go 版渲染层一致。
pub const DEFAULT_TITLE: &str = "任务已完成";
/// 缺失 `last_assistant_message` 时的正文，避免通知正文为空。
pub const DEFAULT_BODY: &str = "任务已完成。";

const SESSION_ID_FIELD: &str = "session_id";
const HOOK_EVENT_FIELD: &str = "hook_event_name";
const STOP_HOOK_ACTIVE_FIELD: &str = "stop_hook_active";
const LAST_ASSISTANT_MESSAGE_FIELD: &str = "last_assistant_message";
const STOP_EVENT: &str = "Stop";
/// 标题降级提示，文案与 Go 版一致，用户能在微信里看懂。
const TITLE_DEGRADATION: &str = "未能读取 Devin 会话标题，已使用默认标题。";
/// 正文里的降级标记，与 Go 版渲染出的 `> ⚠️` 提示一致。
const NOTE_PREFIX: &str = "\n\n> ⚠️ ";

/// 只处理非递归的 Devin Stop 事件；其余返回 Ignored，不产生通知。
pub fn parse_event(
    envelope: AgentEventEnvelope,
    sessions: &DevinSessions,
) -> Result<NormalizedAgentEvent, AgentError> {
    let expected_agent = AgentId::new(DEVIN_AGENT_ID).expect("Devin Agent ID 是固定有效值");
    if envelope.agent_id != expected_agent {
        return Err(AgentError::InvalidEvent);
    }
    let payload = envelope
        .payload
        .as_object()
        .ok_or(AgentError::InvalidEvent)?;

    // Devin 在 Stop Hook 内部再次触发 Stop 时标记 true；继续处理会造成递归推送。
    if stop_hook_active(payload)? {
        return Err(AgentError::Ignored(safe_error(
            "devin_stop_hook_active",
            "Devin 已在处理 Stop hook",
        )));
    }
    if !is_stop_event(payload)? {
        return Err(AgentError::Ignored(safe_error(
            "devin_hook_event_mismatch",
            "Devin hook 事件不是 Stop",
        )));
    }

    // 会话号只取稳定的 `session_id`；其他字段（prompt、Cascade 标识等）都不参与，
    // 缺失时宁可跳过也不回退到最近会话。
    let Some(session_id) = optional_text(payload, SESSION_ID_FIELD)? else {
        return Err(AgentError::Ignored(safe_error(
            "devin_session_missing",
            "Devin hook 缺少 session_id",
        )));
    };
    let session_id = AgentSessionId::new(session_id).map_err(|_| AgentError::InvalidEvent)?;

    let summary = optional_text(payload, LAST_ASSISTANT_MESSAGE_FIELD)?;
    let (title, degradation) = match sessions.lookup_title(session_id.as_str()) {
        Ok(title) => (title, None),
        Err(_) => (DEFAULT_TITLE.to_owned(), Some(TITLE_DEGRADATION)),
    };

    Ok(NormalizedAgentEvent {
        // Devin Stop 事件没有轮次 ID，去重交给入口 requestId。
        idempotency_key: None,
        occurred_at: Timestamp::now_utc(),
        session_id: Some(session_id),
        session_title: Some(title.clone()),
        title,
        body: compose_body(summary.as_deref(), degradation),
        metadata: NotificationMetadata::default(),
    })
}

/// `stop_hook_active` 缺失或为 false 时继续；类型非法按事件格式错误处理（与 Go 解码一致）。
fn stop_hook_active(payload: &Map<String, Value>) -> Result<bool, AgentError> {
    match payload.get(STOP_HOOK_ACTIVE_FIELD) {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(value)) => Ok(*value),
        Some(_) => Err(AgentError::InvalidEvent),
    }
}

/// `hook_event_name` 缺失或为空时按 Stop 处理；存在时必须（忽略大小写）是 `Stop`。
fn is_stop_event(payload: &Map<String, Value>) -> Result<bool, AgentError> {
    match payload.get(HOOK_EVENT_FIELD) {
        None | Some(Value::Null) => Ok(true),
        Some(Value::String(value)) => {
            Ok(clean_text(value).is_none_or(|value| value.eq_ignore_ascii_case(STOP_EVENT)))
        }
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
        body.push_str(NOTE_PREFIX);
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

fn safe_error(code: &str, message: &str) -> SafeError {
    SafeError::new(code, message).expect("Devin 错误常量必须是有效安全错误")
}
