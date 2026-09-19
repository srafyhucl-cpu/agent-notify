use agentnotify_application::{ClaimStore, Clock, IngestStore, OutboxItem, StoreError};
use agentnotify_domain::{
    AgentId, ChannelAccountId, ChannelId, ClaimKey, ClaimOutcome, InboundClaim, InboundMessage,
    Notification, NotificationId, NotificationMetadata, Timestamp,
};
use agentnotify_testkit::{FakeClock, MemoryStore};

fn fixture_notification() -> Notification {
    Notification::new(
        NotificationId::new("notification-1").unwrap(),
        "event-1",
        AgentId::new("opencode").unwrap(),
        None,
        None,
        "任务完成",
        "构建成功",
        Timestamp::parse_rfc3339("2026-09-19T10:20:30Z").unwrap(),
        NotificationMetadata::default(),
    )
    .unwrap()
}

fn fixture_outbox(notification: &Notification) -> OutboxItem {
    OutboxItem::pending(
        "outbox-1",
        notification.id.clone(),
        notification.occurred_at,
    )
}

fn fixture_claim() -> InboundClaim {
    let received_at = Timestamp::parse_rfc3339("2026-09-19T10:20:30Z").unwrap();
    let message = InboundMessage::without_external_id(
        ChannelId::new("clawbot").unwrap(),
        ChannelAccountId::new("account-1").unwrap(),
        "cursor-1",
        Vec::new(),
        "继续处理",
        received_at,
    )
    .unwrap();
    let key = ClaimKey::from_inbound(&message).unwrap();
    InboundClaim::new(
        key,
        message.channel_id.clone(),
        message.account_id.clone(),
        message.external_message_id.clone(),
        received_at,
        received_at
            .checked_add(time::Duration::minutes(30))
            .unwrap(),
    )
    .unwrap()
}

#[tokio::test]
async fn ingest_store_commits_notification_and_outbox_together() {
    let store = MemoryStore::default();
    let notification = fixture_notification();
    let outbox = fixture_outbox(&notification);
    store
        .commit_ingest(notification.clone(), vec![outbox.clone()])
        .await
        .unwrap();

    assert_eq!(store.notification_count().await, 1);
    assert_eq!(store.outbox_count().await, 1);
    assert_eq!(
        store.notification(notification.id.clone()).await.unwrap(),
        notification
    );
}

#[tokio::test]
async fn claim_store_never_returns_acquired_twice() {
    let store = MemoryStore::default();
    let claim = fixture_claim();
    assert!(matches!(
        store.claim(claim.clone()).await.unwrap(),
        ClaimOutcome::Acquired(_)
    ));
    assert!(matches!(
        store.claim(claim).await.unwrap(),
        ClaimOutcome::AlreadyClaimed { .. }
    ));
}

#[tokio::test]
async fn fake_clock_is_deterministic() {
    let start = Timestamp::parse_rfc3339("2026-09-19T10:20:30Z").unwrap();
    let clock = FakeClock::new(start);
    assert_eq!(clock.now(), start);
    clock.advance(time::Duration::seconds(5));
    assert_eq!(
        clock.now(),
        start.checked_add(time::Duration::seconds(5)).unwrap()
    );
    assert!(matches!(
        store_error("memory_conflict").code(),
        "memory_conflict"
    ));
}

fn store_error(code: &str) -> StoreError {
    StoreError::conflict(code, "内存测试冲突")
}
