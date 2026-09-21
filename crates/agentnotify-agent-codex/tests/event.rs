use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};

use agentnotify_agent_codex::{
    CODEX_AGENT_ID, CodexAgent, CodexQueue, CommandExecutor, CommandOutput, DEFAULT_BODY,
    DEFAULT_TITLE, TURN_COMPLETE_EVENT, queue_args,
};
use agentnotify_agent_sdk::{AgentAdapter, AgentError, AgentEventEnvelope, assert_agent_contract};
use agentnotify_domain::{AgentId, AgentSessionId, RequestId};

const THREAD_ID: &str = "019abc-thread-1";

fn envelope(payload: serde_json::Value) -> AgentEventEnvelope {
    AgentEventEnvelope {
        request_id: RequestId::new("codex-test-request").unwrap(),
        agent_id: AgentId::new(CODEX_AGENT_ID).unwrap(),
        payload,
    }
}

fn codex_envelope(thread_id: &str, message: &str) -> AgentEventEnvelope {
    envelope(serde_json::json!({
        "type": TURN_COMPLETE_EVENT,
        "thread-id": thread_id,
        "turn-id": "turn-1",
        "input-messages": [message],
        "last-assistant-message": message,
    }))
}

fn write_state_database(home: &Path, version: u64, statements: &[&str]) {
    let connection =
        rusqlite::Connection::open(home.join(format!("state_{version}.sqlite"))).unwrap();
    for statement in statements {
        connection.execute_batch(statement).unwrap();
    }
}

#[test]
fn codex_thread_id_is_required_for_resume_capability() {
    let adapter = CodexAgent::new_test();
    let event = adapter
        .parse_event(codex_envelope("thread-1", "任务完成"))
        .unwrap();

    assert_eq!(event.session_id.unwrap().as_str(), "thread-1");
    assert_eq!(event.session_title.as_deref(), Some("任务完成"));
    assert_eq!(event.title, "任务完成");
    assert!(
        event.body.starts_with("任务完成"),
        "正文必须保留 last-assistant-message：{}",
        event.body
    );
    assert_eq!(
        event.idempotency_key.as_deref(),
        Some("codex:thread-1:turn-1")
    );
}

#[test]
fn missing_thread_id_still_notifies_without_reply_route() {
    let adapter = CodexAgent::new_test();
    let event = adapter
        .parse_event(envelope(serde_json::json!({
            "type": TURN_COMPLETE_EVENT,
            "last-assistant-message": "完成",
            "input-messages": ["无 ID 的任务"],
        })))
        .unwrap();

    assert!(event.session_id.is_none());
    assert_eq!(event.title, "无 ID 的任务");
    assert_eq!(event.body, "完成");
}

#[test]
fn thread_id_alias_is_compatible_and_hyphen_wins() {
    let adapter = CodexAgent::new_test();

    let alias = adapter
        .parse_event(envelope(serde_json::json!({
            "thread_id": "snake-1",
            "last-assistant-message": "完成",
        })))
        .unwrap();
    assert_eq!(alias.session_id.unwrap().as_str(), "snake-1");

    let both = adapter
        .parse_event(envelope(serde_json::json!({
            "thread-id": "hyphen-1",
            "thread_id": "snake-1",
            "last-assistant-message": "完成",
        })))
        .unwrap();
    assert_eq!(both.session_id.unwrap().as_str(), "hyphen-1");

    let numeric = adapter
        .parse_event(envelope(serde_json::json!({
            "thread-id": 12345,
            "last-assistant-message": "完成",
        })))
        .unwrap();
    assert_eq!(numeric.session_id.unwrap().as_str(), "12345");
}

