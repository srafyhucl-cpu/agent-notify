mod support;

use agentnotify_application::{
    ClaimStore, DeliveryStore, IngestResult, ProcessOutcome, ReplyOutcome, RouteStore, StatusStore,
};
use agentnotify_domain::{
    AgentId, ChannelAccountId, ChannelId, ClaimState, ExternalMessageId, InboundClaim, ReplyRoute,
    RouteKey,
};
use agentnotify_runtime::AppRuntime;
use rusqlite::Connection;
use support::{
    ACCOUNT_ID, AGENT_ID, CHANNEL_ID, CoreServices, SharedAdapters, agent_session, completed_event,
    initialize_store, quoted_message, runtime_config, timestamp,
};

#[tokio::test]
async fn pending_outbox_survives_restart_and_is_delivered_once() {
    let temp = tempfile::tempdir().unwrap();
    let database_path = temp.path().join("state.db");
    let shared = SharedAdapters::new();

    let store = initialize_store(&database_path, &shared).await;
    let core = CoreServices::new(store.clone(), &shared);
    core.ingest
        .ingest(completed_event(
            "75fe53aa-2314-4c21-b12e-773efce521d9",
            "event-1",
            "session-1",
        ))
        .await
        .unwrap();
    drop(core);
    drop(store);

    let reopened = initialize_store(&database_path, &shared).await;
    let restarted = CoreServices::new(reopened.clone(), &shared);
    assert!(matches!(
        restarted.delivery.process_next().await.unwrap(),
        ProcessOutcome::Completed { .. }
    ));
    assert!(matches!(
        restarted.delivery.process_next().await.unwrap(),
        ProcessOutcome::Idle
    ));
    assert!(shared.channel.last_receipt(ACCOUNT_ID).is_some());
    assert_eq!(reopened.snapshot().await.unwrap().delivery_count, 1);
}

#[tokio::test]
async fn unknown_delivery_is_not_reclaimed_after_restart() {
    let temp = tempfile::tempdir().unwrap();
    let database_path = temp.path().join("state.db");
    let shared = SharedAdapters::unknown_channel();

    let store = initialize_store(&database_path, &shared).await;
    let core = CoreServices::new(store.clone(), &shared);
    core.ingest
        .ingest(completed_event(
            "75fe53aa-2314-4c21-b12e-773efce521d9",
            "event-1",
            "session-1",
        ))
        .await
        .unwrap();
    assert!(matches!(
        core.delivery.process_next().await.unwrap(),
        ProcessOutcome::Completed { .. }
    ));
    drop(core);
    drop(store);

    let reopened = initialize_store(&database_path, &shared).await;
    let restarted = CoreServices::new(reopened.clone(), &shared);
    assert!(matches!(
        restarted.delivery.process_next().await.unwrap(),
        ProcessOutcome::Idle
    ));
    let snapshot = reopened.snapshot().await.unwrap();
    assert_eq!(snapshot.delivery_count, 1);
    assert_eq!(snapshot.pending_outbox_count, 0);
}

#[tokio::test]
async fn runtime_marks_interrupted_outbox_unknown_without_resending() {
    let temp = tempfile::tempdir().unwrap();
    let database_path = temp.path().join("state.db");
    let shared = SharedAdapters::new();

    let store = initialize_store(&database_path, &shared).await;
    let core = CoreServices::new(store.clone(), &shared);
    let result = core
        .ingest
        .ingest(completed_event(
            "75fe53aa-2314-4c21-b12e-773efce521d9",
            "event-1",
            "session-1",
        ))
        .await
        .unwrap();
    assert!(matches!(result, IngestResult::Queued { .. }));
    store
        .lease_next_outbox(
            timestamp("2026-09-19T09:00:01Z"),
            timestamp("2026-09-19T09:05:00Z"),
        )
        .await
        .unwrap()
        .unwrap();
    drop(core);
    drop(store);

    let mut runtime = AppRuntime::start(runtime_config(database_path, &shared))
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;

    assert!(shared.channel.last_receipt(ACCOUNT_ID).is_none());
    let snapshot = runtime.snapshot();
    assert_eq!(snapshot.overview.storage.delivery_count, 0);
    assert_eq!(snapshot.overview.storage.pending_outbox_count, 0);
    assert_eq!(
        snapshot
            .overview
            .storage
            .recent_error
            .as_ref()
            .unwrap()
            .code(),
        "runtime_recovered_interrupted_work"
    );
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn runtime_marks_interrupted_claim_unknown_and_never_resumes_again() {
    let temp = tempfile::tempdir().unwrap();
    let database_path = temp.path().join("state.db");
    let shared = SharedAdapters::new();

    let store = initialize_store(&database_path, &shared).await;
    let message = quoted_message("reply-1", "external-1", "继续处理");
    let route = ReplyRoute::new(
        RouteKey::new(
            ChannelId::new(CHANNEL_ID).unwrap(),
            ChannelAccountId::new(ACCOUNT_ID).unwrap(),
            ExternalMessageId::new("external-1").unwrap(),
        ),
        AgentId::new(AGENT_ID).unwrap(),
        agent_session("session-1"),
        timestamp("2026-09-19T09:00:00Z"),
        timestamp("2026-09-19T10:00:00Z"),
    );
    store.insert_route(route).await.unwrap();
    store
        .claim(InboundClaim::from_inbound(&message, timestamp("2026-09-19T10:00:02Z")).unwrap())
        .await
        .unwrap();
    drop(store);

    let mut runtime = AppRuntime::start(runtime_config(database_path, &shared))
        .await
        .unwrap();
    let core = CoreServices::new(runtime.store(), &shared);
    let outcome = core.reply.handle(message.clone()).await.unwrap();
    assert!(matches!(
        outcome,
        ReplyOutcome::AlreadyClaimed {
            state: ClaimState::Unknown,
            ..
        }
    ));
    assert_eq!(shared.agent.resume_count(), 0);

    let claim = runtime
        .store()
        .find_claim(&message.claim_key().unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(claim.state, ClaimState::Unknown);
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn migration_checksum_mismatch_fails_runtime_with_chinese_diagnostic() {
    let temp = tempfile::tempdir().unwrap();
    let database_path = temp.path().join("state.db");
    let shared = SharedAdapters::new();

    drop(initialize_store(&database_path, &shared).await);
    {
        let connection = Connection::open(&database_path).unwrap();
        connection
            .execute(
                "UPDATE schema_migrations SET checksum = 'tampered' WHERE version = 1",
                [],
            )
            .unwrap();
    }

    let result = AppRuntime::start(runtime_config(database_path, &shared)).await;
    let error = match result {
        Ok(_) => panic!("篡改迁移后 runtime 不应启动"),
        Err(error) => error,
    };
    assert_eq!(error.code(), "migration_checksum_mismatch");
    assert!(error.message().contains("数据库迁移校验失败"));
}
