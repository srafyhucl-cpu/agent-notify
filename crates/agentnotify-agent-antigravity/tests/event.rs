//! Antigravity 事件语义：只有 `fullyIdle=true` 且带 conversationId 的 Stop 才生成通知，
//! 标题按 annotations → transcript 首条用户请求 → 默认标题降级，摘要只读 transcript 尾部有界字节。

use std::{path::Path, sync::Arc};

use agentnotify_agent_antigravity::{
    ANTIGRAVITY_AGENT_ID, AntigravityAgent, AntigravityReply, DEFAULT_BODY, DEFAULT_TITLE,
    SUMMARY_MAX_CHARS, TRANSCRIPT_TAIL_BYTES, TitleSource, read_transcript_summary,
};
use agentnotify_agent_sdk::{AgentAdapter, AgentError, AgentEventEnvelope, assert_agent_contract};
use agentnotify_domain::{AgentId, RequestId};

const CONVERSATION_ID: &str = "conversation-1";

fn envelope(payload: serde_json::Value) -> AgentEventEnvelope {
    AgentEventEnvelope {
        request_id: RequestId::new(uuid::Uuid::new_v4().to_string()).unwrap(),
        agent_id: AgentId::new(ANTIGRAVITY_AGENT_ID).unwrap(),
        payload,
    }
}

/// 计划里的最小 Stop 事件构造器。
fn antigravity_envelope(fully_idle: bool, conversation_id: &str) -> AgentEventEnvelope {
    envelope(serde_json::json!({
        "conversationId": conversation_id,
        "transcriptPath": "",
        "fullyIdle": fully_idle,
        "error": ""
    }))
}

/// 测试实例都注入不可用后端，保证不会碰到本机真实 Antigravity 进程。
fn agent_with_gemini_home(gemini_home: &Path) -> AntigravityAgent {
    AntigravityAgent::new(gemini_home).with_reply(AntigravityReply::without_backend())
}

fn write_annotation(
    gemini_home: &Path,
    conversation_id: &str,
    content: &str,
) -> std::path::PathBuf {
    let directory = gemini_home.join("antigravity").join("annotations");
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join(format!("{conversation_id}.pbtxt"));
    std::fs::write(&path, content).unwrap();
    path
}

fn write_transcript(path: &Path, lines: &[String]) {
    std::fs::write(path, lines.join("\n")).unwrap();
}

fn transcript_line(step: usize, source: &str, kind: &str, content: &str) -> String {
    serde_json::json!({
        "step_index": step,
        "source": source,
        "type": kind,
        "status": "DONE",
        "content": content,
    })
    .to_string()
}

#[test]
fn non_idle_stop_is_ignored_without_notification() {
    let adapter = AntigravityAgent::new_test();
    assert!(matches!(
        adapter.parse_event(antigravity_envelope(false, "conversation-1")),
        Err(AgentError::Ignored { .. })
    ));
}

#[test]
fn missing_fully_idle_is_ignored_without_notification() {
    let adapter = AntigravityAgent::new_test();
    let error = adapter
        .parse_event(envelope(serde_json::json!({
            "conversationId": CONVERSATION_ID,
            "transcriptPath": ""
        })))
        .unwrap_err();

    assert!(matches!(error, AgentError::Ignored(_)));
    assert_eq!(error.code(), "antigravity_not_idle");
}

#[test]
fn missing_conversation_id_is_ignored_without_reply_route() {
    let adapter = AntigravityAgent::new_test();
    for payload in [
        serde_json::json!({"fullyIdle": true, "transcriptPath": ""}),
        serde_json::json!({"fullyIdle": true, "conversationId": "   ", "transcriptPath": ""}),
        serde_json::json!({"fullyIdle": true, "conversationId": null, "transcriptPath": ""}),
    ] {
        let error = adapter.parse_event(envelope(payload)).unwrap_err();
        assert!(matches!(error, AgentError::Ignored(_)), "{error:?}");
        assert_eq!(error.code(), "antigravity_conversation_missing");
    }
}

