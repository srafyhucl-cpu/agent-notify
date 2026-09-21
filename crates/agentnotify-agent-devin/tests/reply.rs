//! Devin 精确回复：会话号必须先解析成桌面端 Cascade 标识；
//! 未登记、标识不匹配或扩展离线都明确失败，绝不回退到最近会话。

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use agentnotify_agent_devin::{
    DevinAgent, DevinDesktopSessions, DevinInboxState, DevinReplyInbox, DevinSessions,
};
use agentnotify_agent_sdk::{AgentAdapter, AgentError};
use agentnotify_domain::AgentSessionId;

const SESSION_ID: &str = "session-1";
const CASCADE_ID: &str = "acp/devin-cli/session-1";
const READY_WAIT: Duration = Duration::from_secs(5);

fn session(id: &str) -> AgentSessionId {
    AgentSessionId::new(id).unwrap()
}

/// 只写入指定条目的桌面端状态库；测试都指向临时目录，绝不读真实 Devin 数据。
fn write_desktop_database(database: &Path, entries: &[(String, String)]) {
    let connection = rusqlite::Connection::open(database).unwrap();
    connection
        .execute_batch("CREATE TABLE ItemTable (key TEXT PRIMARY KEY, value TEXT);")
        .unwrap();
    for (key, value) in entries {
        connection
            .execute(
                "INSERT INTO ItemTable (key, value) VALUES (?1, ?2)",
                rusqlite::params![key, value],
            )
            .unwrap();
    }
}

fn desktop_key(session_id: &str) -> String {
    format!("windsurf.acp.sessioninfo.session.acp/devin-cli/{session_id}")
}

fn desktop_entry(session_id: &str, cascade_id: &str) -> (String, String) {
    (
        desktop_key(session_id),
        serde_json::json!({"info": {"sessionId": cascade_id}}).to_string(),
    )
}

fn fresh_timestamp() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap()
}

fn write_heartbeat(root: &Path, name: &str, ready: bool, timestamp: &str) {
    let directory = root.join("heartbeats");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join(format!("{name}.json")),
        serde_json::json!({"ready": ready, "timestamp": timestamp}).to_string(),
    )
    .unwrap();
}

fn ready_inbox(root: &Path) -> DevinReplyInbox {
    write_heartbeat(root, "extension-1", true, &fresh_timestamp());
    DevinReplyInbox::new(root)
}

/// 模拟扩展：认领 pending 任务后按给定结果回写 results，并返回任务内容。
async fn auto_confirm_pending(
    root: PathBuf,
    result: serde_json::Value,
) -> (String, serde_json::Value) {
    let pending = root.join("pending");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(mut entries) = tokio::fs::read_dir(&pending).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                if path.extension().and_then(|value| value.to_str()) != Some("json") {
                    continue;
                }
                let bytes = tokio::fs::read(&path).await.unwrap_or_default();
                let job: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                let id = job["id"].as_str().unwrap().to_owned();
                tokio::fs::create_dir_all(root.join("results"))
                    .await
                    .unwrap();
                tokio::fs::write(
                    root.join("results").join(format!("{id}.json")),
                    serde_json::to_vec(&result).unwrap(),
                )
                .await
                .unwrap();
                return (id, job);
            }
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "等待扩展认领 pending 任务超时"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

fn pending_job_ids(root: &Path) -> Vec<String> {
    let mut ids = Vec::new();
    let Ok(entries) = std::fs::read_dir(root.join("pending")) else {
        return ids;
    };
    for entry in entries.flatten() {
        if let Some(stem) = entry.path().file_stem().and_then(|value| value.to_str()) {
            ids.push(stem.to_owned());
        }
    }
    ids.sort();
    ids
}

#[test]
fn missing_cascade_mapping_fails_instead_of_using_latest_session() {
    let home = tempfile::tempdir().unwrap();
    let database = home.path().join("state.vscdb");
    // 状态库里有别的会话（也就是唯一的“最近会话”），但目标会话没有登记。
    let (key, value) = desktop_entry("session-2", "acp/devin-cli/session-2");
    write_desktop_database(&database, &[(key, value)]);
    let desktop = DevinDesktopSessions::new(&database);

    let error = desktop.resolve_cascade("missing-session").unwrap_err();

    assert_eq!(error.code(), "devin_session_not_found");
    assert!(
        error.to_string().contains("先在 Devin 中打开该会话"),
        "错误必须告诉用户下一步：{error}"
    );
}

