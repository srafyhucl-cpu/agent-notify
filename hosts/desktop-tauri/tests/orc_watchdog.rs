//! 宿主看门狗（§4.4 兜底）集成测试：插件事件丢失时，看门狗用 OpenCode 会话快照
//! 自动回注「当前步已完成但宿主没收到」的汇报并推进任务；不重复回注、不猜历史回合。

use std::sync::{Arc, RwLock};

use agentnotify_desktop::bridge::dto::{
    CreateOrcTaskPayload, OrcTaskIdPayload, OrcTemplateStepConfigDto, SaveOrcTemplateConfigPayload,
};
use agentnotify_desktop::bridge::error::CommandError;
use agentnotify_desktop::production::agent_driver::{AgentDriver, DispatchOptions};
use agentnotify_desktop::production::orc_handler::OrcCommandHandler;
use agentnotify_desktop::production::orc_watchdog::{OrcSessionProbe, OrcWatchdog};
use agentnotify_desktop::production::settings::ProductionSettingsStore;
use agentnotify_domain::{AgentId, AgentSessionId};
use agentnotify_orchestration::{OrcTask, OrcTaskRepository, TemplateResolver};
use agentnotify_storage_sqlite::SqliteStore;
use serde_json::{Value, json};

/// 固定模板「快速修复」：动态模式（模板 + 节点配置）测试用。
const TEMPLATE_QUICKFIX: &str = "template-quickfix";

/// 记录型假 driver（只关心派活次数与目标）。
struct FakeDriver {
    calls: Arc<RwLock<Vec<(String, String)>>>,
}

impl FakeDriver {
    fn new() -> Self {
        Self {
            calls: Arc::new(RwLock::new(Vec::new())),
        }
    }

    fn call_count(&self) -> usize {
        self.calls.read().expect("测试锁").len()
    }
}

#[async_trait::async_trait]
impl AgentDriver for FakeDriver {
    async fn dispatch(
        &self,
        _task_id: &str,
        agent_id: &AgentId,
        session_id: &AgentSessionId,
        _envelope: &str,
        _open: bool,
        _options: &DispatchOptions,
    ) -> Result<(), CommandError> {
        self.calls
            .write()
            .expect("测试锁")
            .push((agent_id.to_string(), session_id.to_string()));
        Ok(())
    }
}

/// 假会话探针：按逻辑会话 id 返回预设会话与消息。
struct FakeProbe {
    sessions: RwLock<Vec<(String, String, Vec<Value>)>>,
}

impl FakeProbe {
    fn new(entries: Vec<(String, String, Vec<Value>)>) -> Self {
        Self {
            sessions: RwLock::new(entries),
        }
    }
}

#[async_trait::async_trait]
impl OrcSessionProbe for FakeProbe {
    async fn resolve_session(&self, logical_id: &str) -> Result<Option<String>, String> {
        Ok(self
            .sessions
            .read()
            .expect("测试锁")
            .iter()
            .find(|(key, _, _)| key == logical_id)
            .map(|(_, session_id, _)| session_id.clone()))
    }

