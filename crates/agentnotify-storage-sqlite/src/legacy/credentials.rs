use std::fmt::Write as _;

use agentnotify_application::{SecretKind, SecretValue};
use agentnotify_channel_clawbot::{
    CLAWBOT_CHANNEL_ID, ClawBotAccountState, ClawBotContext, ClawBotCredentials, ClawBotCursor,
    DEFAULT_BASE_URL, bot_token_secret_ref, stable_account_id,
};
use agentnotify_channel_sdk::ChannelAccount;
use agentnotify_domain::{ChannelAccountId, ChannelId, Timestamp};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::{ImportWarning, LegacyImportError};

const PLATFORM_ID_HINT_CHARACTERS: usize = 6;

/// 旧 ClawBot 凭据导入后的账号元数据和待写入密钥。
#[derive(Clone, Debug)]
pub(crate) struct LegacyClawBotImport {
    pub account: Option<ChannelAccount>,
    pub account_id: Option<ChannelAccountId>,
    pub secrets: Vec<LegacySecret>,
}

#[derive(Clone, Debug)]
pub(crate) struct LegacySecret {
    pub account_id: ChannelAccountId,
    pub kind: SecretKind,
    pub value: SecretValue,
}

pub(crate) fn prepare_credentials(
    bytes: Option<&[u8]>,
    imported_at: Timestamp,
) -> Result<(LegacyClawBotImport, Vec<ImportWarning>), LegacyImportError> {
    let Some(bytes) = bytes else {
        return Ok((
            LegacyClawBotImport {
                account: None,
                account_id: None,
                secrets: Vec::new(),
            },
            Vec::new(),
        ));
    };

    let raw = serde_json::from_slice::<RawCredentials>(bytes).map_err(|_| {
        LegacyImportError::invalid_field(
            "legacy_credentials_invalid",
            "clawbot.json",
            "旧版 ClawBot 凭据不是有效的 JSON 对象",
        )
    })?;

    let bot_token = required_field(raw.bot_token, "clawbot.json", "bot_token")?;
    let bot_id = required_field(raw.ilink_bot_id, "clawbot.json", "ilink_bot_id")?;
    let user_id = required_field(raw.ilink_user_id, "clawbot.json", "ilink_user_id")?;
    let base_url = optional_field(raw.baseurl, "clawbot.json", "baseurl")?
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_BASE_URL.to_owned());
    let get_updates_buf =
        optional_field(raw.get_updates_buf, "clawbot.json", "get_updates_buf")?.unwrap_or_default();
    let stale_at = optional_timestamp(raw.stale_at, "clawbot.json", "stale_at")?;
    let session_established_at = optional_timestamp(
        raw.session_established_at,
        "clawbot.json",
        "session_established_at",
    )?;
    let session_alert_at =
        optional_timestamp(raw.session_alert_at, "clawbot.json", "session_alert_at")?;
    let context_token =
        optional_field(raw.context_token, "clawbot.json", "context_token")?.unwrap_or_default();
    let context_user_id =
        optional_field(raw.context_user_id, "clawbot.json", "context_user_id")?.unwrap_or_default();

    let account_id = stable_account_id(&bot_id, &user_id).map_err(|_| {
        LegacyImportError::invalid_field(
            "legacy_credentials_invalid",
            "clawbot.json",
            "ClawBot 平台账号标识无效",
        )
    })?;
    let mut warnings = Vec::new();
    let mut secrets = Vec::new();

    let credentials =
        ClawBotCredentials::new(&bot_token, &bot_id, &user_id, &base_url).map_err(|_| {
            LegacyImportError::invalid_field(
                "legacy_credentials_invalid",
                "clawbot.json",
                "ClawBot 登录凭据字段无效",
            )
        })?;
    secrets.push(LegacySecret {
        account_id: account_id.clone(),
        kind: SecretKind::BotToken,
        value: SecretValue::new(serde_json::to_string(&credentials).map_err(|_| {
            LegacyImportError::invalid_field(
                "legacy_credentials_invalid",
                "clawbot.json",
                "ClawBot 登录凭据无法编码",
            )
        })?)
        .map_err(|_| {
            LegacyImportError::invalid_field(
                "legacy_credentials_invalid",
                "clawbot.json",
                "ClawBot 登录凭据为空",
            )
        })?,
    });

    if !context_token.is_empty() {
        if context_user_id == user_id {
            let context = ClawBotContext::new(&context_token, &context_user_id).map_err(|_| {
                LegacyImportError::invalid_field(
                    "legacy_credentials_invalid",
                    "clawbot.json",
                    "ClawBot 会话凭据无效",
                )
            })?;
            secrets.push(LegacySecret {
                account_id: account_id.clone(),
                kind: SecretKind::ContextToken,
                value: SecretValue::new(serde_json::to_string(&context).map_err(|_| {
                    LegacyImportError::invalid_field(
                        "legacy_credentials_invalid",
                        "clawbot.json",
                        "ClawBot 会话凭据无法编码",
                    )
                })?)
                .map_err(|_| {
                    LegacyImportError::invalid_field(
                        "legacy_credentials_invalid",
                        "clawbot.json",
                        "ClawBot 会话凭据为空",
                    )
                })?,
            });
        } else {
            warnings.push(ImportWarning {
                code: "legacy_context_token_scope_mismatch".into(),
                file: "clawbot.json".into(),
                record: None,
            });
        }
    }

    let state = ClawBotAccountState {
        bot_id_hint: tail_hint(&bot_id),
        user_id_hint: tail_hint(&user_id),
        base_url,
        stale_at,
        session_established_at,
        session_alert_at,
    };
    let mut account = ChannelAccount::new(
        account_id.clone(),
        ChannelId::new(CLAWBOT_CHANNEL_ID).expect("ClawBot 渠道 ID 是固定有效值"),
        format!("ClawBot 微信 · ...{}", state.user_id_hint),
        imported_at,
    );
    account.enabled = true;
    account.config = serde_json::to_value(&state).map_err(|_| {
        LegacyImportError::invalid_field(
            "legacy_credentials_invalid",
            "clawbot.json",
            "ClawBot 账号状态无法编码",
        )
    })?;
    account.secret_ref = Some(bot_token_secret_ref(&account_id).map_err(|_| {
        LegacyImportError::invalid_field(
            "legacy_credentials_invalid",
            "clawbot.json",
            "ClawBot 密钥引用无效",
        )
    })?);
    account.cursor = serde_json::to_value(ClawBotCursor::new(get_updates_buf)).map_err(|_| {
        LegacyImportError::invalid_field(
            "legacy_credentials_invalid",
            "clawbot.json",
            "ClawBot 游标无法编码",
        )
    })?;

    Ok((
        LegacyClawBotImport {
            account: Some(account),
            account_id: Some(account_id),
            secrets,
        },
        warnings,
    ))
}

