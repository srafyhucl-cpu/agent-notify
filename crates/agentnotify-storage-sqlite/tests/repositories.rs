use std::sync::Arc;

use agentnotify_application::{
    ClaimStore, DeliveryStore, IngestStore, OutboxItem, OutboxState, RouteStore, StatusStore,
};
use agentnotify_domain::{
    AgentId, AgentSessionId, ChannelAccountId, ChannelId, ClaimOutcome, ClaimState, Delivery,
    DeliveryId, ExternalMessageId, InboundClaim, Notification, NotificationId,
    NotificationMetadata, ReplyRoute, RouteKey, SafeError, Timestamp,
};
use agentnotify_storage_sqlite::SqliteStore;

fn timestamp(value: &str) -> Timestamp {
    Timestamp::parse_rfc3339(value).unwrap()
}

fn fixture_store() -> (tempfile::TempDir, Arc<SqliteStore>) {
    let temp = tempfile::tempdir().unwrap();
    let store = Arc::new(SqliteStore::open(temp.path().join("state.db")).unwrap());
    (temp, store)
}

fn fixture_notification() -> Notification {
    Notification::new(
        NotificationId::new("notification-1").unwrap(),
        "event-1",
        AgentId::new("opencode").unwrap(),
        Some(AgentSessionId::new("session-1").unwrap()),
        Some("会话标题".into()),
        "任务完成",
        "Agent 已完成当前任务",
        timestamp("2026-09-19T09:00:00Z"),
        NotificationMetadata::default(),
    )
    .unwrap()
}

fn fixture_outbox(notification_id: NotificationId) -> OutboxItem {
    OutboxItem::pending(
        "outbox-1",
        notification_id,
        timestamp("2026-09-19T09:00:00Z"),
    )
}

async fn commit_fixture_notification(store: &SqliteStore) -> Notification {
    let notification = fixture_notification();
    store
        .commit_ingest(
            notification.clone(),
            vec![fixture_outbox(notification.id.clone())],
        )
        .await
        .unwrap();
    notification
}

fn fixture_delivery(lease: &agentnotify_application::OutboxLease) -> Delivery {
    let mut delivery = Delivery::pending(
        DeliveryId::new("delivery-1").unwrap(),
        lease.notification.id.clone(),
        ChannelId::new("clawbot").unwrap(),
        ChannelAccountId::new("account-1").unwrap(),
    );
    delivery
        .mark_sent(ExternalMessageId::new("wechat-message-1").unwrap())
        .unwrap();
    delivery
}

fn fixture_route() -> ReplyRoute {
    ReplyRoute::new(
        RouteKey::new(
            ChannelId::new("clawbot").unwrap(),
            ChannelAccountId::new("account-1").unwrap(),
            ExternalMessageId::new("wechat-message-1").unwrap(),
        ),
        AgentId::new("opencode").unwrap(),
        AgentSessionId::new("session-1").unwrap(),
        timestamp("2026-09-19T09:00:00Z"),
        timestamp("2026-09-20T09:00:00Z"),
    )
}

fn fixture_claim() -> InboundClaim {
    InboundClaim::new(
        agentnotify_domain::ClaimKey::new("claim-1").unwrap(),
        ChannelId::new("clawbot").unwrap(),
        ChannelAccountId::new("account-1").unwrap(),
        Some(ExternalMessageId::new("wechat-reply-1").unwrap()),
        timestamp("2026-09-19T09:01:00Z"),
        timestamp("2026-09-20T09:01:00Z"),
    )
    .unwrap()
}

#[tokio::test]
async fn ingest_round_trips_notification_and_outbox() {
    let (_temp, store) = fixture_store();
    let notification = commit_fixture_notification(&store).await;

    let stored = store
        .notification_by_ingest_key(&notification.agent_id, &notification.ingest_key)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored, notification);

    let lease = store
        .lease_next_outbox(
            timestamp("2026-09-19T09:00:01Z"),
            timestamp("2026-09-19T09:05:00Z"),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(lease.outbox.state, OutboxState::Leased);
    assert_eq!(lease.outbox.attempt_count, 1);
    assert_eq!(lease.notification, notification);
}

