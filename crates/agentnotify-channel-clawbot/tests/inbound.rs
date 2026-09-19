use agentnotify_channel_clawbot::{ClawBotCredentials, ClawBotInboundMessage, normalize_inbound};
use agentnotify_domain::Timestamp;
use serde_json::json;

const CURSOR: &str = "cursor-1";
const EXPECTED_FALLBACK_KEY: &str = "account:af912c0d054558c4b289982eeb1ae39a5dc3495d7bad3776a801744b0dacaf60:fallback:\
     da378b526d4318c535c6e0ca85f6eb972ce5fe9affc240f003739fa032c33ba4";

#[test]
fn conflicting_reference_ids_are_rejected() {
    let value = text_message("user-1", 1, None, "继续");
    let message: ClawBotInboundMessage = serde_json::from_value(json!({
        "from_user_id": value.from_user_id,
        "to_user_id": value.to_user_id,
        "message_type": value.message_type,
        "item_list": [{
            "type": 1,
            "text_item": { "text": "继续" },
            "ref_msg": {
                "message_item": { "msg_id": 22 }
            }
        }],
        "referenced_msg_id": "11"
    }))
    .expect("测试消息必须是合法 JSON");
    let account_id = account_id("bot-1", "user-1");

    let error = normalize_inbound(
        &account_id,
        &credentials("bot-1", "user-1"),
        message,
        Some(CURSOR),
        Timestamp::now_utc(),
    )
    .unwrap_err();

    assert_eq!(error.code(), "clawbot_reference_conflict");
}

#[test]
fn duplicate_reference_sources_are_deduplicated() {
    let message: ClawBotInboundMessage = serde_json::from_value(json!({
        "msg_id": "message-1",
        "from_user_id": "user-1",
        "to_user_id": "bot-1",
        "message_type": 1,
        "item_list": [{
            "type": 1,
            "text_item": { "text": "继续" },
            "ref_msg": {
                "msg_id": "platform-1",
                "referenced_msg_id": "platform-1",
                "message_item": { "msg_id": "platform-1" }
            }
        }],
        "referenced_msg_id": "platform-1"
    }))
    .expect("测试消息必须是合法 JSON");

    let inbound = normalize_inbound(
        &account_id("bot-1", "user-1"),
        &credentials("bot-1", "user-1"),
        message,
        Some(CURSOR),
        Timestamp::now_utc(),
    )
    .expect("相同引用 ID 不应冲突");

    assert_eq!(
        inbound
            .referenced_message_ids
            .iter()
            .map(|value| value.as_str())
            .collect::<Vec<_>>(),
        vec!["platform-1"]
    );
}

#[test]
fn string_and_numeric_ids_are_normalized() {
    let numeric: ClawBotInboundMessage = serde_json::from_value(json!({
        "msg_id": 123,
        "from_user_id": "user-1",
        "to_user_id": "bot-1",
        "message_type": 1,
        "item_list": [{
            "type": 1,
            "text_item": { "text": "数字" },
            "ref_msg": { "msg_id": 456 }
        }]
    }))
    .expect("数字 ID 必须兼容");
    let string: ClawBotInboundMessage = serde_json::from_value(json!({
        "message_id": "789",
        "from_user_id": "user-1",
        "to_user_id": "bot-1",
        "message_type": 1,
        "item_list": [{
            "type": 1,
            "text_item": { "text": "字符串" },
            "ref_msg": { "referenced_msg_id": "platform-789" }
        }]
    }))
    .expect("字符串 ID 必须兼容");

    let numeric = normalize_inbound(
        &account_id("bot-1", "user-1"),
        &credentials("bot-1", "user-1"),
        numeric,
        Some(CURSOR),
        Timestamp::now_utc(),
    )
    .expect("数字消息 ID 应可归一化");
    let string = normalize_inbound(
        &account_id("bot-1", "user-1"),
        &credentials("bot-1", "user-1"),
        string,
        Some(CURSOR),
        Timestamp::now_utc(),
    )
    .expect("字符串消息 ID 应可归一化");

    assert_eq!(
        numeric.external_message_id.as_ref().unwrap().as_str(),
        "123"
    );
    assert_eq!(
        numeric.referenced_message_ids.first().unwrap().as_str(),
        "456"
    );
    assert_eq!(string.external_message_id.as_ref().unwrap().as_str(), "789");
    assert_eq!(
        string.referenced_message_ids.first().unwrap().as_str(),
        "platform-789"
    );
}

