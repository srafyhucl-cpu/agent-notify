mod support;

use agentnotify_application::{ClaimStore, IngestResult, ProcessOutcome, ReplyOutcome, RouteStore};
use agentnotify_domain::{ClaimState, RouteKey};
use support::{
    ACCOUNT_ID, AGENT_ID, CHANNEL_ID, completed_event, external_id_for_fixture, initialize_store,
    quoted_message, timestamp,
};

#[tokio::test]
async fn fake_agent_to_fake_channel_creates_exact_reply_route() {
    let temp = tempfile::tempdir().unwrap();
    let database_path = temp.path().join("state.db");
    let shared = support::SharedAdapters::new();
    let store = initialize_store(&database_path, &shared).await;
    let core = support::CoreServices::new(store.clone(), &shared);

    let ingest = core
        .ingest
        .ingest(completed_event(
            "75fe53aa-2314-4c21-b12e-773efce521d9",
            "event-1",
            "session-1",
        ))
        .await
        .unwrap();
    assert!(matches!(ingest, IngestResult::Queued { .. }));

    let outcome = core.delivery.process_next().await.unwrap();
    assert!(matches!(outcome, ProcessOutcome::Completed { .. }));

    let receipt = shared.channel.last_receipt(ACCOUNT_ID).unwrap();
    let external_message_id = receipt.external_message_id.unwrap();
    assert_eq!(external_message_id, external_id_for_fixture());

    let route = store
        .find_route(
            &RouteKey::new(
                agentnotify_domain::ChannelId::new(CHANNEL_ID).unwrap(),
                agentnotify_domain::ChannelAccountId::new(ACCOUNT_ID).unwrap(),
                external_message_id.clone(),
            ),
            timestamp("2026-09-19T09:00:03Z"),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(route.agent_id.as_str(), AGENT_ID);
    assert_eq!(route.session_id.as_str(), "session-1");

    let reply = core
        .reply
        .handle(quoted_message(
            "reply-1",
            external_message_id.as_str(),
            "继续处理",
        ))
        .await
        .unwrap();
    assert!(matches!(reply, ReplyOutcome::Accepted { .. }));
    assert_eq!(shared.agent.resume_count(), 1);
    assert_eq!(
        shared.agent.last_session().unwrap().as_str(),
        route.session_id.as_str()
    );

    let claim_key = quoted_message("reply-1", external_message_id.as_str(), "继续处理")
        .claim_key()
        .unwrap();
    let claim = store.find_claim(&claim_key).await.unwrap().unwrap();
    assert_eq!(claim.state, ClaimState::Completed);
}
