use agentnotify_channel_clawbot::stable_account_id;
use agentnotify_channel_sdk::ChannelAccount;
use agentnotify_domain::{
    AgentId, AgentSessionId, ChannelAccountId, ChannelId, ExternalMessageId, ReplyRoute, RouteKey,
    Timestamp,
};
use serde::Deserialize;
use time::Duration;

use super::credentials::route_account;
use super::{ImportWarning, LegacyImportError};

const ROUTES_FILE: &str = "reply-routes.jsonl";
const DEFAULT_ROUTE_TTL_DAYS: i64 = 30;

#[derive(Clone, Debug)]
pub(crate) struct LegacyRouteRecord {
    pub route: ReplyRoute,
    pub additional_account: Option<ChannelAccount>,
}

pub(crate) fn prepare_routes(
    bytes: Option<&[u8]>,
    primary_account_id: Option<&ChannelAccountId>,
    imported_at: Timestamp,
) -> Result<(Vec<LegacyRouteRecord>, Vec<ImportWarning>, usize), LegacyImportError> {
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
        let raw = match serde_json::from_slice::<RawRoute>(raw_line) {
            Ok(raw) => raw,
            Err(_) => {
                skipped += 1;
                warnings.push(route_warning("legacy_route_invalid_json", line_number));
                continue;
            }
        };
        match route_from_raw(&raw, primary_account_id, imported_at) {
            RouteParse::Imported(record) => records.push(*record),
            RouteParse::Skipped(code) => {
                skipped += 1;
                warnings.push(route_warning(code, line_number));
            }
        }
    }
    Ok((records, warnings, skipped))
}

enum RouteParse {
    Imported(Box<LegacyRouteRecord>),
    Skipped(&'static str),
}

fn route_from_raw(
    raw: &RawRoute,
    primary_account_id: Option<&ChannelAccountId>,
    imported_at: Timestamp,
) -> RouteParse {
    let Some(bot_id) = non_empty(raw.bot_id.as_deref()) else {
        return RouteParse::Skipped("legacy_route_account_missing");
    };
    let Some(user_id) = non_empty(raw.user_id.as_deref()) else {
        return RouteParse::Skipped("legacy_route_account_missing");
    };
    let Some(agent_id) = non_empty(raw.agent.as_deref()) else {
        return RouteParse::Skipped("legacy_route_agent_missing");
    };
    let Some(session_id) = non_empty(raw.session_id.as_deref()) else {
        return RouteParse::Skipped("legacy_route_session_missing");
    };
    let external_message_id =
        non_empty(raw.message_id.as_deref()).or_else(|| non_empty(raw.client_id.as_deref()));
    let Some(external_message_id) = external_message_id else {
        return RouteParse::Skipped("legacy_route_message_id_missing");
    };
    let Ok(created_at) = raw
        .created_at
        .as_deref()
        .and_then(|value| Timestamp::parse_rfc3339(value).ok())
        .ok_or(())
    else {
        return RouteParse::Skipped("legacy_route_created_at_invalid");
    };
    let expires_at = match raw.expires_at.as_deref().map(Timestamp::parse_rfc3339) {
        Some(Ok(value)) => value,
        Some(Err(_)) => return RouteParse::Skipped("legacy_route_expires_at_invalid"),
        None => match created_at.checked_add(Duration::days(DEFAULT_ROUTE_TTL_DAYS)) {
            Some(value) => value,
            None => return RouteParse::Skipped("legacy_route_expires_at_invalid"),
        },
    };
    if expires_at <= imported_at {
        return RouteParse::Skipped("legacy_route_expired");
    }

    let Ok(account_id) = stable_account_id(bot_id, user_id) else {
        return RouteParse::Skipped("legacy_route_account_invalid");
    };
    let Ok(agent_id) = AgentId::new(agent_id) else {
        return RouteParse::Skipped("legacy_route_agent_invalid");
    };
    let Ok(session_id) = AgentSessionId::new(session_id) else {
        return RouteParse::Skipped("legacy_route_session_invalid");
    };
    let Ok(external_message_id) = ExternalMessageId::new(external_message_id) else {
        return RouteParse::Skipped("legacy_route_message_id_invalid");
    };

    let additional_account = if primary_account_id == Some(&account_id) {
        None
    } else {
        match route_account(bot_id, user_id, imported_at) {
            Ok(account) => Some(account),
            Err(_) => return RouteParse::Skipped("legacy_route_account_invalid"),
        }
    };
    RouteParse::Imported(Box::new(LegacyRouteRecord {
        route: ReplyRoute::new(
            RouteKey::new(
                ChannelId::new("clawbot").expect("ClawBot 渠道 ID 是固定有效值"),
                account_id,
                external_message_id,
            ),
            agent_id,
            session_id,
            created_at,
            expires_at,
        ),
        additional_account,
    }))
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn route_warning(code: &'static str, record: u64) -> ImportWarning {
    ImportWarning {
        code: code.into(),
        file: ROUTES_FILE.into(),
        record: Some(record),
    }
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawRoute {
    #[serde(rename = "messageID")]
    message_id: Option<String>,
    #[serde(rename = "clientID")]
    client_id: Option<String>,
    #[serde(rename = "botID")]
    bot_id: Option<String>,
    #[serde(rename = "userID")]
    user_id: Option<String>,
    agent: Option<String>,
    #[serde(rename = "sessionID")]
    session_id: Option<String>,
    #[serde(rename = "createdAt")]
    created_at: Option<String>,
    #[serde(rename = "expiresAt")]
    expires_at: Option<String>,
}
