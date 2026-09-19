use std::fs;

use agentnotify_agent_sdk::AgentEventEnvelope;
use agentnotify_domain::{AgentId, RequestId};
use agentnotify_ingress::{Spool, SpoolError, SpoolLimits};
use tempfile::TempDir;

fn fixture_event(request_id: &str) -> AgentEventEnvelope {
    AgentEventEnvelope {
        request_id: RequestId::new(request_id).unwrap(),
        agent_id: AgentId::new("opencode").unwrap(),
        payload: serde_json::json!({
            "eventType": "session.completed",
            "idempotencyKey": format!("opencode:{request_id}"),
            "body": "任务完成"
        }),
    }
}

fn spool(temp: &TempDir, limits: SpoolLimits) -> Spool {
    Spool::open(temp.path().join("spool"), limits).unwrap()
}

#[test]
fn spool_write_is_atomic_and_bounded() {
    let temp = tempfile::tempdir().unwrap();
    let spool = spool(
        &temp,
        SpoolLimits {
            max_events: 1,
            ..SpoolLimits::default()
        },
    );
    let request_id = "75fe53aa-2314-4c21-b12e-773efce521d9";
    let path = spool.write_event(&fixture_event(request_id)).unwrap();

    assert_eq!(spool.queued_count().unwrap(), 1);
    assert!(path.exists());
    assert_eq!(spool.path_for(request_id).unwrap(), path);
    assert!(matches!(
        spool.write_event(&fixture_event("f4e97519-77be-45b2-ae89-ef286f43cd84")),
        Err(SpoolError::CapacityExceeded)
    ));
}

#[test]
fn duplicate_request_id_is_not_written_twice() {
    let temp = tempfile::tempdir().unwrap();
    let spool = spool(&temp, SpoolLimits::default());
    let request_id = "75fe53aa-2314-4c21-b12e-773efce521d9";

    let first = spool.write_event(&fixture_event(request_id)).unwrap();
    let second = spool.write_event(&fixture_event(request_id)).unwrap();

    assert_eq!(first, second);
    assert_eq!(spool.queued_count().unwrap(), 1);
}

#[test]
fn drain_batch_returns_valid_and_invalid_entries_for_ack_or_quarantine() {
    let temp = tempfile::tempdir().unwrap();
    let spool = spool(&temp, SpoolLimits::default());
    let request_id = "75fe53aa-2314-4c21-b12e-773efce521d9";
    spool.write_event(&fixture_event(request_id)).unwrap();
    fs::write(spool.root().join("999-invalid.json"), b"not-json").unwrap();

    let entries = spool.drain_batch(10).unwrap();
    assert_eq!(entries.len(), 2);

    let invalid = entries
        .iter()
        .find(|entry| entry.event.is_err())
        .expect("应保留无法解析的 spool 条目");
    spool
        .quarantine(invalid, "ingress_invalid_json")
        .expect("无效条目应进入隔离区");
    let valid = entries
        .iter()
        .find(|entry| entry.event.is_ok())
        .expect("应保留有效 spool 条目");
    spool.ack(valid).unwrap();

    assert_eq!(spool.queued_count().unwrap(), 0);
    assert!(
        spool
            .root()
            .join("quarantine")
            .read_dir()
            .unwrap()
            .any(|entry| entry
                .unwrap()
                .path()
                .extension()
                .is_some_and(|value| value == "error"))
    );
}