/// `type` 只是信息字段：未知、缺失或非字符串都不能丢弃事件（对齐 Go 版无条件推送的行为）。
#[test]
fn unknown_event_type_still_produces_notification() {
    let adapter = CodexAgent::new_test();
    let type_values = [
        None,
        Some(serde_json::json!("session.started")),
        Some(serde_json::json!("agent-turn-complete")),
        Some(serde_json::json!(42)),
    ];

    for type_value in type_values {
        let mut payload = serde_json::json!({
            "thread-id": "thread-1",
            "turn-id": "turn-1",
            "input-messages": ["任务"],
            "last-assistant-message": "完成",
        });
        if let Some(value) = type_value {
            payload["type"] = value;
        }

        let event = adapter.parse_event(envelope(payload)).unwrap();
        assert_eq!(event.session_id.unwrap().as_str(), "thread-1");
        assert_eq!(event.title, "任务");
        assert!(
            event.body.starts_with("完成"),
            "正文规则不因 type 变化：{}",
            event.body
        );
        assert_eq!(
            event.idempotency_key.as_deref(),
            Some("codex:thread-1:turn-1")
        );
    }
}

#[test]
fn title_prefers_state_database_columns_in_order() {
    let cases = [
        (
            "CREATE TABLE threads (id TEXT, name TEXT, title TEXT, first_user_message TEXT);",
            "INSERT INTO threads VALUES ('019abc-thread-1', '会话名', '线程标题', '第一条消息');",
            "会话名",
        ),
        (
            "CREATE TABLE threads (id TEXT, title TEXT, first_user_message TEXT);",
            "INSERT INTO threads VALUES ('019abc-thread-1', '线程标题', '第一条消息');",
            "线程标题",
        ),
        (
            "CREATE TABLE threads (id TEXT, first_user_message TEXT);",
            "INSERT INTO threads VALUES ('019abc-thread-1', '第一条消息');",
            "第一条消息",
        ),
    ];

    for (schema, row, expected) in cases {
        let home = tempfile::tempdir().unwrap();
        write_state_database(home.path(), 5, &[schema, row]);
        let event = CodexAgent::new(home.path())
            .parse_event(codex_envelope(THREAD_ID, "完成"))
            .unwrap();

        assert_eq!(event.title, expected);
        assert_eq!(event.session_title.as_deref(), Some(expected));
        assert_eq!(event.body, "完成", "标题来自状态库时不应标记降级");
    }
}

#[test]
fn newest_state_database_wins_and_older_one_is_used_as_fallback() {
    let home = tempfile::tempdir().unwrap();
    write_state_database(
        home.path(),
        9,
        &[
            "CREATE TABLE threads (id TEXT, name TEXT);",
            "INSERT INTO threads VALUES ('019abc-thread-1', '旧库会话');",
        ],
    );
    write_state_database(
        home.path(),
        10,
        &[
            "CREATE TABLE threads (id TEXT, name TEXT);",
            "INSERT INTO threads VALUES ('other-thread', '其他会话');",
        ],
    );

    let event = CodexAgent::new(home.path())
        .parse_event(codex_envelope(THREAD_ID, "完成"))
        .unwrap();
    assert_eq!(event.title, "旧库会话");
}

#[test]
fn non_numeric_state_database_is_still_discovered() {
    // state_x.sqlite 没有数字版本，仍要被查询：只有它存在时应报告“无法读取”而不是“未找到”。
    let home = tempfile::tempdir().unwrap();
    std::fs::write(home.path().join("state_x.sqlite"), b"not a database").unwrap();

    let event = CodexAgent::new(home.path())
        .parse_event(codex_envelope(THREAD_ID, "完成"))
        .unwrap();

    assert_eq!(event.title, "完成");
    assert!(
        event
            .body
            .contains("标题读取失败：无法读取 Codex 会话数据库"),
        "{}",
        event.body
    );
}

#[test]
fn session_index_is_used_when_database_has_no_title() {
    let home = tempfile::tempdir().unwrap();
    write_state_database(
        home.path(),
        5,
        &[
            "CREATE TABLE threads (id TEXT, name TEXT);",
            "INSERT INTO threads VALUES ('other-thread', '其他会话');",
        ],
    );
    std::fs::write(
        home.path().join("session_index.jsonl"),
        format!("{{\"id\":\"{THREAD_ID}\",\"thread_name\":\"索引会话名\"}}\n"),
    )
    .unwrap();

    let event = CodexAgent::new(home.path())
        .parse_event(codex_envelope(THREAD_ID, "完成"))
        .unwrap();
    assert_eq!(event.title, "索引会话名");
    assert!(
        !event.body.contains("标题读取失败"),
        "状态库可读时不应标记降级：{}",
        event.body
    );
}

