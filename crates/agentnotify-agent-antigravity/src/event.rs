use std::path::Path;

use agentnotify_agent_sdk::{AgentError, AgentEventEnvelope, NormalizedAgentEvent};
use agentnotify_domain::{AgentId, NotificationMetadata, SafeError, Timestamp};
use serde_json::{Map, Value};

use crate::{
    descriptor::ANTIGRAVITY_AGENT_ID, reply::antigravity_resume_target, title::resolve_title,
    transcript::read_transcript_summary,
};

/// 无法从 transcript 得到摘要时的正文，避免通知正文为空。
pub const DEFAULT_BODY: &str = "任务已完成。";
const FULLY_IDLE_FIELD: &str = "fullyIdle";
const CONVERSATION_ID_FIELD: &str = "conversationId";
const TRANSCRIPT_PATH_FIELD: &str = "transcriptPath";
const ERROR_FIELD: &str = "error";
/// Antigravity 在结束时报告错误的提示，与 Go 版一致。
const ERROR_NOTICE: &str = "Antigravity 结束时报告了错误。";
/// 正文里的降级标记，与 Go 版渲染出的 `> ⚠️` 提示一致。
const NOTE_PREFIX: &str = "\n\n> ⚠️ ";
const NOTE_JOINER: &str = "\n> ";

/// 只处理 `fullyIdle=true` 且带 conversationId 的 Stop 事件；其余返回 Ignored，不产生通知。
pub fn parse_event(
    envelope: AgentEventEnvelope,
    annotations_dir: Option<&Path>,
) -> Result<NormalizedAgentEvent, AgentError> {
    let expected_agent =
        AgentId::new(ANTIGRAVITY_AGENT_ID).expect("Antigravity Agent ID 是固定有效值");
    if envelope.agent_id != expected_agent {
        return Err(AgentError::InvalidEvent);
    }
    let payload = envelope
        .payload
        .as_object()
        .ok_or(AgentError::InvalidEvent)?;

    if !fully_idle(payload)? {
        return Err(AgentError::Ignored(safe_error(
            "antigravity_not_idle",
            "Antigravity 尚未完全空闲",
        )));
    }

    let Some(conversation_id) = conversation_id(payload)? else {
        return Err(AgentError::Ignored(safe_error(
            "antigravity_conversation_missing",
            "Antigravity hook 缺少 conversationId",
        )));
    };

    let transcript_path = optional_text(payload, TRANSCRIPT_PATH_FIELD)?;
    let summary = read_transcript_summary(transcript_path.as_deref().map(Path::new));
    let resolution = resolve_title(
        annotations_dir,
        &conversation_id,
        transcript_path.as_deref(),
    );

    let mut notes = Vec::new();
    if has_json_value(payload.get(ERROR_FIELD)) {
        notes.push(ERROR_NOTICE.to_owned());
    }
    if let Some(degradation) = resolution.degradation {
        notes.push(degradation);
    }

    Ok(NormalizedAgentEvent {
        // Antigravity Stop 事件没有轮次 ID，去重交给入口 requestId，避免同一会话第二次完成被误判重复。
        idempotency_key: None,
        occurred_at: Timestamp::now_utc(),
        session_id: Some(antigravity_resume_target(&conversation_id)),
        session_title: Some(resolution.name.clone()),
        title: resolution.name,
        body: compose_body(&summary, &notes),
        metadata: NotificationMetadata::default(),
    })
}

/// `fullyIdle` 缺失或为 false 时跳过；类型非法按事件格式错误处理（与 Go 解码语义一致）。
fn fully_idle(payload: &Map<String, Value>) -> Result<bool, AgentError> {
    match payload.get(FULLY_IDLE_FIELD) {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(value)) => Ok(*value),
        Some(_) => Err(AgentError::InvalidEvent),
    }
}

/// conversationId 必须是非空字符串；缺失或空值只跳过，不算格式错误。
fn conversation_id(payload: &Map<String, Value>) -> Result<Option<String>, AgentError> {
    match payload.get(CONVERSATION_ID_FIELD) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(clean_text(value)),
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

/// 与 Go 版 `hasJSONValue` 相同的判定：空字符串、空数组、空对象与 null 不算错误。
fn has_json_value(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::String(text)) => !text.trim().is_empty(),
        Some(Value::Array(items)) => !items.is_empty(),
        Some(Value::Object(object)) => !object.is_empty(),
        Some(_) => true,
    }
}

fn compose_body(summary: &str, notes: &[String]) -> String {
    let mut body = if summary.trim().is_empty() {
        DEFAULT_BODY.to_owned()
    } else {
        summary.to_owned()
    };
    if !notes.is_empty() {
        body.push_str(NOTE_PREFIX);
        body.push_str(&notes.join(NOTE_JOINER));
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
    SafeError::new(code, message).expect("Antigravity 错误常量必须是有效安全错误")
}