#[test]
fn conversation_id_is_the_only_resume_target_and_stays_exact() {
    let adapter = AntigravityAgent::new_test();

    let first = adapter
        .parse_event(antigravity_envelope(true, "conversation-1"))
        .unwrap();
    let second = adapter
        .parse_event(antigravity_envelope(true, "conversation-2"))
        .unwrap();

    assert_eq!(
        first.session_id.as_ref().unwrap().as_str(),
        "conversation-1"
    );
    assert_eq!(
        second.session_id.as_ref().unwrap().as_str(),
        "conversation-2"
    );
    assert_ne!(first.session_id, second.session_id);
    // 没有轮次 ID 时把去重交给入口 requestId，避免同一会话的第二次完成被误判重复。
    assert!(first.idempotency_key.is_none());
}

#[test]
fn annotation_title_wins_without_degradation_note() {
    let home = tempfile::tempdir().unwrap();
    write_annotation(
        home.path(),
        CONVERSATION_ID,
        r#"title:"Antigravity Hook Acceptance Test" last_user_view_time:{seconds:1}"#,
    );

    let event = agent_with_gemini_home(home.path())
        .parse_event(antigravity_envelope(true, CONVERSATION_ID))
        .unwrap();

    assert_eq!(event.title, "Antigravity Hook Acceptance Test");
    assert_eq!(
        event.session_title.as_deref(),
        Some("Antigravity Hook Acceptance Test")
    );
    assert_eq!(event.body, DEFAULT_BODY, "标题命中时不应标记降级");
}