    async fn session_messages(&self, session_id: &str) -> Result<Vec<Value>, String> {
        Ok(self
            .sessions
            .read()
            .expect("测试锁")
            .iter()
            .find(|(_, id, _)| id == session_id)
            .map(|(_, _, messages)| messages.clone())
            .unwrap_or_default())
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

fn dynamic_handler(store: &Arc<SqliteStore>, driver: Arc<FakeDriver>) -> OrcCommandHandler {
    OrcCommandHandler::with_selector(
        None,
        store.clone(),
        ProductionSettingsStore::new(store.clone(), store_home()),
        TemplateResolver::new(),
        None,
        Some(driver),
    )
}

/// 节点配置目录：用临时目录里的配置目录（无配置 = 模板默认 Agent）。
fn store_home() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join("agentnotify-watchdog-config");
    std::fs::create_dir_all(&dir).expect("配置目录必须可创建");
    dir
}

/// 覆盖「快速修复」模板的节点配置（两步都派给 opencode）。
async fn save_quickfix_config(handler: &OrcCommandHandler) {
    let steps = [(1, "opencode"), (2, "opencode")]
        .into_iter()
        .map(|(order, agent)| OrcTemplateStepConfigDto {
            order,
            agent: Some(agent.to_string()),
            model: None,
        })
        .collect();
    handler
        .save_orc_template_config(SaveOrcTemplateConfigPayload {
            template_id: TEMPLATE_QUICKFIX.into(),
            steps,
        })
        .await
        .expect("保存节点配置必须成功");
}

async fn create_and_start(handler: &OrcCommandHandler, dir: &str) -> String {
    let created = handler
        .create(CreateOrcTaskPayload {
            name: Some("看门狗".into()),
            steps: None,
            goal: "看门狗自检".into(),
            template_id: TEMPLATE_QUICKFIX.into(),
            working_dir: dir.into(),
            notify_mode: None,
        })
        .await
        .expect("创建任务必须成功");
    handler
        .start(OrcTaskIdPayload {
            task_id: created.id.clone(),
        })
        .await
        .expect("开始执行必须成功");
    created.id
}

/// 从库里读取任务（断言用）。
async fn load_task(store: &Arc<SqliteStore>, task_id: &str) -> OrcTask {
    let a2a = store
        .get_task(task_id)
        .await
        .expect("读取任务必须成功")
        .expect("任务必须存在");
    OrcTask::from_a2a(a2a).expect("任务元数据必须可解析")
}

fn turn_with_text(completed_at_ms: i64, text: &str) -> Vec<Value> {
    vec![
        json!({"info": {"type": "user", "time": {"created": completed_at_ms - 1_000}} , "parts": []}),
        json!({
            "info": {"type": "assistant", "time": {"created": completed_at_ms, "completed": completed_at_ms}},
            "parts": [{"type": "text", "text": text}],
        }),
        json!({"info": {"type": "idle"}, "parts": []}),
    ]
}

/// 插件事件丢失、回合已完成：看门狗回注汇报并推进到下一步。
#[tokio::test]
async fn recovers_missed_report_and_advances() {
    let (_root, store) = open_sqlite("agentnotify-watchdog-recover-");
    let driver = Arc::new(FakeDriver::new());
    let handler = Arc::new(dynamic_handler(&store, driver.clone()));
    save_quickfix_config(&handler).await;
    let dir = _root.path().to_string_lossy().into_owned();
    let task_id = create_and_start(&handler, &dir).await;

    let dispatched_at = load_task(&store, &task_id)
        .await
        .last_dispatch_at_ms()
        .expect("读取派活时刻必须成功")
        .expect("派活后必须记录派活时刻");
    let completed_at = dispatched_at + 5_000;
    let logical = format!("task-{task_id}-step-1");
    let probe = Arc::new(FakeProbe::new(vec![(
        logical,
        "ses-fake-step-1".into(),
        turn_with_text(completed_at, "第 1 步真实产出"),
    )]));
    let watchdog = OrcWatchdog::new(handler.clone(), store.clone(), probe);

    let recovered = watchdog.run_once(completed_at + 90_000).await;
    assert_eq!(recovered, 1, "必须回注一次");

    let task = load_task(&store, &task_id).await;
    assert_eq!(
        task.current_step().expect("读取当前步必须成功"),
        2,
        "汇报回注后必须推进到第 2 步"
    );
    assert_eq!(
        task.step_report(1).expect("读取产出必须成功").as_deref(),
        Some("第 1 步真实产出")
    );
    assert_eq!(driver.call_count(), 2, "推进后必须派活第 2 步");

    // 再跑一轮：任务已到第 2 步，第 1 步的回合不得重复回注。
    let recovered_again = watchdog.run_once(completed_at + 200_000).await;
    assert_eq!(recovered_again, 0, "同一回合不得重复回注");
    assert_eq!(
        load_task(&store, &task_id)
            .await
            .current_step()
            .expect("读取当前步必须成功"),
        2
    );
}

/// 回合产出早于派活（上一轮残留）：不回注。
#[tokio::test]
async fn ignores_turn_completed_before_dispatch() {
    let (_root, store) = open_sqlite("agentnotify-watchdog-stale-");
    let driver = Arc::new(FakeDriver::new());
    let handler = Arc::new(dynamic_handler(&store, driver.clone()));
    save_quickfix_config(&handler).await;
    let dir = _root.path().to_string_lossy().into_owned();
    let task_id = create_and_start(&handler, &dir).await;

    let dispatched_at = load_task(&store, &task_id)
        .await
        .last_dispatch_at_ms()
        .expect("读取派活时刻必须成功")
        .expect("派活后必须记录派活时刻");
    let logical = format!("task-{task_id}-step-1");
    let probe = Arc::new(FakeProbe::new(vec![(
        logical,
        "ses-fake-step-1".into(),
        turn_with_text(dispatched_at - 5_000, "上一轮残留正文"),
    )]));
    let watchdog = OrcWatchdog::new(handler.clone(), store.clone(), probe);

    let recovered = watchdog.run_once(dispatched_at + 90_000).await;
    assert_eq!(recovered, 0, "早于派活的产出不得回注");
    assert_eq!(
        load_task(&store, &task_id)
            .await
            .current_step()
            .expect("读取当前步必须成功"),
        1
    );
}

/// 旧任务（升级前没有派活时刻）：按会话最后一条 user 消息写基线，并兜底当前回合。
#[tokio::test]
async fn recovers_legacy_task_baseline_and_inflight_turn() {
    let (_root, store) = open_sqlite("agentnotify-watchdog-baseline-");
    let driver = Arc::new(FakeDriver::new());
    let handler = Arc::new(dynamic_handler(&store, driver.clone()));
    save_quickfix_config(&handler).await;
    let dir = _root.path().to_string_lossy().into_owned();
    let task_id = create_and_start(&handler, &dir).await;

    // 模拟升级前任务：清掉派活时刻。
    let mut legacy = load_task(&store, &task_id).await;
    {
        let mut meta = legacy.meta().expect("读取元数据必须成功");
        meta.last_dispatch_at_ms = None;
        let mut value = serde_json::to_value(&legacy.a2a_task).expect("序列化必须成功");
        value["metadata"]["orc"] = serde_json::to_value(&meta).expect("序列化元数据必须成功");
        legacy.a2a_task = serde_json::from_value(value).expect("反序列化必须成功");
    }
    store
        .save_task(&legacy.a2a_task)
        .await
        .expect("保存必须成功");

    let logical = format!("task-{task_id}-step-1");
    let probe = Arc::new(FakeProbe::new(vec![(
        logical,
        "ses-fake-step-1".into(),
        turn_with_text(1_800_000_000_000, "旧任务进行中的回合产出"),
    )]));
    let watchdog = OrcWatchdog::new(handler.clone(), store.clone(), probe);

    // 最后一条 user 消息 = 派活基线（1_799_999_999_000）；回合产出晚于它 → 兜底回注。
    let recovered = watchdog.run_once(1_800_000_100_000).await;
    assert_eq!(recovered, 1, "旧任务的进行中回合必须兜底回注");

    let task = load_task(&store, &task_id).await;
    assert_eq!(
        task.current_step().expect("读取当前步必须成功"),
        2,
        "回注后必须推进到第 2 步"
    );
    // 回注后推进并派活第 2 步（派活时刻被新值覆盖是预期），已回注的回合必须被记录防重复。
    assert_eq!(
        task.last_settled_turn_ms().expect("读取结算时刻必须成功"),
        Some(1_800_000_000_000),
        "看门狗必须记录已回注的回合"
    );
}

/// 旧任务且会话没有任何消息：写 now 基线并跳过（不猜历史回合）。
#[tokio::test]
async fn legacy_task_without_messages_writes_now_baseline() {
    let (_root, store) = open_sqlite("agentnotify-watchdog-baseline-empty-");
    let driver = Arc::new(FakeDriver::new());
    let handler = Arc::new(dynamic_handler(&store, driver.clone()));
    save_quickfix_config(&handler).await;
    let dir = _root.path().to_string_lossy().into_owned();
    let task_id = create_and_start(&handler, &dir).await;

    let mut legacy = load_task(&store, &task_id).await;
    {
        let mut meta = legacy.meta().expect("读取元数据必须成功");
        meta.last_dispatch_at_ms = None;
        let mut value = serde_json::to_value(&legacy.a2a_task).expect("序列化必须成功");
        value["metadata"]["orc"] = serde_json::to_value(&meta).expect("序列化元数据必须成功");
        legacy.a2a_task = serde_json::from_value(value).expect("反序列化必须成功");
    }
    store
        .save_task(&legacy.a2a_task)
        .await
        .expect("保存必须成功");

    let logical = format!("task-{task_id}-step-1");
    let probe = Arc::new(FakeProbe::new(vec![(
        logical,
        "ses-fake-step-1".into(),
        Vec::new(),
    )]));
    let watchdog = OrcWatchdog::new(handler.clone(), store.clone(), probe);

    let now_ms = 1_800_000_000_000;
    let recovered = watchdog.run_once(now_ms).await;
    assert_eq!(recovered, 0, "没有会话消息时不得猜历史回合");

    let task = load_task(&store, &task_id).await;
    assert_eq!(
        task.last_dispatch_at_ms().expect("读取派活时刻必须成功"),
        Some(now_ms)
    );
    assert_eq!(
        task.current_step().expect("读取当前步必须成功"),
        1,
        "不得推进历史任务"
    );
}
