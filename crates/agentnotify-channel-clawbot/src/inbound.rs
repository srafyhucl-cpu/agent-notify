use std::fmt::Write as _;

use agentnotify_channel_sdk::ChannelError;
use agentnotify_domain::{
    ChannelAccountId, ExternalMessageId, InboundMessage, InboundMessageId, Timestamp,
};
use serde::{Deserialize, Deserializer};
use sha2::{Digest, Sha256};

use crate::{descriptor::CLAWBOT_CHANNEL_ID, state::ClawBotCredentials};

const MESSAGE_TYPE_USER: i32 = 1;
const ITEM_TYPE_TEXT: i32 = 1;
const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";

/// `getupdates` 返回的单条消息；兼容平台历史上的字符串和数值 ID。
#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize)]
pub struct ClawBotInboundMessage {
    #[serde(default)]
    pub seq: i64,
    #[serde(default, deserialize_with = "deserialize_optional_string")]
    pub msg_id: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_string")]
    pub message_id: Option<String>,
    #[serde(default)]
    pub from_user_id: String,
    #[serde(default)]
    pub to_user_id: String,
    #[serde(default)]
    pub message_type: i32,
    #[serde(default)]
    pub message_state: i32,
    #[serde(default)]
    pub context_token: Option<String>,
    #[serde(default)]
    pub group_id: Option<String>,
    #[serde(default)]
    pub item_list: Vec<ClawBotMessageItem>,
    #[serde(default, deserialize_with = "deserialize_optional_string")]
    pub referenced_msg_id: Option<String>,
}

impl ClawBotInboundMessage {
    pub fn text(&self) -> String {
        self.item_list
            .iter()
            .find(|item| item.item_type == ITEM_TYPE_TEXT && item.text_item.is_some())
            .and_then(|item| item.text_item.as_ref())
            .map(|item| item.text.clone())
            .unwrap_or_default()
    }

    pub fn platform_message_id(&self) -> Option<String> {
        [
            self.msg_id.as_deref(),
            self.message_id.as_deref(),
            self.item_list
                .iter()
                .find_map(|item| item.msg_id.as_deref()),
        ]
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|value| !value.is_empty())
        .map(str::to_owned)
    }

    pub fn is_private_from(&self, bound_user_id: &str) -> bool {
        self.message_type == MESSAGE_TYPE_USER
            && self
                .group_id
                .as_deref()
                .is_none_or(|group_id| group_id.trim().is_empty())
            && self.from_user_id.trim() == bound_user_id.trim()
    }

    pub fn referenced_message_ids(&self) -> Result<Vec<String>, ChannelError> {
        let mut ids = Vec::new();
        push_unique(&mut ids, self.referenced_msg_id.as_deref());
        for item in &self.item_list {
            let Some(reference) = item.ref_msg.as_ref() else {
                continue;
            };
            if let Some(message_item) = reference.message_item.as_ref() {
                push_unique(&mut ids, message_item.msg_id.as_deref());
            }
            push_unique(&mut ids, reference.msg_id.as_deref());
            push_unique(&mut ids, reference.referenced_msg_id.as_deref());
        }
        if ids.len() > 1 {
            return Err(ChannelError::permanent(
                "clawbot_reference_conflict",
                "ClawBot 引用消息包含多个不一致的消息 ID，已停止转发",
            ));
        }
        Ok(ids)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize)]
pub struct ClawBotMessageItem {
    #[serde(rename = "type", default)]
    pub item_type: i32,
    #[serde(default, deserialize_with = "deserialize_optional_string")]
    pub msg_id: Option<String>,
    #[serde(default)]
    pub text_item: Option<ClawBotTextItem>,
    #[serde(default)]
    pub ref_msg: Option<ClawBotReference>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize)]
pub struct ClawBotTextItem {
    #[serde(default)]
    pub text: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize)]
pub struct ClawBotReference {
    #[serde(default)]
    pub message_item: Option<Box<ClawBotMessageItem>>,
    #[serde(default, deserialize_with = "deserialize_optional_string")]
    pub msg_id: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_string")]
    pub referenced_msg_id: Option<String>,
}

