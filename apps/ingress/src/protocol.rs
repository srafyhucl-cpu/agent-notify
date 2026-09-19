use std::fmt::Display;

use agentnotify_agent_sdk::AgentEventEnvelope;
use agentnotify_domain::{AgentId, RequestId};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

pub const PROTOCOL_VERSION: u8 = 1;
pub const EVENT_KIND: &str = "agent.event";
pub const MAX_PROTOCOL_BYTES: usize = 320 * 1024;
pub const MAX_PAYLOAD_BYTES: usize = 256 * 1024;
pub const MAX_BODY_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IngressError {
    InputTooLarge,
    InvalidJson,
    UnsupportedVersion,
    UnsupportedKind,
    InvalidRequestId,
    InvalidAgentId,
    InvalidPayload,
    PayloadTooLarge,
    BodyTooLarge,
}

impl IngressError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::InputTooLarge => "ingress_input_too_large",
            Self::InvalidJson => "ingress_invalid_json",
            Self::UnsupportedVersion => "ingress_unsupported_version",
            Self::UnsupportedKind => "ingress_unsupported_kind",
            Self::InvalidRequestId => "ingress_invalid_request_id",
            Self::InvalidAgentId => "ingress_invalid_agent_id",
            Self::InvalidPayload => "ingress_invalid_payload",
            Self::PayloadTooLarge => "ingress_payload_too_large",
            Self::BodyTooLarge => "ingress_body_too_large",
        }
    }

    pub const fn message(&self) -> &'static str {
        match self {
            Self::InputTooLarge => "入口事件超过总大小限制",
            Self::InvalidJson => "入口事件不是有效的 JSON 对象",
            Self::UnsupportedVersion => "入口协议版本不受支持",
            Self::UnsupportedKind => "入口只接受 agent.event",
            Self::InvalidRequestId => "入口事件缺少有效的 requestId",
            Self::InvalidAgentId => "入口事件缺少有效的 agentId",
            Self::InvalidPayload => "入口事件的 payload 必须是对象",
            Self::PayloadTooLarge => "入口事件 payload 超过大小限制",
            Self::BodyTooLarge => "入口事件正文超过大小限制",
        }
    }
}

impl Display for IngressError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.message())
    }
}

impl std::error::Error for IngressError {}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WireEvent {
    protocol_version: u8,
    kind: String,
    request_id: String,
    agent_id: String,
    payload: Value,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PersistedEvent<'a> {
    protocol_version: u8,
    kind: &'static str,
    request_id: String,
    agent_id: String,
    payload: &'a Value,
}

pub struct IngressEvent;

impl IngressEvent {
    pub fn parse(input: &[u8]) -> Result<AgentEventEnvelope, IngressError> {
        if input.len() > MAX_PROTOCOL_BYTES {
            return Err(IngressError::InputTooLarge);
        }
        let wire: WireEvent =
            serde_json::from_slice(input).map_err(|_| IngressError::InvalidJson)?;
        if wire.protocol_version != PROTOCOL_VERSION {
            return Err(IngressError::UnsupportedVersion);
        }
        if wire.kind != EVENT_KIND {
            return Err(IngressError::UnsupportedKind);
        }

        let request_id = Uuid::parse_str(wire.request_id.trim())
            .map(|value| value.to_string())
            .map_err(|_| IngressError::InvalidRequestId)
            .and_then(|value| RequestId::new(value).map_err(|_| IngressError::InvalidRequestId))?;
        let agent_id = AgentId::new(wire.agent_id).map_err(|_| IngressError::InvalidAgentId)?;
        let payload = wire.payload;
        if !payload.is_object() {
            return Err(IngressError::InvalidPayload);
        }
        let payload_bytes =
            serde_json::to_vec(&payload).map_err(|_| IngressError::InvalidPayload)?;
        if payload_bytes.len() > MAX_PAYLOAD_BYTES {
            return Err(IngressError::PayloadTooLarge);
        }
        if payload
            .get("body")
            .and_then(Value::as_str)
            .is_some_and(|body| body.len() > MAX_BODY_BYTES)
        {
            return Err(IngressError::BodyTooLarge);
        }

        Ok(AgentEventEnvelope {
            request_id,
            agent_id,
            payload,
        })
    }
}

pub(crate) fn encode_envelope(envelope: &AgentEventEnvelope) -> Result<Vec<u8>, serde_json::Error> {
    serde_json::to_vec(&PersistedEvent {
        protocol_version: PROTOCOL_VERSION,
        kind: EVENT_KIND,
        request_id: envelope.request_id.to_string(),
        agent_id: envelope.agent_id.to_string(),
        payload: &envelope.payload,
    })
}