pub(crate) fn unbound_account(imported_at: Timestamp) -> ChannelAccount {
    let account_id =
        ChannelAccountId::new("clawbot-unbound").expect("固定 ClawBot 未绑定账号 ID 始终有效");
    let mut account = ChannelAccount::new(
        account_id,
        ChannelId::new(CLAWBOT_CHANNEL_ID).expect("ClawBot 渠道 ID 是固定有效值"),
        "ClawBot 微信 · 未绑定",
        imported_at,
    );
    account.enabled = false;
    account.config = json!({
        "bot_id_hint": "",
        "user_id_hint": "",
        "base_url": DEFAULT_BASE_URL,
        "stale_at": null,
        "session_established_at": null,
        "session_alert_at": null
    });
    account
}

pub(crate) fn route_account(
    bot_id: &str,
    user_id: &str,
    imported_at: Timestamp,
) -> Result<ChannelAccount, LegacyImportError> {
    let account_id = stable_account_id(bot_id, user_id).map_err(|_| {
        LegacyImportError::invalid_field(
            "legacy_route_invalid",
            "reply-routes.jsonl",
            "回复路由中的 ClawBot 账号标识无效",
        )
    })?;
    let mut account = ChannelAccount::new(
        account_id,
        ChannelId::new(CLAWBOT_CHANNEL_ID).expect("ClawBot 渠道 ID 是固定有效值"),
        format!("ClawBot 微信 · ...{}", tail_hint(user_id)),
        imported_at,
    );
    account.enabled = false;
    account.config = json!({
        "bot_id_hint": tail_hint(bot_id),
        "user_id_hint": tail_hint(user_id),
        "base_url": DEFAULT_BASE_URL,
        "stale_at": null,
        "session_established_at": null,
        "session_alert_at": null
    });
    Ok(account)
}

fn required_field(
    value: Option<Value>,
    file: &'static str,
    field: &'static str,
) -> Result<String, LegacyImportError> {
    optional_field(value, file, field)?
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            LegacyImportError::invalid_field(
                "legacy_credentials_invalid",
                file,
                format!("旧版 ClawBot 凭据缺少字段 {field}"),
            )
        })
}

fn optional_field(
    value: Option<Value>,
    file: &'static str,
    field: &'static str,
) -> Result<Option<String>, LegacyImportError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.trim().to_owned())),
        Some(Value::Number(value)) => Ok(Some(value.to_string())),
        Some(_) => Err(LegacyImportError::invalid_field(
            "legacy_credentials_invalid",
            file,
            format!("旧版 ClawBot 凭据字段 {field} 类型无效"),
        )),
    }
}

fn optional_timestamp(
    value: Option<Value>,
    file: &'static str,
    field: &'static str,
) -> Result<Option<Timestamp>, LegacyImportError> {
    let Some(value) = optional_field(value, file, field)?.filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    Timestamp::parse_rfc3339(&value).map(Some).map_err(|_| {
        LegacyImportError::invalid_field(
            "legacy_credentials_invalid",
            file,
            format!("旧版 ClawBot 凭据字段 {field} 时间格式无效"),
        )
    })
}

fn tail_hint(value: &str) -> String {
    let mut characters = value
        .chars()
        .rev()
        .take(PLATFORM_ID_HINT_CHARACTERS)
        .collect::<Vec<_>>();
    characters.reverse();
    characters.into_iter().collect()
}

pub(crate) fn redact(value: &str, secrets: &[String]) -> String {
    let mut redacted = value.to_owned();
    for secret in secrets {
        if !secret.is_empty() {
            redacted = redacted.replace(secret, "[REDACTED]");
        }
    }
    redacted
}

pub(crate) fn sha256_hex(value: &[u8]) -> String {
    let digest = Sha256::digest(value);
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(encoded, "{:02x}", byte);
    }
    encoded
}

#[derive(Default, Deserialize)]
struct RawCredentials {
    bot_token: Option<Value>,
    ilink_bot_id: Option<Value>,
    baseurl: Option<Value>,
    ilink_user_id: Option<Value>,
    context_token: Option<Value>,
    context_user_id: Option<Value>,
    get_updates_buf: Option<Value>,
    stale_at: Option<Value>,
    session_established_at: Option<Value>,
    session_alert_at: Option<Value>,
}
