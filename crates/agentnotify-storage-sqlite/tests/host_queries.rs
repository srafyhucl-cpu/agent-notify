use agentnotify_application::{DeliveryStore, IngestStore, OutboxItem, RouteStore};
use agentnotify_domain::{
    AgentId, AgentSessionId, ChannelAccountId, ChannelId, Delivery, DeliveryId, DeliveryState,
    ExternalMessageId, Notification, NotificationId, NotificationMetadata, ReplyRoute, RouteKey,
    SafeError, Timestamp,
};
use agentnotify_storage_sqlite::{NotificationQuery, SqliteStore};

fn timestamp(value: &str) -> Timestamp {
    Timestamp::parse_rfc3339(value).unwrap()
}

fn fixture_store() -> (tempfile::TempDir, SqliteStore) {
    let temp = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(temp.path().join("state.db")).unwrap();
    (temp, store)
}

async fn commit_notification(
    store: &SqliteStore,
    id: &str,
    agent_id: &str,
    title: &str,
    body: &str,
    occurred_at: &str,
) -> Notification {
    let notification = Notification::new(
        NotificationId::new(id).unwrap(),
        format!("event-{id}"),
        AgentId::new(agent_id).unwrap(),
        Some(AgentSessionId::new(format!("session-{id}")).unwrap()),
        Some(format!("会话 {id}")),
        title,
        body,
        timestamp(occurred_at),
        NotificationMetadata::default(),
    )
    .unwrap();
    store
        .commit_ingest(
            notification.clone(),
            vec![OutboxItem::pending(
                format!("outbox-{id}"),
                notification.id.clone(),
                timestamp("2026-09-19T09:00:00Z"),
            )],
        )
        .await
        .unwrap();
    notification
}

async fn commit_sent_delivery(
    store: &SqliteStore,
    notification: &Notification,
    channel_id: &str,
    account_id: &str,
    external_message_id: &str,
) {
    let lease = store
        .lease_next_outbox(
            timestamp("2026-09-19T10:00:00Z"),
            timestamp("2026-09-19T11:00:00Z"),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(lease.notification.id, notification.id);

    let mut delivery = Delivery::pending(
        DeliveryId::new(format!("delivery-{}", notification.id)).unwrap(),
        notification.id.clone(),
        ChannelId::new(channel_id).unwrap(),
        ChannelAccountId::new(account_id).unwrap(),
    );
    delivery
        .mark_sent(ExternalMessageId::new(external_message_id).unwrap())
        .unwrap();
    store.commit_delivery(lease, delivery, None).await.unwrap();
}

#[tokio::test]
async fn notification_history_filters_and_cursor_are_stable() {
    let (_temp, store) = fixture_store();
    let first = commit_notification(
        &store,
        "notification-1",
        "opencode",
        "第一条",
        "普通内容",
        "2026-09-19T09:00:00Z",
    )
    .await;
    commit_sent_delivery(&store, &first, "clawbot", "account-1", "external-1").await;
    commit_notification(
        &store,
        "notification-2",
        "codex",
        "第二条",
        "只在 Codex 出现",
        "2026-09-19T09:01:00Z",
    )
    .await;
    commit_notification(
        &store,
        "notification-3",
        "opencode",
        "第三条",
        "只匹配正文关键字",
        "2026-09-19T09:02:00Z",
    )
    .await;

    let page = store
        .notification_page(NotificationQuery {
            agent_id: Some("opencode".into()),
            limit: 1,
            ..NotificationQuery::default()
        })
        .await
        .unwrap();
    assert_eq!(page.total, 2);
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].notification.id.as_str(), "notification-3");
    assert_eq!(page.next_cursor.as_deref(), Some("notification-3"));

    let next = store
        .notification_page(NotificationQuery {
            agent_id: Some("opencode".into()),
            cursor: page.next_cursor,
            limit: 1,
            ..NotificationQuery::default()
        })
        .await
        .unwrap();
    assert_eq!(next.items.len(), 1);
    assert_eq!(next.items[0].notification.id.as_str(), "notification-1");
    assert!(next.next_cursor.is_none());

    let searched = store
        .notification_page(NotificationQuery {
            search: Some("关键字".into()),
            limit: 20,
            ..NotificationQuery::default()
        })
        .await
        .unwrap();
    assert_eq!(searched.total, 1);
    assert_eq!(searched.items[0].notification.id.as_str(), "notification-3");

    let delivery_filtered = store
        .notification_page(NotificationQuery {
            channel_id: Some("clawbot".into()),
            account_id: Some("account-1".into()),
            delivery_state: Some(DeliveryState::Sent),
            limit: 20,
            ..NotificationQuery::default()
        })
        .await
        .unwrap();
    assert_eq!(delivery_filtered.total, 1);
    assert_eq!(
        delivery_filtered.items[0].notification.id.as_str(),
        "notification-1"
    );
}

