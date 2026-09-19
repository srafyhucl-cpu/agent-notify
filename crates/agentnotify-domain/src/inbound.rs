use std::fmt::Display;

use sha2::{Digest, Sha256};

use crate::{
    ChannelAccountId, ChannelId, DomainError, ExternalMessageId, InboundMessageId, Timestamp,
};

const SHA256_HEX_LENGTH: usize = 64;
const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";

/// 不额外保存正文副本的 Claim 材料。
#[derive(Clone, Debug, Eq, PartialEq)]
struct ClaimMaterial {
    cursor: Option<String>,
    referenced_message_ids: Vec<ExternalMessageId>,
    text_hash: [u8; 32],
    legacy_key: Option<String>,
}

/// 标准化后的入站回复消息。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InboundMessage {
    pub id: InboundMessageId,
    pub channel_id: ChannelId,
    pub account_id: ChannelAccountId,
    pub external_message_id: Option<ExternalMessageId>,
    pub sender_id: String,
    pub conversation_id: String,
    pub referenced_message_ids: Vec<ExternalMessageId>,
    pub text: String,
    pub received_at: Timestamp,
    claim_material: ClaimMaterial,
}

/// 渠道已确认拥有稳定消息 ID 时的入站消息字段。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InboundMessageInput {
    pub external_message_id: ExternalMessageId,
    pub sender_id: String,
    pub conversation_id: String,
    pub referenced_message_ids: Vec<ExternalMessageId>,
    pub text: String,
    pub received_at: Timestamp,
}

impl InboundMessage {
    /// 使用渠道提供的稳定消息 ID 构造入站消息。
    pub fn new(
        id: InboundMessageId,
        channel_id: ChannelId,
        account_id: ChannelAccountId,
        input: InboundMessageInput,
    ) -> Result<Self, DomainError> {
        let InboundMessageInput {
            external_message_id,
            sender_id,
            conversation_id,
            referenced_message_ids,
            text,
            received_at,
        } = input;
        validate_text(&text)?;
        let text_hash = sha256(text.as_bytes());
        Ok(Self {
            id,
            channel_id,
            account_id,
            external_message_id: Some(external_message_id),
            sender_id,
            conversation_id,
            referenced_message_ids,
            text,
            received_at,
            claim_material: ClaimMaterial {
                cursor: None,
                referenced_message_ids: Vec::new(),
                text_hash,
                legacy_key: None,
            },
        })
    }

    /// 在渠道没有稳定消息 ID 时，使用游标构造确定性回退材料。
    pub fn without_external_id(
        channel_id: ChannelId,
        account_id: ChannelAccountId,
        cursor: impl Into<String>,
        referenced_message_ids: Vec<ExternalMessageId>,
        text: impl Into<String>,
        received_at: Timestamp,
    ) -> Result<Self, DomainError> {
        let cursor = normalize_cursor(cursor.into())?;
        let text = text.into();
        validate_text(&text)?;
        let text_hash = sha256(text.as_bytes());
        Ok(Self {
            id: InboundMessageId::new(uuid::Uuid::new_v4().to_string())
                .expect("UUID 字符串始终是有效标识"),
            channel_id,
            account_id,
            external_message_id: None,
            sender_id: String::new(),
            conversation_id: String::new(),
            referenced_message_ids: referenced_message_ids.clone(),
            text,
            received_at,
            claim_material: ClaimMaterial {
                cursor: Some(cursor),
                referenced_message_ids,
                text_hash,
                legacy_key: None,
            },
        })
    }

    /// 为适配器提供旧版去重键，优先用于升级期间的重复拦截。
    pub fn with_legacy_key(mut self, legacy_key: impl Into<String>) -> Result<Self, DomainError> {
        let legacy_key = legacy_key.into();
        if legacy_key.trim().is_empty() {
            return Err(DomainError::InvalidValue {
                field: "legacy_key",
            });
        }
        self.claim_material.legacy_key = Some(legacy_key);
        Ok(self)
    }

    pub fn claim_key(&self) -> Result<ClaimKey, DomainError> {
        ClaimKey::from_inbound(self)
    }
}

/// 入站消息的持久化去重键。
#[derive(Clone, Debug, Eq, Hash, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(transparent)]
pub struct ClaimKey(String);

impl ClaimKey {
    /// 从持久化记录恢复去重键。
    pub fn new(value: impl Into<String>) -> Result<Self, DomainError> {
        Self::parse(value)
    }

    /// 从持久化记录恢复去重键；旧版迁移键允许非当前格式。
    pub fn parse(value: impl Into<String>) -> Result<Self, DomainError> {
        let value = value.into();
        if value.trim().is_empty() || value.trim() != value {
            return Err(DomainError::InvalidValue { field: "claim_key" });
        }
        Ok(Self(value))
    }