#[tokio::test]
async fn sent_delivery_and_route_commit_in_one_transaction() {
    let (_temp, store) = fixture_store();
    let notification = commit_fixture_notification(&store).await;
    let lease = store
        .lease_next_outbox(
            timestamp("2026-09-19T09:00:01Z"),
            timestamp("2026-09-19T09:05:00Z"),
        )
        .await
        .unwrap()
        .unwrap();
    let delivery = fixture_delivery(&lease);
    let route = fixture_route();

    store
        .commit_delivery(lease, delivery.clone(), Some(route.clone()))
        .await
        .unwrap();

    let stored = store
        .delivery(DeliveryId::new("delivery-1").unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        stored.delivery.state(),
        agentnotify_domain::DeliveryState::Sent
    );
    assert_eq!(
        stored.external_message_id,
        Some(ExternalMessageId::new("wechat-message-1").unwrap())
    );
    assert_eq!(
        store
            .find_route(&route.key, timestamp("2026-09-19T09:02:00Z"))
            .await
            .unwrap(),
        Some(route)
    );
    assert_eq!(notification.id, stored.delivery.notification_id().clone());
}

#[tokio::test]
async fn retryable_failure_reschedules_without_losing_attempt_count() {
    let (_temp, store) = fixture_store();
    commit_fixture_notification(&store).await;
    let lease = store
        .lease_next_outbox(
            timestamp("2026-09-19T09:00:01Z"),
            timestamp("2026-09-19T09:05:00Z"),
        )
        .await
        .unwrap()
        .unwrap();
    let mut delivery = Delivery::pending(
        DeliveryId::new("delivery-1").unwrap(),
        lease.notification.id.clone(),
        ChannelId::new("clawbot").unwrap(),
        ChannelAccountId::new("account-1").unwrap(),
    );
    delivery
        .mark_retryable(SafeError::new("network_timeout", "渠道暂时不可用").unwrap())
        .unwrap();
    let next_attempt = timestamp("2026-09-19T09:01:00Z");

    store
        .reschedule_outbox(lease, delivery, next_attempt)
        .await
        .unwrap();

    let stored = store
        .delivery(DeliveryId::new("delivery-1").unwrap())
        .await
        .unwrap()
        .unwrap();
    assert!(stored.delivery.can_retry());

    let next_lease = store
        .lease_next_outbox(
            timestamp("2026-09-19T09:01:01Z"),
            timestamp("2026-09-19T09:05:00Z"),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(next_lease.outbox.attempt_count, 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_claim_allows_exactly_one_owner() {
    let (_temp, store) = fixture_store();
    let claim = fixture_claim();
    let first = store.clone();
    let second = store.clone();

    let (left, right) = tokio::join!(first.claim(claim.clone()), second.claim(claim));
    let acquired = [left.unwrap(), right.unwrap()]
        .into_iter()
        .filter(|outcome| matches!(outcome, ClaimOutcome::Acquired(_)))
        .count();

    assert_eq!(acquired, 1);
    let stored = store
        .find_claim(&agentnotify_domain::ClaimKey::new("claim-1").unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.state, ClaimState::InProgress);
}

#[tokio::test]
async fn completed_claim_never_returns_acquired_again() {
    let (_temp, store) = fixture_store();
    let claim = fixture_claim();
    let acquired = store.claim(claim.clone()).await.unwrap();
    let ClaimOutcome::Acquired(mut claim) = acquired else {
        panic!("首次 Claim 应成功");
    };
    claim
        .mark_completed(timestamp("2026-09-19T09:02:00Z"))
        .unwrap();
    store.update_claim(claim).await.unwrap();

    let outcome = store.claim(fixture_claim()).await.unwrap();
    assert!(matches!(
        outcome,
        ClaimOutcome::AlreadyClaimed {
            state: ClaimState::Completed,
            ..
        }
    ));
}

#[tokio::test]
async fn route_lookup_requires_exact_account_and_unexpired_key() {
    let (_temp, store) = fixture_store();
    let route = fixture_route();
    store.insert_route(route.clone()).await.unwrap();

    assert_eq!(
        store
            .find_route(&route.key, timestamp("2026-09-19T09:02:00Z"))
            .await
            .unwrap(),
        Some(route.clone())
    );

    let wrong_account = RouteKey::new(
        ChannelId::new("clawbot").unwrap(),
        ChannelAccountId::new("account-2").unwrap(),
        ExternalMessageId::new("wechat-message-1").unwrap(),
    );
    assert!(
        store
            .find_route(&wrong_account, timestamp("2026-09-19T09:02:00Z"))
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .find_route(&route.key, route.expires_at)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn status_snapshot_counts_pending_outbox_and_recent_error() {
    let (_temp, store) = fixture_store();
    commit_fixture_notification(&store).await;

    let snapshot = store.snapshot().await.unwrap();
    assert_eq!(snapshot.notification_count, 1);
    assert_eq!(snapshot.delivery_count, 0);
    assert_eq!(snapshot.pending_outbox_count, 1);

    let error = SafeError::new("channel_offline", "渠道当前离线，请检查连接").unwrap();
    store.record_error(error.clone()).await.unwrap();
    assert_eq!(store.snapshot().await.unwrap().recent_error, Some(error));
}
