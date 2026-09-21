//! Devin 事件语义：`stop_hook_active=true` 的递归 Stop 必须跳过；
//! 会话号只取稳定的 `session_id`；正文取 `last_assistant_message`；
//! 标题按 sessions.db → 默认标题降级，缺失时在正文里标记来源。

use std::{path::Path, sync::Arc};

use agentnotify_agent_devin::{
    DEFAULT_BODY, DEFAULT_TITLE, DEVIN_AGENT_ID, DevinAgent, DevinSessions, parse_event,
};
use agentnotify_agent_sdk::{AgentAdapter, AgentError, AgentEventEnvelope, assert_agent_contract};
use agentnotify_domain::{AgentId, RequestId};

const SESSION_ID: &str = "session-1";

fn envelope(payload: serde_json::Value) -> AgentEventEnvelope {
    AgentEventEnvelope {
        request_id: RequestId::new(uuid::Uuid::new_v4().to_string()).unwrap(),
        agent_id: AgentId::new(DEVIN_AGENT_ID).unwrap(),
        payload,
    }
}

/// 计划里的最小 Devin Stop 事件构造器。
fn devin_envelope(stop_hook_active: bool, session_id: &str) -> AgentEventEnvelope {
    envelope(serde_json::json!({
        "session_id": session_id,
        "prompt_id": "prompt-1",
        "hook_event_name": "Stop",
        "stop_hook_active": stop_hook_active,
        "last_assistant_message": "devin result"
    }))
}

/// 只写入指定语句的会话库；测试都指向临时目录，绝不读真实 Devin 数据。
fn write_sessions_database(database: &Path, statements: &[&str]) {
    let connection = rusqlite::Connection::open(database).unwrap();
    for statement in statements {
        connection.execute_batch(statement).unwrap();
    }
}

/// 带隔离会话库的测试实例。
fn agent_with_sessions(database: &Path) -> DevinAgent {
    DevinAgent::new_test().with_sessions(DevinSessions::new(database))
}

#[test]
fn recursive_stop_is_ignored() {
    let adapter = DevinAgent::new_test();
    assert!(matches!(
        adapter.parse_event(devin_envelope(true, "session-1")),
        Err(AgentError::Ignored { .. })
    ));
}

#[test]
fn recursive_stop_is_ignored_with_explicit_reason() {
    let error = DevinAgent::new_test()
        .parse_event(devin_envelope(true, SESSION_ID))
        .unwrap_err();

    assert!(matches!(error, AgentError::Ignored(_)));
    assert_eq!(error.code(), "devin_stop_hook_active");
}

#[test]
fn non_stop_hook_event_is_ignored() {
    for event_name in ["SessionStart", "session_start", "  Stop 2  "] {
        let error = DevinAgent::new_test()
            .parse_event(envelope(serde_json::json!({
                "session_id": SESSION_ID,
                "hook_event_name": event_name,
                "stop_hook_active": false,
            })))
            .unwrap_err();

        assert!(matches!(error, AgentError::Ignored(_)), "{event_name}");
        assert_eq!(error.code(), "devin_hook_event_mismatch", "{event_name}");
    }
}

#[test]
fn missing_hook_event_name_is_treated_as_stop() {
    let event = DevinAgent::new_test()
        .parse_event(envelope(serde_json::json!({
            "session_id": SESSION_ID,
            "stop_hook_active": false,
            "last_assistant_message": "完成"
        })))
        .unwrap();

    assert_eq!(event.session_id.unwrap().as_str(), SESSION_ID);
    assert!(
        event.body.starts_with("完成"),
        "正文必须来自 last_assistant_message：{}",
        event.body
    );
}

#[test]
fn missing_session_id_is_ignored_without_reply_route() {
    let adapter = DevinAgent::new_test();
    for payload in [
        serde_json::json!({"hook_event_name": "Stop", "stop_hook_active": false}),
        serde_json::json!({"session_id": "   ", "hook_event_name": "Stop"}),
        serde_json::json!({"session_id": null, "hook_event_name": "Stop"}),
        // Cascade / prompt / 会话标题字段都不能充当会话号，缺失就跳过。
        serde_json::json!({
            "hook_event_name": "Stop",
            "cascade_id": "acp/devin-cli/session-1",
            "conversation_id": "session-1",
            "prompt_id": "prompt-1"
        }),
    ] {
        let error = adapter.parse_event(envelope(payload.clone())).unwrap_err();
        assert!(matches!(error, AgentError::Ignored(_)), "{payload}");
        assert_eq!(error.code(), "devin_session_missing");
    }
}

