use std::time::Duration;

use agentnotify_agent_opencode::{OpenCodeInboxState, OpenCodeReplyInbox};
use agentnotify_agent_sdk::AgentError;
use agentnotify_domain::AgentSessionId;

async fn ready_inbox() -> (tempfile::TempDir, OpenCodeReplyInbox) {
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
    (temp, inbox)
}

async fn claim_pending_once(inbox: &OpenCodeReplyInbox) -> String {
    let pending = inbox.root().join("pending");
    let processing = inbox.root().join("processing");
    for _ in 0..100 {
        if let Ok(mut entries) = tokio::fs::read_dir(&pending).await {
            if let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                let job_id = path
                    .file_stem()
                    .and_then(|value| value.to_str())
                    .unwrap()
                    .to_owned();
                tokio::fs::create_dir_all(&processing).await.unwrap();
                tokio::fs::rename(path, processing.join(entry.file_name()))
                    .await
                    .unwrap();
                return job_id;
            }
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("测试插件未在期限内认领任务");
}

async fn write_result(inbox: &OpenCodeReplyInbox, job_id: &str, ok: bool, error: &str) {
    let results = inbox.root().join("results");
    tokio::fs::create_dir_all(&results).await.unwrap();
    tokio::fs::write(
        results.join(format!("{job_id}.json")),
        serde_json::json!({"ok": ok, "error": error}).to_string(),
    )
    .await
    .unwrap();
}

/// 认领一个任务并返回运行时实际写出的 JSON 原文，随后写出成功结果让 resume 返回。
async fn capture_job_json(inbox: &OpenCodeReplyInbox) -> String {
    let job_id = claim_pending_once(inbox).await;
    let raw = tokio::fs::read_to_string(
        inbox
            .root()
            .join("processing")
            .join(format!("{job_id}.json")),
    )
    .await
    .unwrap();
    write_result(inbox, &job_id, true, "").await;
    raw
}

#[tokio::test]
async fn timeout_is_unknown_and_job_is_not_replayed() {
    let (_temp, inbox) = ready_inbox().await;
    let session_id = AgentSessionId::new("session-1").unwrap();
    let adapter = agentnotify_agent_opencode::OpenCodeAgent::new(inbox.clone());
    let claim = tokio::spawn({
        let inbox = inbox.clone();
        async move { claim_pending_once(&inbox).await }
    });

    let error = adapter
        .resume_with_timeout(&session_id, "继续处理", Duration::from_millis(50))
        .await
        .unwrap_err();

    assert!(matches!(error, AgentError::Unknown(_)));
    assert_eq!(error.code(), "opencode_resume_unconfirmed");
    let _ = claim.await.unwrap();
    assert_eq!(inbox.pending_count().await, 0);
    assert_eq!(inbox.processing_count().await, 1);
}

#[tokio::test]
async fn plugin_failure_returns_safe_agent_error() {
    let (_temp, inbox) = ready_inbox().await;
    let session_id = AgentSessionId::new("session-1").unwrap();
    let adapter = agentnotify_agent_opencode::OpenCodeAgent::new(inbox.clone());
    let plugin = tokio::spawn({
        let inbox = inbox.clone();
        async move {
            let job_id = claim_pending_once(&inbox).await;
            write_result(&inbox, &job_id, false, "当前会话不存在").await;
        }
    });

    let error = adapter
        .resume_with_timeout(&session_id, "继续处理", Duration::from_secs(2))
        .await
        .unwrap_err();

    plugin.await.unwrap();
    assert_eq!(error.code(), "opencode_resume_failed");
    assert!(!error.to_string().contains("继续处理"));
}

#[tokio::test]
async fn successful_result_returns_accepted_receipt() {
    let (_temp, inbox) = ready_inbox().await;
    let session_id = AgentSessionId::new("session-1").unwrap();
    let adapter = agentnotify_agent_opencode::OpenCodeAgent::new(inbox.clone());
    let plugin = tokio::spawn({
        let inbox = inbox.clone();
        async move {
            let job_id = claim_pending_once(&inbox).await;
            write_result(&inbox, &job_id, true, "").await;
        }
    });

    let receipt = adapter
        .resume_with_timeout(&session_id, "继续处理", Duration::from_secs(2))
        .await
        .unwrap();

    plugin.await.unwrap();
    assert_eq!(receipt.session_id.as_str(), "session-1");
}

/// 任务载荷的字段名是运行时与 OpenCode 插件之间的契约。插件按 OpenCode 自身约定读取
/// `sessionID`；若写成 camelCase 的 `sessionId`，插件会判定“引用回复任务字段不完整”
/// 并拒绝执行。该断言锁定字段名，避免真实链路再次整条失败。
#[tokio::test]
async fn job_payload_uses_plugin_field_names() {
    let (_temp, inbox) = ready_inbox().await;
    let session_id = AgentSessionId::new("session-1").unwrap();
    let adapter = agentnotify_agent_opencode::OpenCodeAgent::new(inbox.clone());
    let reader = tokio::spawn({
        let inbox = inbox.clone();
        async move { capture_job_json(&inbox).await }
    });

    adapter
        .resume_with_timeout(&session_id, "继续处理", Duration::from_secs(2))
        .await
        .unwrap();

    let raw = reader.await.unwrap();
    let value: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let object = value.as_object().unwrap();
    assert_eq!(
        object.get("sessionID").and_then(|value| value.as_str()),
        Some("session-1"),
        "插件按 sessionID 读取任务"
    );
    assert!(
        object.get("sessionId").is_none(),
        "不得写成 camelCase 的 sessionId"
    );
    assert!(object.get("createdAt").is_some());
    assert!(object.get("expiresAt").is_some());
    assert!(object.get("id").is_some());
    assert!(object.get("text").is_some());
}

/// 运行时写出的每个字段都必须由插件接口声明。这条断言直接读插件源码里的 `ReplyJob`，
/// 因此任何单侧改名（例如把 `sessionID` 写成 `sessionId`）都会当场失败——
/// 两侧各自的单元测试都发现不了这类跨语言漂移，真实链路曾因此整条失败。
#[tokio::test]
async fn runtime_job_fields_are_declared_by_plugin_interface() {
    let (_temp, inbox) = ready_inbox().await;
    let session_id = AgentSessionId::new("session-1").unwrap();
    let adapter = agentnotify_agent_opencode::OpenCodeAgent::new(inbox.clone());
    let reader = tokio::spawn({
        let inbox = inbox.clone();
        async move { capture_job_json(&inbox).await }
    });

    adapter
        .resume_with_timeout(&session_id, "继续处理", Duration::from_secs(2))
        .await
        .unwrap();

    let raw = reader.await.unwrap();
    let value: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let runtime_fields: Vec<&str> = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();

    let plugin_source = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("plugin")
            .join("rust")
            .join("agent-notify.ts"),
    )
    .expect("插件源码必须可读");
    let interface = plugin_source
        .split("interface ReplyJob {")
        .nth(1)
        .and_then(|rest| rest.split('}').next())
        .expect("插件必须声明 ReplyJob 接口");
    let plugin_fields: Vec<&str> = interface
        .lines()
        .filter_map(|line| line.trim().split(':').next())
        .map(|name| name.trim().trim_end_matches('?'))
        .filter(|name| !name.is_empty())
        .collect();

    for field in &runtime_fields {
        assert!(
            plugin_fields.contains(field),
            "插件 ReplyJob 未声明运行时写出的字段 {field}；插件声明的是 {plugin_fields:?}"
        );
    }
}
