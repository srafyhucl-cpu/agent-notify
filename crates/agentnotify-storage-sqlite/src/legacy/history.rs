use agentnotify_domain::{
    AgentId, AgentSessionId, ChannelAccountId, ChannelId, Delivery, DeliveryId, ExternalMessageId,
    Notification, NotificationId, NotificationMetadata, SafeError, Timestamp,
};
use serde::Deserialize;

use super::credentials::{redact, sha256_hex};
use super::{ImportWarning, LegacyImportError};

const HISTORY_FILE: &str = "push.log";
const UNKNOWN_AGENT_ID: &str = "legacy";
const MAX_SAFE_ERROR_CHARACTERS: usize = 400;

#[derive(Clone, Debug)]
pub(crate) struct LegacyHistoryRecord {
    pub notification: Notification,
    pub delivery: Delivery,
}

pub(crate) fn prepare_history(
    bytes: Option<&[u8]>,
    account_id: &ChannelAccountId,
    sensitive_values: &[String],
) -> Result<(Vec<LegacyHistoryRecord>, Vec<ImportWarning>, usize), LegacyImportError> {
    let Some(bytes) = bytes else {
        return Ok((Vec::new(), Vec::new(), 0));
    };

    let mut records = Vec::new();
    let mut warnings = Vec::new();
    let mut skipped = 0;
    for (index, raw_line) in bytes.split(|byte| *byte == b'\n').enumerate() {
        let line_number = index as u64 + 1;
        if raw_line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let raw = match serde_json::from_slice::<RawHistory>(raw_line) {
            Ok(raw) => raw,
            Err(_) => {
                skipped += 1;
                warnings.push(history_warning("legacy_history_invalid_json", line_number));
                continue;
            }
        };
        match history_from_raw(&raw, raw_line, account_id, sensitive_values) {
            Ok(record) => records.push(record),
            Err(code) => {
                skipped += 1;
                warnings.push(history_warning(code, line_number));
            }
        }
    }
    Ok((records, warnings, skipped))
}

fn history_from_raw(
    raw: &RawHistory,
    raw_line: &[u8],
    account_id: &ChannelAccountId,
    sensitive_values: &[String],
) -> Result<LegacyHistoryRecord, &'static str> {
    let title = raw
        .title
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or("legacy_history_title_missing")?;
    let body = raw
        .summary
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or("legacy_history_body_missing")?;
    let occurred_at = raw
        .timestamp
        .as_deref()
        .and_then(|value| Timestamp::parse_rfc3339(value).ok())
        .ok_or("legacy_history_timestamp_invalid")?;
    let agent_id = raw
        .agent
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(UNKNOWN_AGENT_ID);
    let agent_id = AgentId::new(agent_id).map_err(|_| "legacy_history_agent_invalid")?;
    let session_id = raw
        .session
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| AgentSessionId::new(value).map_err(|_| "legacy_history_session_invalid"))
        .transpose()?;

    let hash = sha256_hex(raw_line);
    let notification_id = NotificationId::new(format!("legacy-{hash}"))
        .map_err(|_| "legacy_history_identifier_invalid")?;
    let notification = Notification::new(
        notification_id.clone(),
        format!("legacy-history:{hash}"),
        agent_id,
        session_id,
        None,
        title,
        body,
        occurred_at,
        NotificationMetadata::default(),
    )
    .map_err(|_| "legacy_history_notification_invalid")?;

    let mut delivery = Delivery::pending(
        DeliveryId::new(format!("legacy-delivery-{hash}"))
            .map_err(|_| "legacy_history_identifier_invalid")?,
        notification_id,
        ChannelId::new("clawbot").expect("ClawBot 渠道 ID 是固定有效值"),
        account_id.clone(),
    );
    apply_status(&mut delivery, raw, sensitive_values)?;
    Ok(LegacyHistoryRecord {
        notification,
        delivery,
    })
}

fn apply_status(
    delivery: &mut Delivery,
    raw: &RawHistory,
    sensitive_values: &[String],
) -> Result<(), &'static str> {
    match raw.status.as_deref().unwrap_or_default().trim() {
        "成功" => {
            let external_id = raw
                .message_id
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .or_else(|| {
                    raw.client_id
                        .as_deref()
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                });
            if let Some(external_id) = external_id {
                let external_id = ExternalMessageId::new(external_id)
                    .map_err(|_| "legacy_history_message_id_invalid")?;
                delivery
                    .mark_sent(external_id)
                    .map_err(|_| "legacy_history_delivery_invalid")?;
            } else {
                delivery
                    .mark_unknown(safe_error(
                        "legacy_missing_message_id",
                        "旧版记录标记为成功，但没有可验证的消息 ID",
                    ))
                    .map_err(|_| "legacy_history_delivery_invalid")?;
            }
        }
        "失败" => {
            let detail = raw
                .error
                .as_deref()
                .map(|value| sanitize_error(value, sensitive_values))
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| "旧版未提供失败原因".into());
            delivery
                .mark_permanent_failure(safe_error(
                    "legacy_delivery_failed",
                    &format!("旧版投递失败：{detail}"),
                ))
                .map_err(|_| "legacy_history_delivery_invalid")?;
        }
        "未登录" => {
            delivery
                .mark_skipped(safe_error(
                    "legacy_not_logged_in",
                    "旧版记录显示当时未登录 ClawBot",
                ))
                .map_err(|_| "legacy_history_delivery_invalid")?;
        }
        "会话未建立" => {
            delivery
                .mark_skipped(safe_error(
                    "session_missing",
                    "旧版记录显示当时主动推送会话未建立",
                ))
                .map_err(|_| "legacy_history_delivery_invalid")?;
        }
        value if value.eq_ignore_ascii_case("dryrun") => {
            delivery
                .mark_skipped(safe_error("dry_run", "旧版记录为 DryRun，没有发送到微信"))
                .map_err(|_| "legacy_history_delivery_invalid")?;
        }
        _ => {
            delivery
                .mark_skipped(safe_error(
                    "legacy_unknown_status",
                    "旧版记录包含无法识别的投递状态",
                ))
                .map_err(|_| "legacy_history_delivery_invalid")?;
        }
    }
    Ok(())
}

fn safe_error(code: &'static str, message: &str) -> SafeError {
    let message = if message.chars().count() > MAX_SAFE_ERROR_CHARACTERS {
        message.chars().take(MAX_SAFE_ERROR_CHARACTERS).collect()
    } else {
        message.to_owned()
    };
    SafeError::new(code, message).expect("历史迁移错误码和截断后的文案始终有效")
}

fn sanitize_error(value: &str, sensitive_values: &[String]) -> String {
    redact(value, sensitive_values)
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn history_warning(code: &'static str, record: u64) -> ImportWarning {
    ImportWarning {
        code: code.into(),
        file: HISTORY_FILE.into(),
        record: Some(record),
    }
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawHistory {
    timestamp: Option<String>,
    agent: Option<String>,
    session: Option<String>,
    title: Option<String>,
    summary: Option<String>,
    status: Option<String>,
    error: Option<String>,
    #[serde(rename = "messageID")]
    message_id: Option<String>,
    #[serde(rename = "clientID")]
    client_id: Option<String>,
}