#[test]
fn annotation_title_decodes_escapes() {
    let home = tempfile::tempdir().unwrap();
    write_annotation(
        home.path(),
        CONVERSATION_ID,
        r#"title:"修复 \"标题\" 读取" archived:true"#,
    );

    let event = agent_with_gemini_home(home.path())
        .parse_event(antigravity_envelope(true, CONVERSATION_ID))
        .unwrap();

    assert_eq!(event.title, r#"修复 "标题" 读取"#);
}

#[test]
fn annotation_title_is_a_single_file_name_only() {
    let home = tempfile::tempdir().unwrap();
    // 路径穿越的 conversationId 不允许参与文件名拼接，标题退回默认值。
    let event = agent_with_gemini_home(home.path())
        .parse_event(antigravity_envelope(true, r"..\..\title"))
        .unwrap();

    assert_eq!(event.title, DEFAULT_TITLE);
    assert!(event.body.contains("已使用默认标题"), "{}", event.body);
}

#[test]
fn transcript_first_user_request_is_the_title_fallback() {
    let home = tempfile::tempdir().unwrap();
    let transcript = home.path().join("transcript.jsonl");
    write_transcript(
        &transcript,
        &[
            r#"{"step_index":0,"type":"USER_INPUT","content":"<USER_REQUEST>\nAntigravity Hook Acceptance Test\n</USER_REQUEST>\n<ADDITIONAL_METADATA>ignored</ADDITIONAL_METADATA>"}"#.to_owned(),
            r#"{"step_index":1,"type":"PLANNER_RESPONSE","content":"done"}"#.to_owned(),
        ],
    );

    let event = agent_with_gemini_home(home.path())
        .parse_event(envelope(serde_json::json!({
            "conversationId": CONVERSATION_ID,
            "transcriptPath": transcript.to_string_lossy(),
            "fullyIdle": true
        })))
        .unwrap();

    assert_eq!(event.title, "Antigravity Hook Acceptance Test");
    assert!(
        event.body.contains("已使用首条用户请求作为标题"),
        "必须标记降级来源：{}",
        event.body
    );
}

#[test]
fn no_title_source_falls_back_to_default_title() {
    let event = AntigravityAgent::new_test()
        .parse_event(antigravity_envelope(true, CONVERSATION_ID))
        .unwrap();

    assert_eq!(event.title, DEFAULT_TITLE);
    assert_eq!(event.session_id.unwrap().as_str(), CONVERSATION_ID);
    assert!(event.body.contains("已使用默认标题"), "{}", event.body);
}

#[test]
fn summary_prefers_last_assistant_text_and_skips_user_and_tool_content() {
    let home = tempfile::tempdir().unwrap();
    let transcript = home.path().join("transcript.jsonl");
    write_transcript(
        &transcript,
        &[
            transcript_line(0, "USER_EXPLICIT", "USER_INPUT", "<USER_REQUEST>只做这一件事</USER_REQUEST>"),
            r#"{"step_index":1,"source":"MODEL","type":"PLANNER_RESPONSE","thinking":"不应进入摘要","tool_calls":[{"name":"list_dir","args":{"Path":"d:/x"}}]}"#.to_owned(),
            transcript_line(2, "MODEL", "PLANNER_RESPONSE", "最终回复：标题解析已修复"),
        ],
    );

    let event = agent_with_gemini_home(home.path())
        .parse_event(envelope(serde_json::json!({
            "conversationId": CONVERSATION_ID,
            "transcriptPath": transcript.to_string_lossy(),
            "fullyIdle": true
        })))
        .unwrap();

    assert!(
        event.body.starts_with("最终回复：标题解析已修复"),
        "正文必须是助手文本：{}",
        event.body
    );
    assert!(!event.body.contains("只做这一件事"), "{}", event.body);
    assert!(!event.body.contains("不应进入摘要"), "{}", event.body);
}

#[test]
fn transcript_summary_reads_only_the_bounded_tail() {
    let home = tempfile::tempdir().unwrap();
    let transcript = home.path().join("transcript.jsonl");
    let padding = "x".repeat(TRANSCRIPT_TAIL_BYTES as usize + 4096);
    write_transcript(
        &transcript,
        &[
            transcript_line(0, "MODEL", "GENERIC", &format!("HEAD-ONLY {padding}")),
            transcript_line(1, "MODEL", "GENERIC", "TAIL-CONTENT"),
        ],
    );

    let summary = read_transcript_summary(Some(&transcript));

    assert!(summary.contains("TAIL-CONTENT"), "{summary}");
    assert!(
        !summary.contains("HEAD-ONLY"),
        "只允许读取尾部字节：{summary}"
    );
}

#[test]
fn transcript_summary_is_truncated_with_ellipsis() {
    let home = tempfile::tempdir().unwrap();
    let transcript = home.path().join("transcript.jsonl");
    let long = "字".repeat(SUMMARY_MAX_CHARS + 500);
    write_transcript(
        &transcript,
        &[transcript_line(0, "MODEL", "GENERIC", &long)],
    );

    let summary = read_transcript_summary(Some(&transcript));

    assert_eq!(summary.chars().count(), SUMMARY_MAX_CHARS + 1);
    assert!(summary.ends_with('…'), "截断必须可见");
}

#[test]
fn missing_transcript_is_not_a_blocking_error() {
    let home = tempfile::tempdir().unwrap();

    let event = agent_with_gemini_home(home.path())
        .parse_event(envelope(serde_json::json!({
            "conversationId": CONVERSATION_ID,
            "transcriptPath": home.path().join("missing.jsonl").to_string_lossy(),
            "fullyIdle": true
        })))
        .unwrap();

    assert_eq!(event.session_id.unwrap().as_str(), CONVERSATION_ID);
    assert!(
        event.body.starts_with(DEFAULT_BODY),
        "transcript 不可读时仍要推送：{}",
        event.body
    );
}

#[test]
fn error_field_adds_notice_but_empty_error_does_not() {
    let adapter = AntigravityAgent::new_test();

    let with_error = adapter
        .parse_event(envelope(serde_json::json!({
            "conversationId": CONVERSATION_ID,
            "fullyIdle": true,
            "error": "boom"
        })))
        .unwrap();
    assert!(
        with_error.body.contains("Antigravity 结束时报告了错误"),
        "{}",
        with_error.body
    );

    for empty in [
        serde_json::json!(""),
        serde_json::json!(null),
        serde_json::json!([]),
        serde_json::json!({}),
    ] {
        let event = adapter
            .parse_event(envelope(serde_json::json!({
                "conversationId": CONVERSATION_ID,
                "fullyIdle": true,
                "error": empty
            })))
            .unwrap();
        assert!(
            !event.body.contains("结束时报告了错误"),
            "空 error 不应提示错误：{}",
            event.body
        );
    }
}

#[test]
fn invalid_payload_shapes_are_rejected_as_invalid_events() {
    let adapter = AntigravityAgent::new_test();

    let cases = [
        // fullyIdle 类型非法（Go 解码同样会整条失败）。
        serde_json::json!({"fullyIdle": "true", "conversationId": CONVERSATION_ID}),
        // conversationId 类型非法。
        serde_json::json!({"fullyIdle": true, "conversationId": 42}),
        // transcriptPath 类型非法。
        serde_json::json!({"fullyIdle": true, "conversationId": CONVERSATION_ID, "transcriptPath": 42}),
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
    let adapter = AntigravityAgent::new_test();
    let mut wrong_agent = antigravity_envelope(true, CONVERSATION_ID);
    wrong_agent.agent_id = AgentId::new("codex").unwrap();

    assert_eq!(
        adapter.parse_event(wrong_agent).unwrap_err(),
        AgentError::InvalidEvent
    );
}

#[test]
fn title_sources_use_go_compatible_ids() {
    assert_eq!(TitleSource::Annotations.id(), "annotations");
    assert_eq!(TitleSource::Transcript.id(), "transcript");
    assert_eq!(TitleSource::Default.id(), "fallback");
}

#[tokio::test]
async fn adapter_passes_agent_contract() {
    assert_agent_contract(Arc::new(AntigravityAgent::new_test())).await;
}

#[test]
fn descriptor_and_capabilities_match_plan() {
    let adapter = AntigravityAgent::new_test();
    assert_eq!(adapter.descriptor().id.as_str(), ANTIGRAVITY_AGENT_ID);

    let capabilities = adapter.capabilities();
    assert!(capabilities.notify);
    assert!(capabilities.resume);
    assert!(capabilities.session_title);
    assert!(capabilities.hook_installer);
    assert!(!capabilities.reply_window);
}

#[tokio::test]
async fn inspect_reports_hook_installation() {
    let home = tempfile::tempdir().unwrap();
    let adapter = agent_with_gemini_home(home.path());

    let health = adapter.inspect().await;
    assert!(!health.available, "未安装 Hook 时应报告不可用");

    let config_dir = home.path().join("config");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(
        config_dir.join("hooks.json"),
        serde_json::json!({
            "linkweixin-notify": {
                "Stop": [{"type": "command", "command": "D:\\app\\linkWeixin\\linkweixin.exe antigravity"}]
            },
            "agent-notify": {
                "Stop": [{"type": "command", "command": ".\\agent-notify-hook.cmd antigravity stop", "timeout": 60}]
            }
        })
        .to_string(),
    )
    .unwrap();

    let health = adapter.inspect().await;
    assert!(health.available, "Hook 已接入时应报告可用");
}

#[tokio::test]
async fn inspect_rejects_top_level_key_owned_by_another_tool() {
    let home = tempfile::tempdir().unwrap();
    let config_dir = home.path().join("config");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(
        config_dir.join("hooks.json"),
        serde_json::json!({
            "agent-notify": {"Stop": [{"type": "command", "command": ".\\other.exe antigravity stop"}]}
        })
        .to_string(),
    )
    .unwrap();

    let health = agent_with_gemini_home(home.path()).inspect().await;
    assert!(!health.available);
    assert_eq!(
        health.detail.as_ref().unwrap().code(),
        "antigravity_hook_not_installed"
    );
}

#[tokio::test]
async fn inspect_reports_invalid_hooks_json() {
    let home = tempfile::tempdir().unwrap();
    let config_dir = home.path().join("config");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(config_dir.join("hooks.json"), "{ not json").unwrap();

    let health = agent_with_gemini_home(home.path()).inspect().await;
    assert!(!health.available);
    assert_eq!(
        health.detail.as_ref().unwrap().code(),
        "antigravity_hooks_invalid"
    );
}
