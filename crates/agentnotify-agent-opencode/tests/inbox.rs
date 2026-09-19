use agentnotify_agent_opencode::{OpenCodeInboxState, OpenCodeReplyInbox};

#[tokio::test]
async fn missing_inbox_does_not_create_directories() {
    let temp = tempfile::tempdir().unwrap();
    let inbox = OpenCodeReplyInbox::new(temp.path().join("missing"));

    assert_eq!(inbox.inspect_state().await, OpenCodeInboxState::NotFound);
    assert!(!inbox.root().exists());
}

#[tokio::test]
async fn fresh_ready_heartbeat_reports_ready() {
    let temp = tempfile::tempdir().unwrap();
    let inbox = OpenCodeReplyInbox::new(temp.path());
    let heartbeats = temp.path().join("heartbeats");
    tokio::fs::create_dir_all(&heartbeats).await.unwrap();
    tokio::fs::write(
        heartbeats.join("plugin.json"),
        serde_json::json!({
            "ready": true,
            "timestamp": time::OffsetDateTime::now_utc()
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap()
        })
        .to_string(),
    )
    .await
    .unwrap();

    assert_eq!(inbox.inspect_state().await, OpenCodeInboxState::Ready);
}

#[tokio::test]
async fn stale_or_incapable_heartbeat_reports_waiting() {
    let temp = tempfile::tempdir().unwrap();
    let inbox = OpenCodeReplyInbox::new(temp.path());
    let heartbeats = temp.path().join("heartbeats");
    tokio::fs::create_dir_all(&heartbeats).await.unwrap();

    for (name, ready, age) in [
        ("stale.json", true, time::Duration::minutes(1)),
        ("incapable.json", false, time::Duration::ZERO),
    ] {
        let timestamp = (time::OffsetDateTime::now_utc() - age)
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap();
        tokio::fs::write(
            heartbeats.join(name),
            serde_json::json!({"ready": ready, "timestamp": timestamp}).to_string(),
        )
        .await
        .unwrap();
    }

    assert_eq!(inbox.inspect_state().await, OpenCodeInboxState::Waiting);
}
