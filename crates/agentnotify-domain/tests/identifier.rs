use agentnotify_domain::{AgentId, ChannelAccountId, DomainError, Timestamp};

#[test]
fn string_ids_reject_empty_and_whitespace() {
    assert!(matches!(
        AgentId::new(""),
        Err(DomainError::InvalidIdentifier { .. })
    ));
    assert!(matches!(
        ChannelAccountId::new("  "),
        Err(DomainError::InvalidIdentifier { .. })
    ));
}

#[test]
fn ids_are_not_interchangeable() {
    let agent = AgentId::new("opencode").unwrap();
    assert_eq!(agent.as_str(), "opencode");
    assert_eq!(agent.to_string(), "opencode");
}

#[test]
fn timestamp_round_trips_rfc3339_millis() {
    let value = Timestamp::parse_rfc3339("2026-09-19T10:20:30.123+08:00").unwrap();
    assert_eq!(value.to_rfc3339(), "2026-09-19T10:20:30.123+08:00");
}