    /// 生成当前格式的去重键；提供 legacy_key 时保留旧键原值以兼容既有状态。
    pub fn from_inbound(message: &InboundMessage) -> Result<Self, DomainError> {
        if let Some(legacy_key) = &message.claim_material.legacy_key {
            return Ok(Self(legacy_key.clone()));
        }

        let value = if let Some(external_message_id) = &message.external_message_id {
            let mut digest = Sha256::new();
            digest.update(b"message");
            digest.update([0]);
            digest.update(message.channel_id.as_str().as_bytes());
            digest.update([0]);
            digest.update(message.account_id.as_str().as_bytes());
            digest.update([0]);
            digest.update(external_message_id.as_str().as_bytes());
            encode_hex(digest.finalize().as_slice())
        } else {
            let cursor = message
                .claim_material
                .cursor
                .as_deref()
                .ok_or(DomainError::InvalidValue { field: "cursor" })?;
            let joined_reference_ids = message
                .claim_material
                .referenced_message_ids
                .iter()
                .map(ExternalMessageId::as_str)
                .collect::<Vec<_>>()
                .join("\0");
            let text_hash = encode_hex(&message.claim_material.text_hash);

            let mut digest = Sha256::new();
            digest.update(b"fallback");
            digest.update([0]);
            digest.update(message.channel_id.as_str().as_bytes());
            digest.update([0]);
            digest.update(message.account_id.as_str().as_bytes());
            digest.update([0]);
            digest.update(cursor.as_bytes());
            digest.update([0]);
            digest.update(joined_reference_ids.as_bytes());
            digest.update([0]);
            digest.update(text_hash.as_bytes());
            encode_hex(digest.finalize().as_slice())
        };

        debug_assert_eq!(value.len(), SHA256_HEX_LENGTH);
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for ClaimKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

/// 入站消息至多一次分发的持久化状态。
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum ClaimState {
    InProgress,
    Completed,
    Failed,
    Unknown,
}

impl ClaimState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InProgress => "InProgress",
            Self::Completed => "Completed",
            Self::Failed => "Failed",
            Self::Unknown => "Unknown",
        }
    }

    pub fn parse(value: &str) -> Result<Self, DomainError> {
        match value {
            "InProgress" => Ok(Self::InProgress),
            "Completed" => Ok(Self::Completed),
            "Failed" => Ok(Self::Failed),
            "Unknown" => Ok(Self::Unknown),
            _ => Err(DomainError::InvalidValue {
                field: "claim_state",
            }),
        }
    }
}

/// 一条已持久化的入站 Claim。
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct InboundClaim {
    pub key: ClaimKey,
    pub channel_id: ChannelId,
    pub account_id: ChannelAccountId,
    pub external_message_id: Option<ExternalMessageId>,
    pub state: ClaimState,
    pub received_at: Timestamp,
    pub updated_at: Timestamp,
    pub expires_at: Timestamp,
}

impl InboundClaim {
    pub fn new(
        key: ClaimKey,
        channel_id: ChannelId,
        account_id: ChannelAccountId,
        external_message_id: Option<ExternalMessageId>,
        received_at: Timestamp,
        expires_at: Timestamp,
    ) -> Result<Self, DomainError> {
        if expires_at <= received_at {
            return Err(DomainError::InvalidValue {
                field: "expires_at",
            });
        }
        Ok(Self {
            key,
            channel_id,
            account_id,
            external_message_id,
            state: ClaimState::InProgress,
            received_at,
            updated_at: received_at,
            expires_at,
        })
    }

    pub fn from_inbound(
        message: &InboundMessage,
        expires_at: Timestamp,
    ) -> Result<Self, DomainError> {
        Self::new(
            ClaimKey::from_inbound(message)?,
            message.channel_id.clone(),
            message.account_id.clone(),
            message.external_message_id.clone(),
            message.received_at,
            expires_at,
        )
    }

    pub fn mark_completed(&mut self, updated_at: Timestamp) -> Result<(), DomainError> {
        self.finish(ClaimState::Completed, updated_at)
    }

    pub fn mark_failed(&mut self, updated_at: Timestamp) -> Result<(), DomainError> {
        self.finish(ClaimState::Failed, updated_at)
    }

    pub fn mark_unknown(&mut self, updated_at: Timestamp) -> Result<(), DomainError> {
        self.finish(ClaimState::Unknown, updated_at)
    }

    fn finish(&mut self, state: ClaimState, updated_at: Timestamp) -> Result<(), DomainError> {
        if self.state != ClaimState::InProgress || updated_at < self.updated_at {
            return Err(DomainError::InvalidStateTransition);
        }
        self.state = state;
        self.updated_at = updated_at;
        Ok(())
    }
}

/// Claim 写入结果；任何已存在状态都禁止再次执行 Agent。
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum ClaimOutcome {
    Acquired(InboundClaim),
    AlreadyClaimed {
        state: ClaimState,
        updated_at: Timestamp,
    },
}

impl ClaimOutcome {
    pub const fn may_execute_agent(&self) -> bool {
        matches!(self, Self::Acquired(_))
    }
}

fn validate_text(text: &str) -> Result<(), DomainError> {
    if text.trim().is_empty() {
        return Err(DomainError::InvalidValue { field: "text" });
    }
    Ok(())
}

fn normalize_cursor(cursor: String) -> Result<String, DomainError> {
    let cursor = cursor.trim();
    if cursor.is_empty() {
        return Err(DomainError::InvalidValue { field: "cursor" });
    }
    Ok(cursor.to_owned())
}

fn sha256(value: &[u8]) -> [u8; 32] {
    Sha256::digest(value).into()
}

fn encode_hex(value: &[u8]) -> String {
    let mut encoded = String::with_capacity(value.len() * 2);
    for byte in value {
        encoded.push(HEX_DIGITS[(byte >> 4) as usize] as char);
        encoded.push(HEX_DIGITS[(byte & 0x0f) as usize] as char);
    }
    encoded
}
