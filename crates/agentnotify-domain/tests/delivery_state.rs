use agentnotify_domain::{
    AgentId, ChannelAccountId, ChannelId, Delivery, DeliveryErrorKind, DeliveryId, DeliveryState,
    DomainError, ExternalMessageId, Notification, NotificationId, NotificationMetadata, SafeError,
    Timestamp,
};

fn pending_delivery(id: &str) -> Delivery {
    Delivery::pending(
        DeliveryId::new(id).unwrap(),
        NotificationId::new("notification-1").unwrap(),
        ChannelId::new("clawbot").unwrap(),
        ChannelAccountId::new("account-1").unwrap(),
    )
}

#[test]
fn pending_delivery_can_retry_only_after_retryable_failure() {
    let mut delivery = pending_delivery("delivery-1");
    delivery
        .mark_retryable(SafeError::new("channel_timeout", "渠道请求超时，稍后会重试").unwrap())
        .unwrap();
    assert_eq!(delivery.state(), DeliveryState::Failed);
    assert_eq!(delivery.error_kind(), Some(DeliveryErrorKind::Retryable));
    assert!(delivery.can_retry());
}

#[test]
fn unknown_and_sent_deliveries_never_retry() {
    let mut sent = pending_delivery("delivery-1");
    sent.mark_sent(ExternalMessageId::new("message-1").unwrap())
        .unwrap();
    assert!(!sent.can_retry());

    let mut unknown = pending_delivery("delivery-2");
    unknown
        .mark_unknown(SafeError::new("channel_unknown", "无法确认渠道是否已接收").unwrap())
        .unwrap();
    assert_eq!(unknown.state(), DeliveryState::Unknown);
    assert!(!unknown.can_retry());
}

#[test]
fn permanent_failure_cannot_retry() {
    let mut delivery = pending_delivery("delivery-3");
    delivery
        .mark_permanent_failure(
            SafeError::new("invalid_recipient", "渠道账号已失效，请重新登录").unwrap(),
        )
        .unwrap();
    assert_eq!(delivery.error_kind(), Some(DeliveryErrorKind::Permanent));
    assert!(!delivery.can_retry());
}

#[test]
fn terminal_delivery_rejects_later_transition() {
    let mut delivery = pending_delivery("delivery-4");
    delivery
        .mark_unknown(SafeError::new("channel_unknown", "无法确认渠道是否已接收").unwrap())
        .unwrap();

    let result = delivery.mark_sent(ExternalMessageId::new("message-2").unwrap());
    assert_eq!(result, Err(DomainError::InvalidStateTransition));
}

#[test]
fn notification_rejects_blank_fields_and_unsafe_metadata() {
    let blank = Notification::new(
        NotificationId::new("notification-1").unwrap(),
        "event-1",
        AgentId::new("opencode").unwrap(),
        None,
        None,
        " ",
        "正文",
        Timestamp::now_utc(),
        NotificationMetadata::default(),
    );
    assert!(matches!(blank, Err(DomainError::InvalidValue { .. })));

    let metadata =
        NotificationMetadata::new([("authorization".to_owned(), "must-not-be-stored".to_owned())]);
    assert!(matches!(metadata, Err(DomainError::InvalidMetadata)));
}