#[test]
fn registered_session_resolves_to_explicit_cascade_id() {
    let home = tempfile::tempdir().unwrap();
    let database = home.path().join("state.vscdb");
    let (key, value) = desktop_entry(SESSION_ID, CASCADE_ID);
    write_desktop_database(&database, &[(key, value)]);

    let cascade = DevinDesktopSessions::new(&database)
        .resolve_cascade(SESSION_ID)
        .unwrap();

    assert_eq!(cascade, CASCADE_ID);
}

#[test]
fn unrelated_cascade_identifier_is_rejected() {
    let home = tempfile::tempdir().unwrap();
    let database = home.path().join("state.vscdb");
    let (key, value) = desktop_entry(SESSION_ID, "acp/devin-cli/other-session");
    write_desktop_database(&database, &[(key, value)]);

    let error = DevinDesktopSessions::new(&database)
        .resolve_cascade(SESSION_ID)
        .unwrap_err();

    assert_eq!(error.code(), "devin_desktop_session_mismatch");
    assert!(error.to_string().contains("拒绝回复"), "{error}");
}

#[test]
fn malformed_metadata_is_rejected() {
    let home = tempfile::tempdir().unwrap();
    let database = home.path().join("state.vscdb");
    let key = desktop_key(SESSION_ID);
    write_desktop_database(&database, &[(key, "{ not json".to_owned())]);

    let error = DevinDesktopSessions::new(&database)
        .resolve_cascade(SESSION_ID)
        .unwrap_err();

    assert_eq!(error.code(), "devin_desktop_session_invalid");
}

#[test]
fn missing_desktop_database_is_explicit() {
    let home = tempfile::tempdir().unwrap();

    let error = DevinDesktopSessions::new(home.path().join("missing.vscdb"))
        .resolve_cascade(SESSION_ID)
        .unwrap_err();
    assert_eq!(error.code(), "devin_desktop_db_missing");

    let error = DevinDesktopSessions::without_database()
        .resolve_cascade(SESSION_ID)
        .unwrap_err();
    assert_eq!(error.code(), "devin_desktop_db_missing");
    assert!(error.to_string().contains("不会改投到最近会话"), "{error}");
}

#[tokio::test]
async fn missing_extension_heartbeat_blocks_enqueue() {
    let home = tempfile::tempdir().unwrap();
    let inbox = DevinReplyInbox::new(home.path().join("inbox"));

    let error = inbox
        .resume(&session(SESSION_ID), CASCADE_ID, " 继续检查 ")
        .await
        .unwrap_err();

    assert!(matches!(error, AgentError::Unavailable(_)));
    assert_eq!(error.code(), "devin_extension_not_running");
    assert_eq!(inbox.inspect_state().await, DevinInboxState::NotRunning);
    assert!(
        !home.path().join("inbox").exists(),
        "扩展不在线时不得创建收件箱"
    );
}

#[tokio::test]
async fn stale_heartbeat_reports_offline() {
    let home = tempfile::tempdir().unwrap();
    let inbox = DevinReplyInbox::new(home.path());
    let stale = (time::OffsetDateTime::now_utc() - time::Duration::minutes(1))
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap();
    write_heartbeat(home.path(), "extension-1", true, &stale);

    let error = inbox
        .resume(&session(SESSION_ID), CASCADE_ID, "继续")
        .await
        .unwrap_err();

    assert_eq!(error.code(), "devin_extension_offline");
    assert!(error.to_string().contains("重新打开 Devin"), "{error}");
}

#[tokio::test]
async fn unsupported_desktop_blocks_enqueue() {
    let home = tempfile::tempdir().unwrap();
    let inbox = DevinReplyInbox::new(home.path());
    write_heartbeat(home.path(), "extension-1", false, &fresh_timestamp());

    let error = inbox
        .resume(&session(SESSION_ID), CASCADE_ID, "继续")
        .await
        .unwrap_err();

    assert_eq!(error.code(), "devin_desktop_unsupported");
    assert!(error.to_string().contains("精确回复能力"), "{error}");
    assert!(pending_job_ids(home.path()).is_empty());
}