#[test]
fn all_supported_reference_sources_are_read() {
    let cases = [
        json!({
            "msg_id": "message-0",
            "from_user_id": "user-1",
            "to_user_id": "bot-1",
            "message_type": 1,
            "item_list": [{"type": 1, "text_item": {"text": "继续"}}],
            "referenced_msg_id": "top"
        }),
        json!({
            "msg_id": "message-1",
            "from_user_id": "user-1",
            "to_user_id": "bot-1",
            "message_type": 1,
            "item_list": [{
                "type": 1,
                "text_item": {"text": "继续"},
                "ref_msg": {"msg_id": "ref-msg"}
            }]
        }),
        json!({
            "msg_id": "message-2",
            "from_user_id": "user-1",
            "to_user_id": "bot-1",
            "message_type": 1,
            "item_list": [{
                "type": 1,
                "text_item": {"text": "继续"},
                "ref_msg": {"referenced_msg_id": "ref-id"}
            }]
        }),
        json!({
            "msg_id": "message-3",
            "from_user_id": "user-1",
            "to_user_id": "bot-1",
            "message_type": 1,
            "item_list": [{
                "type": 1,
                "text_item": {"text": "继续"},
                "ref_msg": {"message_item": {"msg_id": "nested"}}
            }]
        }),
    ];
    let expected = ["top", "ref-msg", "ref-id", "nested"];

    for (index, value) in cases.into_iter().enumerate() {
        let message: ClawBotInboundMessage =
            serde_json::from_value(value).expect("引用结构必须兼容");

        let inbound = normalize_inbound(
            &account_id("bot-1", "user-1"),
            &credentials("bot-1", "user-1"),
            message,
            Some(CURSOR),
            Timestamp::now_utc(),
        )
        .expect("单一引用 ID 应可归一化");

        assert_eq!(
            inbound.referenced_message_ids.first().unwrap().as_str(),
            expected[index]
        );
    }
}

#[test]
fn only_bound_private_messages_are_accepted() {
    let credentials = credentials("bot-1", "user-1");
    let account_id = account_id("bot-1", "user-1");

    let group_message = text_message("user-1", 1, Some("group-1"), "群聊");
    let stranger_message = text_message("stranger", 1, None, "陌生人");
    let bot_message = text_message("user-1", 2, None, "非用户消息");

    for message in [group_message, stranger_message, bot_message] {
        let error = normalize_inbound(
            &account_id,
            &credentials,
            message,
            Some(CURSOR),
            Timestamp::now_utc(),
        )
        .unwrap_err();
        assert_eq!(error.code(), "clawbot_inbound_sender_mismatch");
    }
}

#[test]
fn same_message_id_in_different_accounts_stays_isolated() {
    let first = normalize_inbound(
        &account_id("bot-1", "user-1"),
        &credentials("bot-1", "user-1"),
        text_message("user-1", 1, None, "继续"),
        Some(CURSOR),
        Timestamp::now_utc(),
    )
    .unwrap();
    let second = normalize_inbound(
        &account_id("bot-2", "user-1"),
        &credentials("bot-2", "user-1"),
        text_message("user-1", 1, None, "继续"),
        Some(CURSOR),
        Timestamp::now_utc(),
    )
    .unwrap();

    assert_ne!(first.account_id, second.account_id);
    assert_ne!(first.claim_key().unwrap(), second.claim_key().unwrap());
}

#[test]
fn fallback_claim_key_matches_go_algorithm() {
    let mut message = text_message("user-1", 1, None, "继续");
    message.referenced_msg_id = Some("platform-1".into());
    let inbound = normalize_inbound(
        &account_id("bot-1", "user-1"),
        &credentials("bot-1", "user-1"),
        message,
        Some(""),
        Timestamp::now_utc(),
    )
    .unwrap();

    assert_eq!(
        inbound.claim_key().unwrap().as_str(),
        EXPECTED_FALLBACK_KEY.trim()
    );
}

fn credentials(bot_id: &str, user_id: &str) -> ClawBotCredentials {
    ClawBotCredentials::new(
        "bot-token",
        bot_id,
        user_id,
        "https://business.example.test",
    )
    .unwrap()
}

fn account_id(bot_id: &str, user_id: &str) -> agentnotify_domain::ChannelAccountId {
    agentnotify_channel_clawbot::stable_account_id(bot_id, user_id).unwrap()
}

fn text_message(
    from_user_id: &str,
    message_type: i32,
    group_id: Option<&str>,
    text: &str,
) -> ClawBotInboundMessage {
    serde_json::from_value(json!({
        "seq": 42,
        "from_user_id": from_user_id,
        "to_user_id": "bot-1",
        "message_type": message_type,
        "group_id": group_id,
        "item_list": [{
            "type": 1,
            "text_item": { "text": text }
        }]
    }))
    .expect("测试消息必须是合法 JSON")
}