/// 将私聊入站消息归一化为核心领域对象，并生成旧 Go 版兼容的 ClaimKey。
pub fn normalize_inbound(
    account_id: &ChannelAccountId,
    credentials: &ClawBotCredentials,
    message: ClawBotInboundMessage,
    cursor: Option<&str>,
    received_at: Timestamp,
) -> Result<InboundMessage, ChannelError> {
    if !message.is_private_from(credentials.user_id()) {
        return Err(ChannelError::permanent(
            "clawbot_inbound_sender_mismatch",
            "ClawBot 入站消息不是来自当前账号绑定的私聊用户",
        ));
    }
    let text = message.text();
    if text.trim().is_empty() {
        return Err(ChannelError::permanent(
            "clawbot_inbound_text_empty",
            "ClawBot 入站消息没有可读取的文本正文",
        ));
    }
    let referenced_ids = message.referenced_message_ids()?;
    let external_ids = referenced_ids
        .iter()
        .map(|value| {
            ExternalMessageId::new(value.clone()).map_err(|_| {
                ChannelError::permanent("clawbot_reference_invalid", "ClawBot 引用消息 ID 格式无效")
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let conversation_id = message.from_user_id.trim().to_owned();
    let legacy_key = legacy_claim_key(credentials, &message, &text, &referenced_ids);

    let mut inbound = match message.platform_message_id() {
        Some(platform_id) => InboundMessage::new(
            InboundMessageId::new(uuid::Uuid::new_v4().to_string())
                .expect("UUID 字符串始终是有效标识"),
            agentnotify_domain::ChannelId::new(CLAWBOT_CHANNEL_ID)
                .expect("ClawBot 渠道 ID 是固定有效值"),
            account_id.clone(),
            agentnotify_domain::InboundMessageInput {
                external_message_id: ExternalMessageId::new(platform_id).map_err(|_| {
                    ChannelError::permanent(
                        "clawbot_inbound_message_id_invalid",
                        "ClawBot 入站消息 ID 格式无效",
                    )
                })?,
                sender_id: conversation_id.clone(),
                conversation_id: conversation_id.clone(),
                referenced_message_ids: external_ids,
                text,
                received_at,
            },
        )
        .map_err(map_domain_error)?,
        None => {
            let fallback_cursor = cursor
                .filter(|value| !value.trim().is_empty())
                .map(str::to_owned)
                .unwrap_or_else(|| format!("seq:{}|from:{conversation_id}", message.seq));
            let mut inbound = InboundMessage::without_external_id(
                agentnotify_domain::ChannelId::new(CLAWBOT_CHANNEL_ID)
                    .expect("ClawBot 渠道 ID 是固定有效值"),
                account_id.clone(),
                fallback_cursor,
                external_ids,
                text,
                received_at,
            )
            .map_err(map_domain_error)?;
            inbound.sender_id = conversation_id.clone();
            inbound.conversation_id = conversation_id.clone();
            inbound
        }
    };
    inbound = inbound
        .with_legacy_key(legacy_key)
        .map_err(map_domain_error)?;
    Ok(inbound)
}

fn legacy_claim_key(
    credentials: &ClawBotCredentials,
    message: &ClawBotInboundMessage,
    text: &str,
    referenced_ids: &[String],
) -> String {
    let mut scope_json = Vec::new();
    scope_json.extend_from_slice(b"{\"botID\":");
    push_go_json_string(credentials.bot_id(), &mut scope_json);
    scope_json.extend_from_slice(b",\"userID\":");
    push_go_json_string(credentials.user_id(), &mut scope_json);
    scope_json.push(b'}');
    let scope_hash = sha256_hex(&scope_json);

    if let Some(message_id) = message.platform_message_id() {
        return format!("account:{scope_hash}:message:{message_id}");
    }

    let referenced_id = referenced_ids
        .first()
        .map(String::as_str)
        .unwrap_or_default();
    let mut fallback_json = Vec::new();
    fallback_json.extend_from_slice(format!("{{\"seq\":{},", message.seq).as_bytes());
    fallback_json.extend_from_slice(b"\"fromUserID\":");
    push_go_json_string(message.from_user_id.trim(), &mut fallback_json);
    fallback_json.extend_from_slice(b",\"referencedID\":");
    push_go_json_string(referenced_id.trim(), &mut fallback_json);
    fallback_json.extend_from_slice(b",\"text\":");
    push_go_json_string(text, &mut fallback_json);
    fallback_json.push(b'}');
    let fallback_hash = sha256_hex(&fallback_json);
    format!("account:{scope_hash}:fallback:{fallback_hash}")
}

fn push_unique(values: &mut Vec<String>, candidate: Option<&str>) {
    let Some(candidate) = candidate.map(str::trim).filter(|value| !value.is_empty()) else {
        return;
    };
    if !values.iter().any(|value| value == candidate) {
        values.push(candidate.to_owned());
    }
}

fn sha256_hex(value: &[u8]) -> String {
    let digest = Sha256::digest(value);
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        encoded.push(HEX_DIGITS[(byte >> 4) as usize] as char);
        encoded.push(HEX_DIGITS[(byte & 0x0f) as usize] as char);
    }
    encoded
}

/// 与 Go `encoding/json` 的字符串转义保持一致，保证旧 ClaimKey 可精确匹配。
fn push_go_json_string(value: &str, output: &mut Vec<u8>) {
    output.push(b'"');
    for character in value.chars() {
        match character {
            '"' => output.extend_from_slice(b"\\\""),
            '\\' => output.extend_from_slice(b"\\\\"),
            '\u{0008}' => output.extend_from_slice(b"\\b"),
            '\u{000c}' => output.extend_from_slice(b"\\f"),
            '\n' => output.extend_from_slice(b"\\n"),
            '\r' => output.extend_from_slice(b"\\r"),
            '\t' => output.extend_from_slice(b"\\t"),
            '<' => output.extend_from_slice(b"\\u003c"),
            '>' => output.extend_from_slice(b"\\u003e"),
            '&' => output.extend_from_slice(b"\\u0026"),
            '\u{2028}' => output.extend_from_slice(b"\\u2028"),
            '\u{2029}' => output.extend_from_slice(b"\\u2029"),
            character if character < '\u{0020}' => {
                let mut encoded = String::new();
                write!(encoded, "\\u{:04x}", u32::from(character)).expect("写入 String 不会失败");
                output.extend_from_slice(encoded.as_bytes());
            }
            character => {
                let mut encoded = [0_u8; 4];
                output.extend_from_slice(character.encode_utf8(&mut encoded).as_bytes());
            }
        }
    }
    output.push(b'"');
}

fn map_domain_error(error: agentnotify_domain::DomainError) -> ChannelError {
    ChannelError::permanent("clawbot_inbound_invalid", error.message())
}

fn deserialize_optional_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    let Some(value) = value else {
        return Ok(None);
    };
    match value {
        serde_json::Value::Null => Ok(None),
        serde_json::Value::String(value) => {
            let value = value.trim();
            if value.is_empty() {
                Ok(None)
            } else {
                Ok(Some(value.to_owned()))
            }
        }
        serde_json::Value::Number(value) => Ok(Some(value.to_string())),
        _ => Err(serde::de::Error::custom(
            "ClawBot 协议字段必须是字符串或数字",
        )),
    }
}
