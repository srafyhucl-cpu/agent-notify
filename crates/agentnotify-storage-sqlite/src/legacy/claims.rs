use agentnotify_domain::{
    ChannelAccountId, ChannelId, ClaimKey, ClaimState, InboundClaim, Timestamp,
};
use serde::Deserialize;
use time::Duration;

use super::{ImportWarning, LegacyImportError};

const CLAIMS_FILE: &str = "reply-state.jsonl";
const CLAIM_TTL_DAYS: i64 = 30;

pub(crate) fn prepare_claims(
    bytes: Option<&[u8]>,
    account_id: Option<&ChannelAccountId>,
    imported_at: Timestamp,
) -> Result<(Vec<InboundClaim>, Vec<ImportWarning>, usize), LegacyImportError> {
    let Some(bytes) = bytes else {
        return Ok((Vec::new(), Vec::new(), 0));
    };

    let mut claims = Vec::new();
    let mut warnings = Vec::new();
    let mut skipped = 0;
    for (index, raw_line) in bytes.split(|byte| *byte == b'\n').enumerate() {
        let line_number = index as u64 + 1;
        if raw_line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let raw = match serde_json::from_slice::<RawClaim>(raw_line) {
            Ok(raw) => raw,
            Err(_) => {
                skipped += 1;
                warnings.push(claim_warning("legacy_claim_invalid_json", line_number));
                continue;
            }
        };
        let key = raw
            .key
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let timestamp = raw
            .timestamp
            .as_deref()
            .and_then(|value| Timestamp::parse_rfc3339(value).ok());
        let state = raw.status.as_deref().map(str::trim);
        let (Some(key), Some(timestamp), Some(state)) = (key, timestamp, state) else {
            skipped += 1;
            warnings.push(claim_warning("legacy_claim_invalid", line_number));
            continue;
        };

        let Some(account_id) = account_id else {
            return Err(LegacyImportError::claim_account_missing());
        };
        let Some(expires_at) = timestamp.checked_add(Duration::days(CLAIM_TTL_DAYS)) else {
            skipped += 1;
            warnings.push(claim_warning(
                "legacy_claim_expires_at_invalid",
                line_number,
            ));
            continue;
        };
        if expires_at <= imported_at {
            skipped += 1;
            warnings.push(claim_warning("legacy_claim_expired", line_number));
            continue;
        }

        let target_state = match state {
            "claimed" => ClaimState::Unknown,
            "sent" => ClaimState::Completed,
            "failed" => ClaimState::Failed,
            _ => {
                skipped += 1;
                warnings.push(claim_warning("legacy_claim_unknown_status", line_number));
                continue;
            }
        };
        let mut claim = match InboundClaim::new(
            ClaimKey::new(key).expect("已校验的旧 ClaimKey 始终有效"),
            ChannelId::new("clawbot").expect("ClawBot 渠道 ID 是固定有效值"),
            account_id.clone(),
            None,
            timestamp,
            expires_at,
        ) {
            Ok(claim) => claim,
            Err(_) => {
                skipped += 1;
                warnings.push(claim_warning("legacy_claim_invalid", line_number));
                continue;
            }
        };
        let update = match target_state {
            ClaimState::Completed => claim.mark_completed(timestamp),
            ClaimState::Failed => claim.mark_failed(timestamp),
            ClaimState::Unknown => claim.mark_unknown(timestamp),
            ClaimState::InProgress => unreachable!("旧 Claim 永远不导入为 InProgress"),
        };
        if update.is_err() {
            skipped += 1;
            warnings.push(claim_warning("legacy_claim_invalid", line_number));
            continue;
        }
        claims.push(claim);
    }

    Ok((claims, warnings, skipped))
}

fn claim_warning(code: &'static str, record: u64) -> ImportWarning {
    ImportWarning {
        code: code.into(),
        file: CLAIMS_FILE.into(),
        record: Some(record),
    }
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawClaim {
    key: Option<String>,
    status: Option<String>,
    timestamp: Option<String>,
}
