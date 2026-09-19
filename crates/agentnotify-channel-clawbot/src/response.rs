use agentnotify_channel_sdk::ChannelError;
use serde_json::{Map, Value};

const MESSAGE_ID_FIELDS: [&str; 3] = ["message_id", "msg_id", "msgid"];
const NESTED_RESPONSE_FIELDS: [&str; 2] = ["msg", "data"];

#[derive(Debug, Default, Eq, PartialEq)]
pub(crate) struct ParsedResponseIds {
    pub message_id: Option<String>,
    pub client_id: Option<String>,
}

/// 从成功响应中归一化平台消息 ID；缺失或冲突都由调用方映射为 Unknown。
pub fn parse_message_id(response: &[u8]) -> Result<String, ChannelError> {
    parse_response_ids(response)?
        .message_id
        .ok_or_else(message_id_missing)
}

pub(crate) fn parse_response_ids(response: &[u8]) -> Result<ParsedResponseIds, ChannelError> {
    let value = serde_json::from_slice::<Value>(response).map_err(|_| {
        ChannelError::unknown(
            "clawbot_response_invalid",
            "ClawBot 返回了无法解析的发送结果",
        )
    })?;
    let object = value.as_object().ok_or_else(|| {
        ChannelError::unknown(
            "clawbot_response_invalid",
            "ClawBot 返回了无法识别的发送结果",
        )
    })?;

    let mut ids = Vec::new();
    collect_object_ids(object, &mut ids)?;
    let unique = unique_ids(ids);
    let message_id = match unique.len() {
        0 => None,
        1 => unique.into_iter().next(),
        _ => {
            return Err(ChannelError::unknown(
                "clawbot_message_id_ambiguous",
                "ClawBot 返回了多个不一致的消息 ID，无法确认投递结果",
            ));
        }
    };

    Ok(ParsedResponseIds {
        message_id,
        client_id: optional_candidate(object.get("client_id"), "client_id")?,
    })
}

fn collect_object_ids(
    object: &Map<String, Value>,
    ids: &mut Vec<String>,
) -> Result<(), ChannelError> {
    for field in MESSAGE_ID_FIELDS {
        if let Some(value) = object.get(field) {
            if let Some(candidate) = optional_candidate(Some(value), field)? {
                ids.push(candidate);
            }
        }
    }
    if let Some(items) = object.get("item_list") {
        collect_item_list_ids(items, ids)?;
    }
    for field in NESTED_RESPONSE_FIELDS {
        if let Some(value) = object.get(field) {
            collect_nested_ids(value, field, ids)?;
        }
    }
    Ok(())
}

fn collect_nested_ids(
    value: &Value,
    field: &str,
    ids: &mut Vec<String>,
) -> Result<(), ChannelError> {
    match value {
        Value::Null => Ok(()),
        Value::Object(object) => collect_object_ids(object, ids),
        Value::Array(items) => {
            for item in items {
                collect_item_ids(item, ids)?;
            }
            Ok(())
        }
        _ => {
            if let Some(candidate) = optional_candidate(Some(value), field)? {
                ids.push(candidate);
            }
            Ok(())
        }
    }
}

fn collect_item_list_ids(value: &Value, ids: &mut Vec<String>) -> Result<(), ChannelError> {
    let items = value.as_array().ok_or_else(|| {
        ChannelError::unknown(
            "clawbot_response_invalid",
            "ClawBot 返回的 item_list 格式无效",
        )
    })?;
    for item in items {
        collect_item_ids(item, ids)?;
    }
    Ok(())
}

fn collect_item_ids(value: &Value, ids: &mut Vec<String>) -> Result<(), ChannelError> {
    match value {
        Value::Object(object) => collect_object_ids(object, ids),
        Value::Array(items) => {
            for item in items {
                collect_item_ids(item, ids)?;
            }
            Ok(())
        }
        _ => Err(ChannelError::unknown(
            "clawbot_response_invalid",
            "ClawBot 返回的消息项格式无效",
        )),
    }
}

fn optional_candidate(value: Option<&Value>, field: &str) -> Result<Option<String>, ChannelError> {
    let Some(value) = value else {
        return Ok(None);
    };
    match value {
        Value::Null => Ok(None),
        Value::String(value) => {
            let value = value.trim();
            if value.is_empty() {
                Ok(None)
            } else {
                Ok(Some(value.into()))
            }
        }
        Value::Number(value) => {
            if let Some(value) = value.as_i64() {
                Ok(Some(value.to_string()))
            } else if let Some(value) = value.as_u64() {
                Ok(Some(value.to_string()))
            } else {
                Err(invalid_field(field))
            }
        }
        _ => Err(invalid_field(field)),
    }
}

fn unique_ids(ids: Vec<String>) -> Vec<String> {
    let mut unique = Vec::new();
    for id in ids {
        if !unique.contains(&id) {
            unique.push(id);
        }
    }
    unique
}

fn invalid_field(field: &str) -> ChannelError {
    ChannelError::unknown(
        "clawbot_message_id_invalid",
        format!("ClawBot 返回的 {field} 不是有效消息 ID"),
    )
}

fn message_id_missing() -> ChannelError {
    ChannelError::unknown(
        "clawbot_message_id_missing",
        "ClawBot 已接受请求，但未返回稳定消息 ID，无法确认投递结果",
    )
}
