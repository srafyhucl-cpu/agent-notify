mod support;

use agentnotify_application::{
    ClaimStore, IngestResult, ProcessOutcome, ReplyOutcome, StatusStore,
};
use agentnotify_domain::ClaimState;
use support::{ACCOUNT_ID, completed_event, initialize_store, quoted_message};

#[tokio::test]
async fn duplicate_ingress_event_creates_one_notification_and_delivery() {
    let temp = tempfile::tempdir().unwrap();
    let database_path = temp.path().join("state.db");
    let shared = support::SharedAdapters::new();
    let store = initialize_store(&database_path, &shared).await;
    let core = support::CoreServices::new(store.clone(), &shared);

    let first = core
        .ingest
        .ingest(completed_event(
            "75fe53aa-2314-4c21-b12e-773efce521d9",
            "same-event",
            "session-1",
        ))
        .await
        .unwrap();
    let second = core
        .ingest
        .ingest(completed_event(
            "d4d54a32-b7d4-4f5c-a130-a2f7ec64d560",
            "same-event",
            "session-1",
        ))
        .await
        .unwrap();

    let IngestResult::Queued { notification_id } = first else {
        panic!("首次事件应创建通知");
    };
    assert_eq!(
        second,
        IngestResult::Duplicate {
            notification_id: notification_id.clone()
        }
    );

    assert!(matches!(
        core.delivery.process_next().await.unwrap(),
        ProcessOutcome::Completed { .. }
    ));
    assert!(matches!(
        core.delivery.process_next().await.unwrap(),
        ProcessOutcome::Idle
    ));

    let snapshot = store.snapshot().await.unwrap();
    assert_eq!(snapshot.notification_count, 1);
    assert_eq!(snapshot.delivery_count, 1);
    assert_eq!(snapshot.pending_outbox_count, 0);
}

#[tokio::test]
async fn duplicate_quoted_reply_resumes_agent_once() {
    let temp = tempfile::tempdir().unwrap();
    let database_path = temp.path().join("state.db");
    let shared = support::SharedAdapters::new();
    let store = initialize_store(&database_path, &shared).await;
    let core = support::CoreServices::new(store.clone(), &shared);

    core.ingest
        .ingest(completed_event(
            "75fe53aa-2314-4c21-b12e-773efce521d9",
            "event-1",
            "session-1",
        ))
        .await
        .unwrap();
    core.delivery.process_next().await.unwrap();
    let external_message_id = shared
        .channel
        .last_receipt(ACCOUNT_ID)
        .unwrap()
        .external_message_id
        .unwrap();
    let message = quoted_message("reply-1", external_message_id.as_str(), "继续处理");

    let (first, second) = tokio::join!(
        core.reply.handle(message.clone()),
        core.reply.handle(message.clone())
    );
    let outcomes = [first.unwrap(), second.unwrap()];
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(outcome, ReplyOutcome::Accepted { .. }))
            .count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(outcome, ReplyOutcome::AlreadyClaimed { .. }))
            .count(),
        1
    );
    assert_eq!(shared.agent.resume_count(), 1);

    let claim = store
        .find_claim(&message.claim_key().unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(claim.state, ClaimState::Completed);
}
