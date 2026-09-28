//! 编排「汇报自动回注」端到端测试（§4.4）：Agent 干完活上报 `session.completed`
//! 且 sessionId 为 `task-<id>-step-<n>` 时，自动推进任务（复用 advance，含下一步派活）。
//! 注入记录型假 driver 与假呈现缺省（不推微信）；不经过真实 Agent 插件。

use std::sync::{Arc, RwLock};

use agentnotify_agent_sdk::AgentEventEnvelope;
use agentnotify_desktop::bridge::dto::{CreateOrcTaskPayload, OrcTaskIdPayload, OrcTaskStateDto};
use agentnotify_desktop::bridge::error::CommandError;
use agentnotify_desktop::production::agent_driver::AgentDriver;
use agentnotify_desktop::production::orc_report_observer::OrcReportObserver;
use agentnotify_desktop::production::service::OrcCommandHandler;
use agentnotify_domain::{AgentId, AgentSessionId, RequestId};
use agentnotify_orchestration::{OrcStore, TemplateResolver, Workflow};
use agentnotify_runtime::AgentEventObserver;
use agentnotify_storage_sqlite::SqliteStore;

/// 记录型假 driver：记下每次派活（任务/Agent/会话/open），恒成功。
struct FakeDriver {
    calls: Arc<RwLock<Vec<(String, String, String, bool)>>>,
}

impl FakeDriver {
    fn new() -> Self {
        Self {
            calls: Arc::new(RwLock::new(Vec::new())),
        }
    }

    fn calls(&self) -> Vec<(String, String, String, bool)> {
        self.calls.read().expect("测试锁").clone()
    }
}

#[async_trait::async_trait]
impl AgentDriver for FakeDriver {
    async fn dispatch(
        &self,
        task_id: &str,
        agent_id: &AgentId,
        session_id: &AgentSessionId,
        _envelope: &str,
        open: bool,
    ) -> Result<(), CommandError> {
        self.calls.write().expect("测试锁").push((
            task_id.to_string(),
            agent_id.to_string(),
            session_id.to_string(),
            open,
        ));
        Ok(())
    }
}

fn open_sqlite(prefix: &str) -> (tempfile::TempDir, Arc<SqliteStore>) {
    let root = tempfile::Builder::new()
        .prefix(prefix)
        .tempdir_in(agentnotify_testkit::test_temp_root())
        .expect("测试临时目录必须可创建");
    let store =
        Arc::new(SqliteStore::open(root.path().join("state.db")).expect("SQLite 数据库必须可创建"));
    (root, store)
}

/// 启用编排 + 注入假 driver 的处理器（预置工作流：codex → opencode → commandcode）。
fn report_handler(store: &Arc<SqliteStore>, driver: Arc<FakeDriver>) -> Arc<OrcCommandHandler> {
    let workflow = Workflow::preset(false).expect("预置工作流必须有效");
    Arc::new(OrcCommandHandler::with_driver(
        Some(OrcStore::with_repository(workflow, store.clone())),
        TemplateResolver::new(),
        None,
        Some(driver),
    ))
}

fn completed_envelope(session_id: &str) -> AgentEventEnvelope {
    AgentEventEnvelope {
        request_id: RequestId::new("req-orc-report-test").expect("有效请求标识"),
        agent_id: AgentId::new("opencode").expect("有效 Agent 标识"),
        payload: serde_json::json!({
            "eventType": "session.completed",
            "sessionId": session_id,
            "title": "【opencode】会话",
            "body": "已完成第 1 步的判断，结论：可行。",
        }),
    }
}