#[test]
fn stable_session_id_is_the_only_resume_target() {
    let adapter = DevinAgent::new_test();

    let first = adapter
        .parse_event(devin_envelope(false, "session-1"))
        .unwrap();
    let second = adapter
        .parse_event(devin_envelope(false, "session-2"))
        .unwrap();

    assert_eq!(first.session_id.as_ref().unwrap().as_str(), "session-1");
    assert_eq!(second.session_id.as_ref().unwrap().as_str(), "session-2");
    assert_ne!(first.session_id, second.session_id);
    // Devin Stop 事件没有轮次 ID，去重交给入口 requestId。
    assert!(first.idempotency_key.is_none());
}

#[test]
fn body_prefers_last_assistant_message_and_falls_back_to_default() {
    let adapter = DevinAgent::new_test();

    let with_message = adapter
        .parse_event(devin_envelope(false, SESSION_ID))
        .unwrap();
    assert!(with_message.body.starts_with("devin result"));

    for empty in [
        serde_json::json!(null),
        serde_json::json!(""),
        serde_json::json!("   "),
    ] {
        let event = adapter
            .parse_event(envelope(serde_json::json!({
                "session_id": SESSION_ID,
                "hook_event_name": "Stop",
                "stop_hook_active": false,
                "last_assistant_message": empty
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
fn session_title_comes_from_sessions_database() {
    let home = tempfile::tempdir().unwrap();
    let database = home.path().join("sessions.db");
    write_sessions_database(
        &database,
        &[
            "CREATE TABLE sessions (id TEXT NOT NULL, title TEXT, working_directory TEXT);",
            "INSERT INTO sessions VALUES ('session-1', '真实 Devin 标题', 'D:\\Project\\yueyou');",
        ],
    );

    let event = agent_with_sessions(&database)
        .parse_event(devin_envelope(false, SESSION_ID))
        .unwrap();

    assert_eq!(event.title, "真实 Devin 标题");
    assert_eq!(event.session_title.as_deref(), Some("真实 Devin 标题"));
    assert_eq!(event.body, "devin result", "标题命中时不应标记降级");
}

#[test]
fn session_title_is_collapsed_and_bounded() {
    let home = tempfile::tempdir().unwrap();
    let database = home.path().join("sessions.db");
    write_sessions_database(
        &database,
        &[
            "CREATE TABLE sessions (id TEXT NOT NULL, title TEXT);",
            "INSERT INTO sessions VALUES ('session-1', '  多   行  标题  ');",
        ],
    );

    let event = agent_with_sessions(&database)
        .parse_event(devin_envelope(false, SESSION_ID))
        .unwrap();

    assert_eq!(event.title, "多 行 标题");
}

#[test]
fn another_session_in_the_database_is_not_used_as_title() {
    let home = tempfile::tempdir().unwrap();
    let database = home.path().join("sessions.db");
    write_sessions_database(
        &database,
        &[
            "CREATE TABLE sessions (id TEXT NOT NULL, title TEXT);",
            "INSERT INTO sessions VALUES ('session-2', '别的会话标题');",
        ],
    );

    let event = agent_with_sessions(&database)
        .parse_event(devin_envelope(false, SESSION_ID))
        .unwrap();

    assert_eq!(event.title, DEFAULT_TITLE);
    assert!(event.body.contains("已使用默认标题"), "{}", event.body);
}

#[test]
fn missing_session_title_degrades_with_default_title() {
    let adapter = DevinAgent::new_test();

    let event = adapter
        .parse_event(devin_envelope(false, SESSION_ID))
        .unwrap();

    assert_eq!(event.title, DEFAULT_TITLE);
    assert_eq!(event.session_id.unwrap().as_str(), SESSION_ID);
    assert!(event.body.contains("已使用默认标题"), "{}", event.body);
}

#[test]
fn invalid_payload_shapes_are_rejected_as_invalid_events() {
    let adapter = DevinAgent::new_test();

    let cases = [
        // stop_hook_active 类型非法（Go 解码同样会整条失败）。
        serde_json::json!({"session_id": SESSION_ID, "stop_hook_active": "true"}),
        // hook_event_name 类型非法。
        serde_json::json!({"session_id": SESSION_ID, "hook_event_name": 42}),
        // session_id 类型非法。
        serde_json::json!({"session_id": 42}),
        // last_assistant_message 类型非法。
        serde_json::json!({"session_id": SESSION_ID, "last_assistant_message": 42}),
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
    let adapter = DevinAgent::new_test();
    let mut wrong_agent = devin_envelope(false, SESSION_ID);
    wrong_agent.agent_id = AgentId::new("codex").unwrap();

    assert_eq!(
        adapter.parse_event(wrong_agent).unwrap_err(),
        AgentError::InvalidEvent
    );
}

#[test]
fn event_module_accepts_the_sessions_store_directly() {
    let event = parse_event(
        devin_envelope(false, SESSION_ID),
        &DevinSessions::without_database(),
    )
    .unwrap();

    assert_eq!(event.title, DEFAULT_TITLE);
}

#[test]
fn descriptor_and_capabilities_match_plan() {
    let adapter = DevinAgent::new_test();
    assert_eq!(adapter.descriptor().id.as_str(), DEVIN_AGENT_ID);

    let capabilities = adapter.capabilities();
    assert!(capabilities.notify);
    assert!(capabilities.resume);
    assert!(capabilities.session_title);
    assert!(capabilities.hook_installer);
    assert!(!capabilities.reply_window);
}

#[tokio::test]
async fn adapter_passes_agent_contract() {
    assert_agent_contract(Arc::new(DevinAgent::new_test())).await;
}

#[tokio::test]
async fn inspect_reports_hook_installation() {
    let home = tempfile::tempdir().unwrap();
    let config = home.path().join("config.json");
    let adapter = DevinAgent::new_test().with_hooks_path(&config);

    let health = adapter.inspect().await;
    assert!(!health.available, "未安装 Hook 时应报告不可用");
    assert_eq!(
        health.detail.as_ref().unwrap().code(),
        "devin_config_not_found"
    );

    std::fs::write(
        &config,
        serde_json::json!({
            "hooks": {
                "Stop": [
                    {
                        "matcher": "",
                        "hooks": [
                            {"type": "command", "command": "D:\\other\\linkweixin.exe devin stop"}
                        ]
                    }
                ]
            }
        })
        .to_string(),
    )
    .unwrap();
    let health = adapter.inspect().await;
    assert!(!health.available, "只指向其他程序时必须报告未接入");
    assert_eq!(
        health.detail.as_ref().unwrap().code(),
        "devin_hook_not_installed"
    );

    std::fs::write(
        &config,
        serde_json::json!({
            "hooks": {
                "Stop": [
                    {
                        "matcher": "",
                        "hooks": [
                            {"type": "command", "command": "D:\\other\\linkweixin.exe devin stop"}
                        ]
                    },
                    {
                        "matcher": "",
                        "hooks": [
                            {
                                "type": "command",
                                "command": "D:\\app\\agentnotify-devin-hook.exe devin stop",
                                "timeout": 60
                            }
                        ]
                    }
                ]
            }
        })
        .to_string(),
    )
    .unwrap();

    let health = adapter.inspect().await;
    assert!(health.available, "Hook 已接入时应报告可用");
    assert!(health.detail.is_none());
}

#[tokio::test]
async fn inspect_accepts_the_legacy_agent_notify_entry() {
    let home = tempfile::tempdir().unwrap();
    let config = home.path().join("config.json");
    std::fs::write(
        &config,
        serde_json::json!({
            "hooks": {
                "Stop": [
                    {
                        "hooks": [
                            {"type": "command", "command": "\"C:\\Users\\u\\bin\\agent-notify.exe\" devin stop"}
                        ]
                    }
                ]
            }
        })
        .to_string(),
    )
    .unwrap();

    let health = DevinAgent::new_test()
        .with_hooks_path(&config)
        .inspect()
        .await;

    assert!(health.available, "旧 AgentNotify 入口也算已接入");
}

#[tokio::test]
async fn inspect_reports_invalid_config() {
    let home = tempfile::tempdir().unwrap();
    let config = home.path().join("config.json");
    std::fs::write(&config, "{ not json").unwrap();

    let health = DevinAgent::new_test()
        .with_hooks_path(&config)
        .inspect()
        .await;

    assert!(!health.available);
    assert_eq!(
        health.detail.as_ref().unwrap().code(),
        "devin_config_invalid"
    );
}