#[test]
fn missing_state_database_uses_payload_and_marks_degradation() {
    let home = tempfile::tempdir().unwrap();
    let event = CodexAgent::new(home.path())
        .parse_event(codex_envelope(THREAD_ID, "完成"))
        .unwrap();

    assert_eq!(event.title, "完成");
    assert_eq!(event.session_id.unwrap().as_str(), THREAD_ID);
    assert!(
        event
            .body
            .contains("标题读取失败：未找到 Codex 会话数据库，已回退为任务摘要。"),
        "正文必须标记降级来源：{}",
        event.body
    );
}

#[test]
fn no_title_source_falls_back_to_default_title() {
    let home = tempfile::tempdir().unwrap();
    let event = CodexAgent::new(home.path())
        .parse_event(envelope(serde_json::json!({
            "thread-id": THREAD_ID,
            "last-assistant-message": "完成",
        })))
        .unwrap();

    assert_eq!(event.title, DEFAULT_TITLE);
    assert!(event.body.contains("已回退为默认标题。"), "{}", event.body);
}

#[test]
fn missing_last_assistant_message_uses_default_body() {
    let event = CodexAgent::new_test()
        .parse_event(envelope(serde_json::json!({
            "thread-id": "thread-1",
            "input-messages": ["任务"],
        })))
        .unwrap();

    assert!(
        event.body.starts_with(DEFAULT_BODY),
        "缺失 last-assistant-message 时用默认正文：{}",
        event.body
    );
}

/// 假执行器的固定返回：状态码 + 合并输出。
type FakeResponse = Option<(Option<i32>, String)>;

#[derive(Clone, Default)]
struct FakeExecutor {
    calls: Arc<Mutex<Vec<Vec<String>>>>,
    response: Arc<Mutex<FakeResponse>>,
    delay: Option<Duration>,
}

impl FakeExecutor {
    fn returning(status: Option<i32>, output: &str) -> Self {
        Self {
            response: Arc::new(Mutex::new(Some((status, output.to_owned())))),
            ..Self::default()
        }
    }

    fn calls(&self) -> Vec<Vec<String>> {
        self.calls.lock().unwrap().clone()
    }
}

#[async_trait::async_trait]
impl CommandExecutor for FakeExecutor {
    async fn execute(&self, _binary: &Path, args: &[String]) -> std::io::Result<CommandOutput> {
        self.calls.lock().unwrap().push(args.to_vec());
        if let Some(delay) = self.delay {
            tokio::time::sleep(delay).await;
        }
        let (status, output) = self
            .response
            .lock()
            .unwrap()
            .clone()
            .unwrap_or((Some(0), String::new()));
        Ok(CommandOutput { status, output })
    }
}

fn agent_with_executor(executor: Arc<FakeExecutor>) -> CodexAgent {
    CodexAgent::new_test().with_queue(CodexQueue::with_binary("codex-test").with_executor(executor))
}

#[tokio::test]
async fn resume_uses_argument_array_and_never_a_shell() {
    let executor = Arc::new(FakeExecutor::returning(Some(0), ""));
    let adapter = agent_with_executor(executor.clone());
    let session = AgentSessionId::new("thread-1").unwrap();

    adapter
        .resume(&session, "继续 \"检查\" $HOME --help")
        .await
        .unwrap();

    assert_eq!(
        executor.calls(),
        vec![queue_args("thread-1", "继续 \"检查\" $HOME --help")]
    );
}

