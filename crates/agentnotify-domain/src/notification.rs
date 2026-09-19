use std::collections::BTreeMap;

use crate::{AgentId, AgentSessionId, DomainError, NotificationId, Timestamp};

const MAX_METADATA_KEY_LENGTH: usize = 64;
const MAX_METADATA_VALUE_LENGTH: usize = 1024;
const SENSITIVE_METADATA_KEY_PARTS: [&str; 7] = [
    "authorization",
    "cookie",
    "credential",
    "password",
    "secret",
    "token",
    "body",
];

/// 只包含脱敏元数据的通知附加信息。
#[derive(Clone, Debug, Default, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(transparent)]
pub struct NotificationMetadata(BTreeMap<String, String>);

impl NotificationMetadata {
    pub fn new<I, K, V>(entries: I) -> Result<Self, DomainError>
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        let mut metadata = BTreeMap::new();
        for (key, value) in entries {
            let key = key.into();
            let value = value.into();
            validate_metadata_entry(&key, &value)?;
            metadata.insert(key, value);
        }
        Ok(Self(metadata))
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(String::as_str)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
    }

    pub fn into_inner(self) -> BTreeMap<String, String> {
        self.0
    }
}

/// Agent 产生的通知事实。
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Notification {
    pub id: NotificationId,
    pub ingest_key: String,
    pub agent_id: AgentId,
    pub session_id: Option<AgentSessionId>,
    pub session_title: Option<String>,
    pub title: String,
    pub body: String,
    pub occurred_at: Timestamp,
    pub metadata: NotificationMetadata,
}

impl Notification {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: NotificationId,
        ingest_key: impl Into<String>,
        agent_id: AgentId,
        session_id: Option<AgentSessionId>,
        session_title: Option<String>,
        title: impl Into<String>,
        body: impl Into<String>,
        occurred_at: Timestamp,
        metadata: NotificationMetadata,
    ) -> Result<Self, DomainError> {
        let ingest_key = required_trimmed(ingest_key.into(), "ingest_key")?;
        let title = required_trimmed(title.into(), "title")?;
        let body = body.into();
        if body.trim().is_empty() {
            return Err(DomainError::InvalidValue { field: "body" });
        }

        let session_title = session_title.and_then(|value| {
            let value = value.trim();
            if value.is_empty() {
                None
            } else {
                Some(value.to_owned())
            }
        });

        Ok(Self {
            id,
            ingest_key,
            agent_id,
            session_id,
            session_title,
            title,
            body,
            occurred_at,
            metadata,
        })
    }
}

fn required_trimmed(value: String, field: &'static str) -> Result<String, DomainError> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed != value {
        return Err(DomainError::InvalidValue { field });
    }
    Ok(value)
}

fn validate_metadata_entry(key: &str, value: &str) -> Result<(), DomainError> {
    if key.is_empty()
        || key.trim() != key
        || key.len() > MAX_METADATA_KEY_LENGTH
        || value.len() > MAX_METADATA_VALUE_LENGTH
        || value.contains(['\0', '\r', '\n'])
    {
        return Err(DomainError::InvalidMetadata);
    }

    let normalized = key.to_ascii_lowercase().replace('-', "_");
    if SENSITIVE_METADATA_KEY_PARTS
        .iter()
        .any(|part| normalized.contains(part))
    {
        return Err(DomainError::InvalidMetadata);
    }

    Ok(())
}
