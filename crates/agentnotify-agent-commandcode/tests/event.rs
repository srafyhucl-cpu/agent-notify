//! Command Code 事件语义与回复窗口边界：`run_end` 标题链按
//! `session_titled` → meta.json → transcript 首条用户消息 → 默认标题降级；
//! 窗口未开启、已过、目标会话不在运行或 mod 不支持注入都明确失败，
//! 绝不回退到最近会话，也不把回复留到下一次 run。

use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use agentnotify_agent_commandcode::{
    COMMANDCODE_AGENT_ID, CommandCodeAgent, CommandCodeInboxState, CommandCodeReplyInbox,
    CommandCodeSessions, DEFAULT_BODY, DEFAULT_TITLE, MAX_REPLY_WINDOW_SEC, RUN_END_EVENT,
    clamp_reply_window_sec, parse_event,
};
use agentnotify_agent_sdk::{AgentAdapter, AgentError, AgentEventEnvelope, assert_agent_contract};
use agentnotify_domain::{AgentId, AgentSessionId, RequestId};

const SESSION_ID: &str = "session-1";
const READY_WAIT: Duration = Duration::from_secs(5);

fn session(id: &str) -> AgentSessionId {
    AgentSessionId::new(id).unwrap()
}

fn envelope(payload: serde_json::Value) -> AgentEventEnvelope {
    AgentEventEnvelope {
        request_id: RequestId::new(uuid::Uuid::new_v4().to_string()).unwrap(),
        agent_id: AgentId::new(COMMANDCODE_AGENT_ID).unwrap(),
        payload,
    }
}

/// 计划里的最小 Command Code `run_end` 事件构造器。
fn run_end_envelope(session_id: &str) -> AgentEventEnvelope {
    envelope(serde_json::json!({
        "eventType": RUN_END_EVENT,
        "sessionId": session_id,
        "body": "commandcode result"
    }))
}

fn user_message_line(text: &str) -> serde_json::Value {
    serde_json::json!({
        "type": "message",
        "message": {
            "role": "user",
            "content": [{"type": "text", "text": text}]
        }
    })
}

/// 只写入临时目录里的 Command Code 项目数据；测试绝不读真实 `.commandcode` 数据。
fn write_meta(projects: &Path, slug: &str, session_id: &str, title: &str) {
    let directory = projects.join(slug);
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join(format!("{session_id}.meta.json")),
        serde_json::json!({"title": title}).to_string(),
    )
    .unwrap();
}

fn write_transcript(projects: &Path, slug: &str, session_id: &str, lines: &[serde_json::Value]) {
    let directory = projects.join(slug);
    std::fs::create_dir_all(&directory).unwrap();
    let content = lines
        .iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(directory.join(format!("{session_id}.jsonl")), content).unwrap();
}

fn fresh_timestamp() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap()
}

fn stale_timestamp() -> String {
    (time::OffsetDateTime::now_utc() - time::Duration::minutes(5))
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap()
}

fn write_heartbeat(
    root: &Path,
    name: &str,
    ready: bool,
    session_id: &str,
    window_open: bool,
    timestamp: &str,
) {
    let directory = root.join("heartbeats");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join(format!("{name}.json")),
        serde_json::json!({
            "ready": ready,
            "timestamp": timestamp,
            "sessionId": session_id,
            "windowOpen": window_open
        })
        .to_string(),
    )
    .unwrap();
}

