use agentnotify_channel_clawbot::parse_message_id;
use agentnotify_channel_sdk::ChannelError;

#[test]
fn message_id_is_read_from_top_level_nested_and_item_list() {
    assert_eq!(parse_message_id(br#"{"message_id":"m1"}"#).unwrap(), "m1");
    assert_eq!(parse_message_id(br#"{"data":{"msg_id":2}}"#).unwrap(), "2");
    assert_eq!(
        parse_message_id(br#"{"item_list":[{"msg_id":3}]}"#).unwrap(),
        "3"
    );
    assert_eq!(parse_message_id(br#"{"msg":5678}"#).unwrap(), "5678");
}

#[test]
fn duplicate_message_ids_from_different_locations_are_accepted() {
    let response = br#"{
        "message_id": 1001,
        "data": {
            "msgid": "1001",
            "item_list": [{"msg_id": 1001}]
        }
    }"#;

    assert_eq!(parse_message_id(response).unwrap(), "1001");
}

#[test]
fn conflicting_message_ids_are_unknown() {
    let error = parse_message_id(
        br#"{"message_id":"top","msg":{"msg_id":"nested"},"item_list":[{"msg_id":"item"}]}"#,
    )
    .unwrap_err();

    assert!(matches!(error, ChannelError::Unknown(_)));
}

#[test]
fn missing_message_id_is_unknown() {
    let error = parse_message_id(br#"{"ret":0,"client_id":"client-1"}"#).unwrap_err();

    assert!(matches!(error, ChannelError::Unknown(_)));
}

#[test]
fn malformed_message_id_is_unknown() {
    let error = parse_message_id(br#"{"message_id":{"unexpected":true}}"#).unwrap_err();

    assert!(matches!(error, ChannelError::Unknown(_)));
}