#[tokio::test]
async fn blank_reply_text_is_rejected_before_any_write() {
    let home = tempfile::tempdir().unwrap();
    let inbox = ready_inbox(home.path());

    let error = inbox
        .resume(&session(SESSION_ID), CASCADE_ID, "   ")
        .await
        .unwrap_err();

    assert!(matches!(error, AgentError::InvalidInput));
    assert!(pending_job_ids(home.path()).is_empty());
}

#[tokio::test]
async fn blank_target_is_rejected_before_any_write() {
    let home = tempfile::tempdir().unwrap();
    let inbox = ready_inbox(home.path());

    let error = inbox
        .resume(&session(SESSION_ID), "   ", "继续")
        .await
        .unwrap_err();

    assert_eq!(error.code(), "devin_reply_target_missing");
    assert!(error.to_string().contains("不会改投到最近会话"), "{error}");
    assert!(pending_job_ids(home.path()).is_empty());
}

#[tokio::test]
async fn confirmed_result_reports_the_exact_target_and_job_identity() {
    let home = tempfile::tempdir().unwrap();
    let root = home.path().to_path_buf();
    let inbox = ready_inbox(&root);
    let writer = tokio::spawn(auto_confirm_pending(
        root.clone(),
        serde_json::json!({"ok": true}),
    ));

    inbox
        .resume_with_timeout(&session(SESSION_ID), CASCADE_ID, " 继续检查 ", READY_WAIT)
        .await
        .unwrap();

    let (id, job) = writer.await.unwrap();
    assert_eq!(id.len(), 32, "扩展只接受 32 位十六进制任务 ID：{id}");
    assert!(id.chars().all(|value| value.is_ascii_hexdigit()), "{id}");
    assert_eq!(job["sessionID"], SESSION_ID);
    assert_eq!(job["targetID"], CASCADE_ID);
    assert_eq!(job["text"], "继续检查");
    assert!(job["createdAt"].as_str().unwrap().contains('T'));
    assert!(job["expiresAt"].as_str().unwrap().contains('T'));
}

#[tokio::test]
async fn unconfirmed_result_is_unknown_and_pending_job_is_kept() {
    let home = tempfile::tempdir().unwrap();
    let inbox = ready_inbox(home.path());

    let error = inbox
        .resume_with_timeout(
            &session(SESSION_ID),
            CASCADE_ID,
            "继续",
            Duration::from_millis(50),
        )
        .await
        .unwrap_err();

    assert!(matches!(error, AgentError::Unknown(_)));
    assert_eq!(error.code(), "devin_reply_unconfirmed");
    assert_eq!(
        pending_job_ids(home.path()).len(),
        1,
        "未确认的任务必须留在 pending，不得自动重放"
    );
    assert_eq!(inbox.pending_count().await, 1);
}

#[tokio::test]
async fn extension_failures_map_to_actionable_errors() {
    // (扩展返回的 code, detail, 期望的错误码, 错误里必须出现的一句话)
    let cases: [(&str, &str, &str, Option<&str>); 9] = [
        (
            "desktop_unavailable",
            "",
            "devin_desktop_unavailable",
            Some("不会改投到新会话"),
        ),
        (
            "invalid_job",
            "",
            "devin_reply_invalid_job",
            Some("重新引用原通知"),
        ),
        (
            "session_not_found",
            "",
            "devin_reply_session_not_found",
            Some("不存在或已删除"),
        ),
        (
            "turn_failed",
            "窗口已关闭",
            "devin_reply_failed",
            Some("窗口已关闭"),
        ),
        (
            "turn_failed",
            "",
            "devin_reply_failed",
            Some("查看 Devin 窗口中的错误提示"),
        ),
        ("agent_missing", "", "devin_agent_missing", Some("Agent")),
        (
            "not_authenticated",
            "",
            "devin_not_authenticated",
            Some("重新登录"),
        ),
        (
            "session_locked",
            "",
            "devin_session_locked",
            Some("被 Devin 占用"),
        ),
        (
            "workspace_untrusted",
            "",
            "devin_workspace_untrusted",
            Some("信任该工作区"),
        ),
    ];

    for (code, detail, expected_code, needle) in cases {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().to_path_buf();
        let inbox = ready_inbox(&root);
        let writer = tokio::spawn(auto_confirm_pending(
            root.clone(),
            serde_json::json!({"ok": false, "code": code, "error": detail}),
        ));

        let error = inbox
            .resume_with_timeout(&session(SESSION_ID), CASCADE_ID, "继续", READY_WAIT)
            .await
            .unwrap_err();
        writer.await.unwrap();

        assert_eq!(error.code(), expected_code, "code={code}");
        let message = error.to_string();
        if let Some(needle) = needle {
            assert!(
                message.contains(needle),
                "code={code} 的错误必须可照做：{message}"
            );
        }
    }
}