/// 目标会话的 mod 在线且回复窗口开着。
fn ready_inbox(root: &Path, session_id: &str) -> CommandCodeReplyInbox {
    write_heartbeat(root, "mod-1", true, session_id, true, &fresh_timestamp());
    CommandCodeReplyInbox::new(root)
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

/// 模拟 mod：认领 pending 任务后按给定结果回写 results，并返回任务内容。
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
            "等待 mod 认领 pending 任务超时"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[test]
fn run_end_event_prefers_the_session_title_from_the_mod() {
    let home = tempfile::tempdir().unwrap();
    let projects = home.path().join("projects");
    write_meta(&projects, "d--project-demo", SESSION_ID, "元数据标题");
    write_transcript(
        &projects,
        "d--project-demo",
        SESSION_ID,
        &[user_message_line("首条用户消息")],
    );
    let adapter = CommandCodeAgent::new_test(0).with_sessions(CommandCodeSessions::new(&projects));

    let event = adapter
        .parse_event(envelope(serde_json::json!({
            "eventType": RUN_END_EVENT,
            "sessionId": SESSION_ID,
            "title": "会话标题",
            "body": "commandcode result"
        })))
        .unwrap();

    assert_eq!(event.title, "会话标题");
    assert_eq!(event.session_title.as_deref(), Some("会话标题"));
    assert_eq!(event.body, "commandcode result", "标题命中时不应标记降级");
    assert_eq!(event.session_id.unwrap().as_str(), SESSION_ID);
}

#[test]
fn title_falls_back_to_meta_then_transcript_then_default() {
    let home = tempfile::tempdir().unwrap();
    let projects = home.path().join("projects");
    write_meta(&projects, "slug", SESSION_ID, "  多   行  标题  ");
    write_transcript(
        &projects,
        "slug",
        SESSION_ID,
        &[user_message_line("首条用户消息")],
    );
    let adapter = CommandCodeAgent::new_test(0).with_sessions(CommandCodeSessions::new(&projects));

    let with_meta = adapter.parse_event(run_end_envelope(SESSION_ID)).unwrap();
    assert_eq!(with_meta.title, "多 行 标题");
    assert_eq!(with_meta.body, "commandcode result");

    let transcript_only = home.path().join("transcript-only");
    write_transcript(
        &transcript_only,
        "slug",
        SESSION_ID,
        &[user_message_line("首条用户消息")],
    );
    let adapter =
        CommandCodeAgent::new_test(0).with_sessions(CommandCodeSessions::new(&transcript_only));
    let with_transcript = adapter.parse_event(run_end_envelope(SESSION_ID)).unwrap();
    assert_eq!(with_transcript.title, "首条用户消息");

    let empty = home.path().join("empty-projects");
    let adapter = CommandCodeAgent::new_test(0).with_sessions(CommandCodeSessions::new(&empty));
    let fallback = adapter.parse_event(run_end_envelope(SESSION_ID)).unwrap();
    assert_eq!(fallback.title, DEFAULT_TITLE);
    assert!(
        fallback.body.contains("已使用默认标题"),
        "降级必须在正文里说明来源：{}",
        fallback.body
    );
}

#[test]
fn transcript_title_uses_the_first_user_message_line_only() {
    let home = tempfile::tempdir().unwrap();
    let projects = home.path().join("projects");
    write_transcript(
        &projects,
        "slug",
        SESSION_ID,
        &[
            serde_json::json!({"type": "message", "message": {"role": "assistant", "content": []}}),
            user_message_line("第一行标题\n第二行不应出现"),
        ],
    );
    let adapter = CommandCodeAgent::new_test(0).with_sessions(CommandCodeSessions::new(&projects));

    let event = adapter.parse_event(run_end_envelope(SESSION_ID)).unwrap();

    assert_eq!(event.title, "第一行标题");
    assert!(!event.title.contains("第二行"), "只取首行：{}", event.title);
}

#[test]
fn transcript_title_is_bounded_and_never_crosses_sessions() {
    let home = tempfile::tempdir().unwrap();
    let projects = home.path().join("projects");
    write_transcript(
        &projects,
        "slug",
        SESSION_ID,
        &[user_message_line(&"字".repeat(100))],
    );
    // 项目目录里还有别的会话，但目标会话没有 transcript，不得借用。
    write_transcript(
        &projects,
        "slug",
        "session-2",
        &[user_message_line("别的会话")],
    );
    let adapter = CommandCodeAgent::new_test(0).with_sessions(CommandCodeSessions::new(&projects));

    let event = adapter.parse_event(run_end_envelope(SESSION_ID)).unwrap();
    assert_eq!(event.title.chars().count(), 40);

    let other = adapter.parse_event(run_end_envelope("session-3")).unwrap();
    assert_eq!(other.title, DEFAULT_TITLE, "没有目标的标题时必须降级");
}

#[test]
fn missing_session_id_still_notifies_without_a_reply_route() {
    let adapter = CommandCodeAgent::new_test(0);

    let event = adapter
        .parse_event(envelope(serde_json::json!({
            "eventType": RUN_END_EVENT,
            "title": "会话标题",
            "body": "commandcode result"
        })))
        .unwrap();

    assert!(event.session_id.is_none(), "缺失会话号不得伪造会话");
    assert_eq!(event.title, "会话标题");
}

#[test]
fn missing_body_falls_back_to_the_default_body() {
    let adapter = CommandCodeAgent::new_test(0);

    for body in [
        serde_json::json!(null),
        serde_json::json!(""),
        serde_json::json!("   "),
    ] {
        let event = adapter
            .parse_event(envelope(serde_json::json!({
                "eventType": RUN_END_EVENT,
                "sessionId": SESSION_ID,
                "body": body
            })))
            .unwrap();
        assert!(
            event.body.starts_with(DEFAULT_BODY),
            "空正文必须退回默认正文：{}",
            event.body
        );
    }
}

#[test]
fn invalid_payload_shapes_are_rejected_as_invalid_events() {
    let adapter = CommandCodeAgent::new_test(0);

    let cases = [
        // eventType 类型非法。
        serde_json::json!({"eventType": 42, "sessionId": SESSION_ID}),
        // 未知事件类型不能产生通知。
        serde_json::json!({"eventType": "run_start", "sessionId": SESSION_ID}),
        serde_json::json!({"eventType": "agentnotify.unknown", "sessionId": SESSION_ID}),
        // sessionId 类型非法。
        serde_json::json!({"eventType": RUN_END_EVENT, "sessionId": 42}),
        // title / body 类型非法。
        serde_json::json!({"eventType": RUN_END_EVENT, "sessionId": SESSION_ID, "title": 42}),
        serde_json::json!({"eventType": RUN_END_EVENT, "sessionId": SESSION_ID, "body": 42}),
    ];
    for payload in cases {
        let error = adapter.parse_event(envelope(payload.clone())).unwrap_err();
        assert_eq!(error, AgentError::InvalidEvent, "payload={payload}");
    }

    let non_object = adapter
        .parse_event(envelope(serde_json::json!("not-an-object")))
        .unwrap_err();
    assert_eq!(non_object, AgentError::InvalidEvent);
}

#[test]
fn other_agents_are_rejected() {
    let adapter = CommandCodeAgent::new_test(0);
    let mut wrong_agent = run_end_envelope(SESSION_ID);
    wrong_agent.agent_id = AgentId::new("opencode").unwrap();

    assert_eq!(
        adapter.parse_event(wrong_agent).unwrap_err(),
        AgentError::InvalidEvent
    );
}

#[test]
fn event_module_accepts_the_sessions_store_directly() {
    let event = parse_event(
        run_end_envelope(SESSION_ID),
        &CommandCodeSessions::without_backend(),
    )
    .unwrap();

    assert_eq!(event.title, DEFAULT_TITLE);
    assert_eq!(event.session_id.unwrap().as_str(), SESSION_ID);
}

#[test]
fn descriptor_and_capabilities_match_plan() {
    let adapter = CommandCodeAgent::new_test(0);
    assert_eq!(adapter.descriptor().id.as_str(), COMMANDCODE_AGENT_ID);
    assert_eq!(adapter.descriptor().display_name, "CommandCode");

    let capabilities = adapter.capabilities();
    assert!(capabilities.notify);
    assert!(capabilities.resume);
    assert!(capabilities.session_title);
    assert!(capabilities.hook_installer);
    assert!(
        capabilities.reply_window,
        "Command Code 是唯一有回复窗口的适配器"
    );
}

#[test]
fn reply_window_is_clamped_to_the_documented_range() {
    assert_eq!(clamp_reply_window_sec(0), 0);
    assert_eq!(clamp_reply_window_sec(60), 60);
    assert_eq!(
        clamp_reply_window_sec(MAX_REPLY_WINDOW_SEC + 1),
        MAX_REPLY_WINDOW_SEC
    );
    assert_eq!(
        CommandCodeAgent::new_test(9_999).reply_window_sec(),
        MAX_REPLY_WINDOW_SEC
    );
}

#[tokio::test]
async fn adapter_passes_agent_contract() {
    assert_agent_contract(Arc::new(CommandCodeAgent::new_test(0))).await;
}

#[tokio::test]
async fn inspect_reports_mod_installation() {
    let home = tempfile::tempdir().unwrap();
    let root = home.path().join("inbox");
    let adapter = CommandCodeAgent::new_test(0).with_inbox(CommandCodeReplyInbox::new(&root));

    let health = adapter.inspect().await;
    assert!(!health.available, "没有心跳时必须报告未接入");
    assert_eq!(
        health.detail.as_ref().unwrap().code(),
        "commandcode_mod_not_found"
    );

    write_heartbeat(&root, "mod-1", true, SESSION_ID, false, &fresh_timestamp());
    let health = adapter.inspect().await;
    assert!(health.available, "mod 在线时应报告可用");
    assert!(health.detail.is_none());
}

/// 计划 Step 1 的回复窗口边界测试：窗口为 0 时明确失败，且不写任何任务。
#[tokio::test]
async fn zero_reply_window_rejects_resume_without_blocking_run() {
    let home = tempfile::tempdir().unwrap();
    let root = home.path().join("inbox");
    let adapter = CommandCodeAgent::new_test(0).with_inbox(CommandCodeReplyInbox::new(&root));

    let error = adapter
        .resume(&session(SESSION_ID), "继续")
        .await
        .unwrap_err();

    assert_eq!(error.code(), "commandcode_reply_window_closed");
    assert!(
        error.to_string().contains("回复窗口未开启"),
        "错误必须告诉用户下一步：{error}"
    );
    assert!(!root.exists(), "窗口未开启时不得创建收件箱，也不得挂住 run");
}

#[tokio::test]
async fn window_closed_heartbeat_reports_expired_without_writing_a_job() {
    let home = tempfile::tempdir().unwrap();
    let root = home.path().to_path_buf();
    write_heartbeat(&root, "mod-1", true, SESSION_ID, false, &fresh_timestamp());
    let adapter = CommandCodeAgent::new_test(60).with_inbox(CommandCodeReplyInbox::new(&root));

    let error = adapter
        .resume(&session(SESSION_ID), "继续")
        .await
        .unwrap_err();

    assert_eq!(error.code(), "commandcode_reply_window_expired");
    assert!(error.to_string().contains("下一次通知"), "{error}");
    assert!(pending_job_ids(&root).is_empty(), "窗口已过时不得写任务");
}

#[tokio::test]
async fn session_mismatch_never_falls_back_to_another_session() {
    let home = tempfile::tempdir().unwrap();
    let root = home.path().to_path_buf();
    // 只有别的会话在线，而且它的窗口正开着：绝不能改投。
    write_heartbeat(&root, "mod-2", true, "session-2", true, &fresh_timestamp());
    let adapter = CommandCodeAgent::new_test(60).with_inbox(CommandCodeReplyInbox::new(&root));

    let error = adapter
        .resume(&session(SESSION_ID), "继续")
        .await
        .unwrap_err();

    assert_eq!(error.code(), "commandcode_session_not_running");
    assert!(error.to_string().contains("不会改投到其他会话"), "{error}");
    assert!(pending_job_ids(&root).is_empty(), "不得为别的会话写任务");
}

#[tokio::test]
async fn stale_heartbeat_reports_session_not_running() {
    let home = tempfile::tempdir().unwrap();
    let root = home.path().to_path_buf();
    write_heartbeat(&root, "mod-1", true, SESSION_ID, true, &stale_timestamp());
    let adapter = CommandCodeAgent::new_test(60).with_inbox(CommandCodeReplyInbox::new(&root));

    let error = adapter
        .resume(&session(SESSION_ID), "继续")
        .await
        .unwrap_err();

    assert_eq!(error.code(), "commandcode_session_not_running");
    assert!(error.to_string().contains("先打开该会话"), "{error}");
    assert!(pending_job_ids(&root).is_empty());
}

#[tokio::test]
async fn missing_heartbeat_reports_the_mod_is_not_running() {
    let home = tempfile::tempdir().unwrap();
    let root = home.path().join("inbox");
    let adapter = CommandCodeAgent::new_test(60).with_inbox(CommandCodeReplyInbox::new(&root));

    let error = adapter
        .resume(&session(SESSION_ID), "继续")
        .await
        .unwrap_err();

    assert_eq!(error.code(), "commandcode_mod_not_running");
    assert!(error.to_string().contains("运行安装器"), "{error}");
    assert_eq!(
        adapter.inbox().inspect_state(SESSION_ID).await,
        CommandCodeInboxState::NotRunning
    );
    assert!(!root.exists(), "mod 不在线时不得创建收件箱");
}

#[tokio::test]
async fn unsupported_heartbeat_reports_unsupported_injection() {
    let home = tempfile::tempdir().unwrap();
    let root = home.path().to_path_buf();
    write_heartbeat(&root, "mod-1", false, SESSION_ID, false, &fresh_timestamp());
    let adapter = CommandCodeAgent::new_test(60).with_inbox(CommandCodeReplyInbox::new(&root));

    let error = adapter
        .resume(&session(SESSION_ID), "继续")
        .await
        .unwrap_err();

    assert_eq!(error.code(), "commandcode_inject_unsupported");
    assert!(error.to_string().contains("更新 Command Code"), "{error}");
    assert!(pending_job_ids(&root).is_empty());
}

#[tokio::test]
async fn invalid_heartbeat_time_is_reported_as_invalid() {
    let home = tempfile::tempdir().unwrap();
    let root = home.path().to_path_buf();
    write_heartbeat(&root, "mod-1", true, SESSION_ID, true, "not-a-time");
    let adapter = CommandCodeAgent::new_test(60).with_inbox(CommandCodeReplyInbox::new(&root));

    let error = adapter
        .resume(&session(SESSION_ID), "继续")
        .await
        .unwrap_err();

    assert_eq!(error.code(), "commandcode_heartbeat_invalid");
    assert!(pending_job_ids(&root).is_empty());
}

#[tokio::test]
async fn blank_reply_text_is_rejected_before_any_write() {
    let home = tempfile::tempdir().unwrap();
    let root = home.path().to_path_buf();
    let adapter = CommandCodeAgent::new_test(60).with_inbox(ready_inbox(&root, SESSION_ID));

    let error = adapter
        .resume(&session(SESSION_ID), "   ")
        .await
        .unwrap_err();

    assert!(matches!(error, AgentError::InvalidInput));
    assert!(pending_job_ids(&root).is_empty());
}

#[tokio::test]
async fn ready_window_writes_the_exact_session_job_and_waits_for_the_result() {
    let home = tempfile::tempdir().unwrap();
    let root = home.path().to_path_buf();
    let adapter = CommandCodeAgent::new_test(60).with_inbox(ready_inbox(&root, SESSION_ID));
    let writer = tokio::spawn(auto_confirm_pending(
        root.clone(),
        serde_json::json!({"ok": true}),
    ));

    let receipt = adapter
        .resume(&session(SESSION_ID), " 继续检查 ")
        .await
        .unwrap();

    let (_, job) = writer.await.unwrap();
    assert_eq!(receipt.session_id, session(SESSION_ID));
    assert_eq!(job["sessionID"], SESSION_ID, "mod 只认领本会话的任务");
    assert_eq!(job["text"], "继续检查");
    assert!(job["createdAt"].as_str().unwrap().contains('T'));
    assert!(job["expiresAt"].as_str().unwrap().contains('T'));
}

#[tokio::test]
async fn window_closed_result_maps_to_expired() {
    let home = tempfile::tempdir().unwrap();
    let root = home.path().to_path_buf();
    let adapter = CommandCodeAgent::new_test(60).with_inbox(ready_inbox(&root, SESSION_ID));
    let writer = tokio::spawn(auto_confirm_pending(
        root.clone(),
        serde_json::json!({"ok": false, "code": "window_closed", "error": "回复窗口已过"}),
    ));

    let error = adapter
        .resume_with_timeout(&session(SESSION_ID), "继续", READY_WAIT)
        .await
        .unwrap_err();
    writer.await.unwrap();

    assert_eq!(error.code(), "commandcode_reply_window_expired");
    assert!(error.to_string().contains("不会留到下一次运行"), "{error}");
}

#[tokio::test]
async fn inject_failure_detail_is_sanitized_and_bounded() {
    let home = tempfile::tempdir().unwrap();
    let root = home.path().to_path_buf();
    let adapter = CommandCodeAgent::new_test(60).with_inbox(ready_inbox(&root, SESSION_ID));
    let detail = format!("未知   失败 {}", "字".repeat(600));
    let writer = tokio::spawn(auto_confirm_pending(
        root.clone(),
        serde_json::json!({"ok": false, "code": "inject_failed", "error": detail}),
    ));

    let error = adapter
        .resume_with_timeout(&session(SESSION_ID), "继续", READY_WAIT)
        .await
        .unwrap_err();
    writer.await.unwrap();

    assert_eq!(error.code(), "commandcode_inject_failed");
    let message = error.to_string();
    assert!(!message.contains("  "), "空白必须折叠：{message}");
    assert!(message.len() < 500, "错误详情必须限长：{message}");
}

#[tokio::test]
async fn unconfirmed_result_is_unknown_and_the_job_stays_pending() {
    let home = tempfile::tempdir().unwrap();
    let root = home.path().to_path_buf();
    let adapter = CommandCodeAgent::new_test(60).with_inbox(ready_inbox(&root, SESSION_ID));

    let error = adapter
        .resume_with_timeout(&session(SESSION_ID), "继续", Duration::from_millis(50))
        .await
        .unwrap_err();

    assert!(matches!(error, AgentError::Unknown(_)));
    assert_eq!(error.code(), "commandcode_reply_unconfirmed");
    assert_eq!(
        pending_job_ids(&root).len(),
        1,
        "未确认的任务必须留在 pending，不得自动重放"
    );
    assert_eq!(adapter.inbox().pending_count().await, 1);
    assert_eq!(adapter.inbox().processing_count().await, 0);
}

#[tokio::test]
async fn inbox_state_is_stable_for_a_missing_inbox() {
    let home = tempfile::tempdir().unwrap();
    let inbox = CommandCodeReplyInbox::new(home.path().join("missing"));

    assert_eq!(
        inbox.inspect_state(SESSION_ID).await,
        CommandCodeInboxState::NotRunning
    );
    assert_eq!(inbox.pending_count().await, 0);
    assert_eq!(inbox.processing_count().await, 0);
    assert!(!home.path().join("missing").exists());
}