#[tokio::test]
async fn notification_detail_exposes_only_persisted_delivery_and_route_state() {
    let (_temp, store) = fixture_store();
    let notification = commit_notification(
        &store,
        "notification-detail",
        "opencode",
        "详情",
        "详情正文",
        "2026-09-19T09:00:00Z",
    )
    .await;
    commit_sent_delivery(
        &store,
        &notification,
        "clawbot",
        "account-detail",
        "external-detail",
    )
    .await;
    store
        .insert_route(ReplyRoute::new(
            RouteKey::new(
                ChannelId::new("clawbot").unwrap(),
                ChannelAccountId::new("account-detail").unwrap(),
                ExternalMessageId::new("external-detail").unwrap(),
            ),
            AgentId::new("opencode").unwrap(),
            AgentSessionId::new("session-notification-detail").unwrap(),
            timestamp("2026-09-19T09:00:00Z"),
            timestamp("2030-09-19T09:00:00Z"),
        ))
        .await
        .unwrap();

    let detail = store
        .notification_detail(notification.id.clone())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(detail.notification, notification);
    assert!(detail.route_exists);
    assert_eq!(detail.deliveries.len(), 1);
    assert_eq!(detail.deliveries[0].delivery.state(), DeliveryState::Sent);
}

#[tokio::test]
async fn failed_retryable_delivery_can_be_requeued_but_unknown_cannot() {
    let (_temp, store) = fixture_store();
    let notification = commit_notification(
        &store,
        "notification-retry",
        "opencode",
        "重试",
        "重试正文",
        "2026-09-19T09:00:00Z",
    )
    .await;
    let lease = store
        .lease_next_outbox(
            timestamp("2026-09-19T10:00:00Z"),
            timestamp("2026-09-19T11:00:00Z"),
        )
        .await
        .unwrap()
        .unwrap();
    let mut delivery = Delivery::pending(
        DeliveryId::new("delivery-retry").unwrap(),
        notification.id.clone(),
        ChannelId::new("clawbot").unwrap(),
        ChannelAccountId::new("account-retry").unwrap(),
    );
    delivery
        .mark_retryable(SafeError::new("temporary_network", "网络暂时不可用").unwrap())
        .unwrap();
    store
        .commit_delivery(lease, delivery.clone(), None)
        .await
        .unwrap();

    let requeued = store.requeue_delivery(delivery.id().clone()).await.unwrap();
    assert_eq!(requeued.delivery.state(), DeliveryState::Failed);
    let now = Timestamp::now_utc();
    assert!(
        store
            .lease_next_outbox(now, timestamp("2030-01-01T00:00:00Z"))
            .await
            .unwrap()
            .is_some()
    );

    let second = commit_notification(
        &store,
        "notification-unknown",
        "opencode",
        "未知",
        "未知正文",
        "2026-09-19T09:03:00Z",
    )
    .await;
    let lease = store
        .lease_next_outbox(
            timestamp("2026-09-19T10:10:00Z"),
            timestamp("2026-09-19T10:11:00Z"),
        )
        .await
        .unwrap()
        .unwrap();
    let mut unknown = Delivery::pending(
        DeliveryId::new("delivery-unknown").unwrap(),
        second.id.clone(),
        ChannelId::new("clawbot").unwrap(),
        ChannelAccountId::new("account-unknown").unwrap(),
    );
    unknown
        .mark_unknown(SafeError::new("send_unknown", "发送结果未知").unwrap())
        .unwrap();
    store
        .commit_delivery(lease, unknown.clone(), None)
        .await
        .unwrap();

    let error = store
        .requeue_delivery(unknown.id().clone())
        .await
        .unwrap_err();
    assert_eq!(error.code(), "delivery_not_retryable");
}

#[tokio::test]
async fn agent_configs_and_settings_round_trip_as_json() {
    let (_temp, store) = fixture_store();
    let config = serde_json::json!({
        "replyWindowSeconds": 300,
        "nested": { "enabled": true }
    });
    let saved = store
        .upsert_agent_config("opencode", false, &config)
        .await
        .unwrap();
    assert!(!saved.enabled);
    assert_eq!(saved.config, config);

    let configs = store.agent_configs().await.unwrap();
    assert_eq!(configs["opencode"].config, config);
    assert!(!configs["opencode"].enabled);

    let entries = [
        ("autoStart", serde_json::json!(true)),
        ("startHidden", serde_json::json!(false)),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_owned(), value))
    .collect();
    store.write_settings_entries(entries).await.unwrap();

    let settings = store.settings_entries().await.unwrap();
    assert_eq!(settings["autoStart"], serde_json::json!(true));
    assert_eq!(settings["startHidden"], serde_json::json!(false));
}