#[tokio::test]
async fn queue_failures_map_to_distinct_actionable_codes() {
    let session = AgentSessionId::new("019abc").unwrap();
    let cases = [
        (
            "Error: No active session found matching '019abc'.",
            "codex_queue_target_missing",
            "未找到可续聊的目标线程",
        ),
        (
            "Error: no rollout found for thread id 019abc (code -32603)",
            "codex_thread_not_found",
            "不存在或已删除",
        ),
        (
            "Error: thread/queue/add failed: ephemeral thread does not support queued submissions: 019abc",
            "codex_thread_ephemeral",
            "临时会话",
        ),
        (
            "Error: thread/queue/add failed: user message queue is unavailable (code -32600)",
            "codex_queue_unavailable",
            "消息队列",
        ),
        (
            "Error: session 019abc is archived. Run `codex unarchive 019abc` to unarchive it first.",
            "codex_thread_archived",
            "恢复（解档）",
        ),
        (
            "Error: cannot queue through an embedded app server while a local app-server daemon is running",
            "codex_app_server_conflict",
            "重启 Codex",
        ),
        (
            "Error: thread/queue/add failed: invalid thread id: bad id",
            "codex_thread_id_invalid",
            "线程 ID 无效",
        ),
        (
            "the current session does not support thread/queue/add; update or restart",
            "codex_queue_unsupported",
            "不支持 queue",
        ),
    ];

    let mut codes = Vec::new();
    for (output, expected_code, expected_message) in cases {
        let executor = Arc::new(FakeExecutor::returning(Some(1), output));
        let error = agent_with_executor(executor)
            .resume(&session, "继续")
            .await
            .unwrap_err();

        assert_eq!(error.code(), expected_code, "output={output}");
        assert!(
            error.to_string().contains(expected_message),
            "错误信息必须可照做：{}",
            error
        );
        codes.push(error.code().to_owned());
    }
    let unique: std::collections::BTreeSet<_> = codes.iter().collect();
    assert_eq!(unique.len(), codes.len(), "失败原因不得共用兜底错误码");
}

#[tokio::test]
async fn resume_timeout_is_unknown_and_not_retried() {
    let executor = Arc::new(FakeExecutor {
        delay: Some(Duration::from_secs(5)),
        ..FakeExecutor::default()
    });
    let adapter = CodexAgent::new_test().with_queue(
        CodexQueue::with_binary("codex-test")
            .with_executor(executor.clone())
            .with_timeout(Duration::from_millis(30)),
    );
    let session = AgentSessionId::new("thread-1").unwrap();

    let error = adapter.resume(&session, "继续").await.unwrap_err();

    assert!(matches!(error, AgentError::Unknown(_)));
    assert_eq!(error.code(), "codex_queue_unconfirmed");
    assert_eq!(executor.calls().len(), 1, "Unknown 结果不得自动重试");
}

#[tokio::test]
async fn blank_reply_text_is_rejected_without_starting_codex() {
    let executor = Arc::new(FakeExecutor::returning(Some(0), ""));
    let adapter = agent_with_executor(executor.clone());
    let session = AgentSessionId::new("thread-1").unwrap();

    let error = adapter.resume(&session, "   ").await.unwrap_err();

    assert!(matches!(error, AgentError::InvalidInput));
    assert!(executor.calls().is_empty());
}

#[tokio::test]
async fn adapter_passes_agent_contract() {
    assert_agent_contract(Arc::new(CodexAgent::new_test())).await;
}

#[test]
fn descriptor_and_capabilities_match_plan() {
    let adapter = CodexAgent::new_test();
    assert_eq!(adapter.descriptor().id.as_str(), CODEX_AGENT_ID);

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
    let adapter = CodexAgent::new(home.path());

    let health = adapter.inspect().await;
    assert!(!health.available, "未安装 Hook 时应报告不可用");

    std::fs::write(
        home.path().join("config.toml"),
        r#"notify = [ "C:/tools/codex-computer-use.exe", "turn-ended", "--previous-notify", "[\"C:/app/agentnotify-codex-hook.exe\",\"codex\",\"turn-ended\"]" ]"#,
    )
    .unwrap();

    let health = adapter.inspect().await;
    assert!(health.available, "Hook 已接入时应报告可用");
}