#[tokio::test]
async fn unknown_result_code_with_detail_is_sanitized_and_bounded() {
    let home = tempfile::tempdir().unwrap();
    let root = home.path().to_path_buf();
    let inbox = ready_inbox(&root);
    let detail = format!("未知   失败 {}", "字".repeat(600));
    let writer = tokio::spawn(auto_confirm_pending(
        root.clone(),
        serde_json::json!({"ok": false, "error": detail}),
    ));

    let error = inbox
        .resume_with_timeout(&session(SESSION_ID), CASCADE_ID, "继续", READY_WAIT)
        .await
        .unwrap_err();
    writer.await.unwrap();

    assert_eq!(error.code(), "devin_reply_failed");
    let message = error.to_string();
    assert!(!message.contains("  "), "空白必须折叠：{message}");
    assert!(message.len() < 400, "错误详情必须限长：{message}");
}

#[tokio::test]
async fn adapter_resume_requires_a_registered_session() {
    let home = tempfile::tempdir().unwrap();
    let root = home.path().join("inbox");
    let database = home.path().join("state.vscdb");
    // 状态库已经存在且登记了别的会话，目标会话仍然没有精确目标。
    let (key, value) = desktop_entry("session-2", "acp/devin-cli/session-2");
    write_desktop_database(&database, &[(key, value)]);
    let adapter = DevinAgent::new_test()
        .with_sessions(DevinSessions::without_database())
        .with_desktop(DevinDesktopSessions::new(&database))
        .with_inbox(ready_inbox(&root));

    let error = adapter
        .resume(&session("missing-session"), "继续")
        .await
        .unwrap_err();

    assert_eq!(error.code(), "devin_session_not_found");
    assert!(
        pending_job_ids(&root).is_empty(),
        "目标解析失败时不得入队，也不得回退到最近会话"
    );
}

#[tokio::test]
async fn adapter_resume_delivers_to_the_resolved_cascade() {
    let home = tempfile::tempdir().unwrap();
    let root = home.path().join("inbox");
    let database = home.path().join("state.vscdb");
    let (key, value) = desktop_entry(SESSION_ID, CASCADE_ID);
    write_desktop_database(&database, &[(key, value)]);
    let adapter = DevinAgent::new_test()
        .with_desktop(DevinDesktopSessions::new(&database))
        .with_inbox(ready_inbox(&root));
    let writer = tokio::spawn(auto_confirm_pending(
        root.clone(),
        serde_json::json!({"ok": true}),
    ));

    let receipt = adapter
        .resume(&session(SESSION_ID), "继续检查")
        .await
        .unwrap();

    let (_, job) = writer.await.unwrap();
    assert_eq!(receipt.session_id, session(SESSION_ID));
    assert_eq!(job["sessionID"], SESSION_ID);
    assert_eq!(job["targetID"], CASCADE_ID);
}

#[tokio::test]
async fn adapter_without_backend_reports_the_extension_is_not_running() {
    let adapter = DevinAgent::new_test();

    let error = adapter
        .resume(&session(SESSION_ID), "继续")
        .await
        .unwrap_err();

    assert_eq!(error.code(), "devin_extension_not_running");
}

#[tokio::test]
async fn health_and_job_counts_are_stable_for_missing_inbox() {
    let home = tempfile::tempdir().unwrap();
    let inbox = DevinReplyInbox::new(home.path().join("missing"));

    assert_eq!(inbox.inspect_state().await, DevinInboxState::NotRunning);
    assert_eq!(inbox.pending_count().await, 0);
    assert_eq!(inbox.processing_count().await, 0);
    assert!(!home.path().join("missing").exists());
}