/// 汇报到达（step 1 会话完成）→ 任务自动推进到 step 2，并派活 step 2 的 Agent。
#[tokio::test]
async fn completed_session_advances_task_and_dispatches_next_step() {
    let (_root, store) = open_sqlite("agentnotify-orc-report-advance-");
    let driver = Arc::new(FakeDriver::new());
    let handler = report_handler(&store, driver.clone());
    let observer = OrcReportObserver::new(handler.clone());

    let created = handler
        .create(CreateOrcTaskPayload {
            goal: "做一个贪吃蛇游戏".into(),
            notify_mode: None,
        })
        .await
        .expect("创建任务必须成功");
    assert_eq!(created.current_step, 1);
    assert!(!created.started, "新建任务必须是「待开始」");

    // 人工确认开始：派活第 1 步（新建会话），此后才会有该会话的完成事件。
    handler
        .start(OrcTaskIdPayload {
            task_id: created.id.clone(),
        })
        .await
        .expect("开始执行必须成功");

    observer
        .observe(&completed_envelope(&format!("task-{}-step-1", created.id)))
        .await;

    let tasks = handler.list().await.expect("列出任务必须成功");
    let task = tasks
        .iter()
        .find(|t| t.id == created.id)
        .expect("任务必须存在");
    assert_eq!(task.current_step, 2, "汇报到达后必须自动推进到第 2 步");
    assert_eq!(task.state, OrcTaskStateDto::Working);

    let calls = driver.calls();
    assert_eq!(calls.len(), 2, "开始 + 汇报推进各派活一次：{calls:?}");
    assert_eq!(calls[1].1, "opencode", "第 2 步建议 Agent 必须是 opencode");
    assert_eq!(calls[1].2, format!("task-{}-step-2", created.id));
    assert!(!calls[1].3, "后续步必须续聊同一会话");
}

/// 非编排会话（普通 OpenCode 会话）完成 → 任务不受影响。
#[tokio::test]
async fn foreign_session_is_ignored() {
    let (_root, store) = open_sqlite("agentnotify-orc-report-foreign-");
    let driver = Arc::new(FakeDriver::new());
    let handler = report_handler(&store, driver.clone());
    let observer = OrcReportObserver::new(handler.clone());

    let created = handler
        .create(CreateOrcTaskPayload {
            goal: "目标".into(),
            notify_mode: None,
        })
        .await
        .expect("创建任务必须成功");

    observer
        .observe(&completed_envelope("ses_not_an_orc_session"))
        .await;

    let tasks = handler.list().await.expect("列出任务必须成功");
    let task = tasks
        .iter()
        .find(|t| t.id == created.id)
        .expect("任务必须存在");
    assert_eq!(task.current_step, 1, "非编排会话不得推进任务");
}

/// 步不一致（过期会话完成）→ 任务不受影响（防旧事件误推进）。
#[tokio::test]
async fn stale_step_session_is_ignored() {
    let (_root, store) = open_sqlite("agentnotify-orc-report-stale-");
    let driver = Arc::new(FakeDriver::new());
    let handler = report_handler(&store, driver.clone());
    let observer = OrcReportObserver::new(handler.clone());

    let created = handler
        .create(CreateOrcTaskPayload {
            goal: "目标".into(),
            notify_mode: None,
        })
        .await
        .expect("创建任务必须成功");

    // 任务在 step 1，但事件来自 step 3 的会话（过期/错位）。
    observer
        .observe(&completed_envelope(&format!("task-{}-step-3", created.id)))
        .await;

    let tasks = handler.list().await.expect("列出任务必须成功");
    let task = tasks
        .iter()
        .find(|t| t.id == created.id)
        .expect("任务必须存在");
    assert_eq!(task.current_step, 1, "步不一致的汇报不得推进任务");
}

/// 非 session.completed 事件（如失败事件）→ 不回注（失败由运行时/呈现层各自语义处理）。
#[tokio::test]
async fn non_completed_event_is_ignored() {
    let (_root, store) = open_sqlite("agentnotify-orc-report-event-");
    let driver = Arc::new(FakeDriver::new());
    let handler = report_handler(&store, driver.clone());
    let observer = OrcReportObserver::new(handler.clone());

    let created = handler
        .create(CreateOrcTaskPayload {
            goal: "目标".into(),
            notify_mode: None,
        })
        .await
        .expect("创建任务必须成功");

    let envelope = AgentEventEnvelope {
        request_id: RequestId::new("req-orc-report-failed").expect("有效请求标识"),
        agent_id: AgentId::new("opencode").expect("有效 Agent 标识"),
        payload: serde_json::json!({
            "eventType": "session.failed",
            "sessionId": format!("task-{}-step-1", created.id),
        }),
    };
    observer.observe(&envelope).await;

    let tasks = handler.list().await.expect("列出任务必须成功");
    let task = tasks
        .iter()
        .find(|t| t.id == created.id)
        .expect("任务必须存在");
    assert_eq!(task.current_step, 1, "非完成事件不得推进任务");
}
