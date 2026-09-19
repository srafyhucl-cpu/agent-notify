use agentnotify_domain::{
    AgentId, AgentSessionId, ChannelAccountId, ChannelId, ClaimKey, DomainError, ExternalMessageId,
    InboundMessage, ReplyRoute, RouteKey, Timestamp,
};

#[test]
fn same_external_message_id_in_different_accounts_is_not_equal() {
    let first = RouteKey::new(
        ChannelId::new("clawbot").unwrap(),
        ChannelAccountId::new("account-a").unwrap(),
        ExternalMessageId::new("message-1").unwrap(),
    );
    let second = RouteKey::new(
        ChannelId::new("clawbot").unwrap(),
        ChannelAccountId::new("account-b").unwrap(),
        ExternalMessageId::new("message-1").unwrap(),
    );
    assert_ne!(first, second);
}

#[test]
fn fallback_claim_key_is_deterministic_without_message_id() {
    let message = InboundMessage::without_external_id(
        ChannelId::new("clawbot").unwrap(),
        ChannelAccountId::new("account-a").unwrap(),
        "cursor-7",
        vec![ExternalMessageId::new("quoted-1").unwrap()],
        "继续检查",
        Timestamp::parse_rfc3339("2026-09-19T10:20:30.123+08:00").unwrap(),
    )
    .unwrap();
    let first = ClaimKey::from_inbound(&message).unwrap();
    let second = ClaimKey::from_inbound(&message).unwrap();
    assert_eq!(first, second);
}

#[test]
fn expired_route_is_rejected() {
    let route = ReplyRoute::new(
        RouteKey::new(
            ChannelId::new("clawbot").unwrap(),
            ChannelAccountId::new("account-a").unwrap(),
            ExternalMessageId::new("message-1").unwrap(),
        ),
        AgentId::new("opencode").unwrap(),
        AgentSessionId::new("session-1").unwrap(),
        Timestamp::parse_rfc3339("2026-09-19T10:00:00Z").unwrap(),
        Timestamp::parse_rfc3339("2026-09-19T10:30:00Z").unwrap(),
    );
    let now = Timestamp::parse_rfc3339("2026-09-19T10:30:00Z").unwrap();
    assert_eq!(route.is_active(now), Err(DomainError::RouteExpired));
}
