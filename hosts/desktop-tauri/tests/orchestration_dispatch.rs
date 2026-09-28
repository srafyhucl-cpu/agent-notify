//! 编排「派活链路」端到端测试（P2）：create/advance 后真实驱动 Agent。
//! 注入记录型假 driver，断言派活目标（Agent/会话/信封文本/open 语义）与
//! 失败 → blocked（§4.6 不自动重推）的自动落库；不经过真实 Agent 插件。

use std::sync::{Arc, RwLock};

use agentnotify_desktop::bridge::dto::{
    AdvanceOrcTaskPayload, CreateOrcTaskPayload, OrcMessageKindDto, OrcTaskIdPayload,
    OrcTaskStateDto,
};
use agentnotify_desktop::bridge::error::CommandError;
use agentnotify_desktop::production::agent_driver::AgentDriver;
use agentnotify_desktop::production::orc_handler::OrcCommandHandler;
use agentnotify_domain::{AgentId, AgentSessionId};
use agentnotify_orchestration::{OrcStore, Workflow, WorkflowStep};
use agentnotify_storage_sqlite::SqliteStore;

/// 记录型假 driver：记下每次派活（任务/Agent/会话/信封/open），可配置失败。
struct FakeDriver {
    calls: Arc<RwLock<Vec<DispatchCall>>>,
    fail: Arc<RwLock<Option<String>>>,
}

/// 一次派活的完整观测（task/agent/session/envelope/open）。
#[derive(Clone, Debug, PartialEq)]
struct DispatchCall {
    task_id: String,
    agent_id: String,
    session_id: String,
    envelope: String,
    open: bool,
}

impl FakeDriver {
    fn new() -> Self {
        Self {
            calls: Arc::new(RwLock::new(Vec::new())),
            fail: Arc::new(RwLock::new(None)),
        }
    }

    /// 让下一次（及以后）派活失败，原因写进 blocked 原因。
    fn fail_with(&self, reason: &str) {
        *self.fail.write().expect("测试锁") = Some(reason.to_string());
    }

    fn calls(&self) -> Vec<DispatchCall> {
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
        envelope: &str,
        open: bool,
    ) -> Result<(), CommandError> {
        self.calls.write().expect("测试锁").push(DispatchCall {
            task_id: task_id.to_string(),
            agent_id: agent_id.to_string(),
            session_id: session_id.to_string(),
            envelope: envelope.to_string(),
            open,
        });
        if let Some(reason) = self.fail.read().expect("测试锁").clone() {
            return Err(CommandError::new("fake_driver_failed", reason));
        }
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
fn dispatched_handler(store: &Arc<SqliteStore>, driver: Arc<FakeDriver>) -> OrcCommandHandler {
    let workflow = Workflow::preset(false).expect("预置工作流必须有效");
    OrcCommandHandler::with_driver(
        Some(OrcStore::with_repository(workflow, store.clone())),
        agentnotify_orchestration::TemplateResolver::new(),
        None,
        Some(driver),
    )
}

async fn create_task(handler: &OrcCommandHandler) -> String {
    let created = handler
        .create(CreateOrcTaskPayload {
            goal: "做一个贪吃蛇游戏".into(),
            notify_mode: None,
        })
        .await
        .expect("创建任务必须成功");
    created.id
}

/// start 派活第 1 步：agent_hint=codex、open=true（新会话）、会话 id 稳定、信封含目标与步数；
/// 创建任务本身不派活（先「待开始」，人工确认后 start）。
#[tokio::test]
async fn start_dispatches_first_step_with_open_session() {
    let (_root, store) = open_sqlite("agentnotify-orc-dispatch-create-");
    let driver = Arc::new(FakeDriver::new());
    let handler = dispatched_handler(&store, driver.clone());

    let task_id = create_task(&handler).await;
    assert!(
        driver.calls().is_empty(),
        "创建任务不得立即派活（需人工点「开始执行」）"
    );

    handler
        .start(OrcTaskIdPayload {
            task_id: task_id.clone(),
        })
        .await
        .expect("开始执行必须成功");

    let calls = driver.calls();
    assert_eq!(calls.len(), 1, "开始执行必须派活一次：{calls:?}");
    let call = &calls[0];
    assert_eq!(call.task_id, task_id);
    assert_eq!(call.agent_id, "codex", "第 1 步建议 Agent 必须是 codex");
    assert_eq!(call.session_id, format!("task-{task_id}-step-1"));
    assert!(call.open, "任务首步必须开新会话");
    assert!(call.envelope.contains("做一个贪吃蛇游戏"), "信封必须含目标");
    assert!(call.envelope.contains("Step 1/3"), "信封必须含步数");
    assert!(call.envelope.contains("判断"), "信封必须含该步角色");
}

/// advance(报告) 后派活下一步：agent_hint=opencode、open=false（续聊）、同一 (task, step) 稳定会话。
#[tokio::test]
async fn advance_dispatches_next_step_with_resume() {
    let (_root, store) = open_sqlite("agentnotify-orc-dispatch-advance-");
    let driver = Arc::new(FakeDriver::new());
    let handler = dispatched_handler(&store, driver.clone());

    let task_id = create_task(&handler).await;
    handler
        .start(OrcTaskIdPayload {
            task_id: task_id.clone(),
        })
        .await
        .expect("开始执行必须成功");
    handler
        .advance(AdvanceOrcTaskPayload {
            task_id: task_id.clone(),
            kind: OrcMessageKindDto::Report,
        })
        .await
        .expect("推进任务必须成功");

    let calls = driver.calls();
    assert_eq!(calls.len(), 2, "开始 + 推进各派活一次：{calls:?}");
    let advanced = &calls[1];
    assert_eq!(advanced.task_id, task_id);
    assert_eq!(
        advanced.agent_id, "opencode",
        "第 2 步建议 Agent 必须是 opencode"
    );
    assert_eq!(advanced.session_id, format!("task-{task_id}-step-2"));
    assert!(!advanced.open, "后续步必须续聊同一会话");
    assert!(
        advanced.envelope.contains("做一个贪吃蛇游戏"),
        "信封必须含目标"
    );
    assert!(advanced.envelope.contains("Step 2/3"), "信封必须含步数");
    assert!(advanced.envelope.contains("规划"), "信封必须含该步角色");
}

/// driver 失败 → 自动 blocked：推进结果仍落库（返回推进态），但任务变 Failed、
/// 阻塞步骤与原因写清（含「插件未连接」），不自动重推。
#[tokio::test]
async fn dispatch_failure_blocks_task_with_clear_reason() {
    let (_root, store) = open_sqlite("agentnotify-orc-dispatch-fail-");
    let driver = Arc::new(FakeDriver::new());
    let handler = dispatched_handler(&store, driver.clone());

    // 先开始执行成功（Step 1 派活正常），再让后续派活失败。
    let task_id = create_task(&handler).await;
    handler
        .start(OrcTaskIdPayload {
            task_id: task_id.clone(),
        })
        .await
        .expect("开始执行必须成功");
    assert_eq!(driver.calls().len(), 1, "开始执行必须已派活 Step 1");
    driver.fail_with("OpenCode 插件未连接，请启动 OpenCode 后重试");

    // 推进本身成功（派活失败不改变已落库的推进结果）：第 1 步汇报 → 第 2 步。
    let advanced = handler
        .advance(AdvanceOrcTaskPayload {
            task_id: task_id.clone(),
            kind: OrcMessageKindDto::Report,
        })
        .await
        .expect("推进必须成功");
    assert_eq!(advanced.state, OrcTaskStateDto::Working);
    assert_eq!(advanced.current_step, 2);

    // 落库侧：任务已自动 blocked（哪一步失败、谁不可用都写清）。
    let tasks = handler.list().await.expect("列出任务必须成功");
    let task = tasks
        .iter()
        .find(|task| task.id == task_id)
        .expect("任务必须还在");
    assert_eq!(task.state, OrcTaskStateDto::Failed);
    assert_eq!(task.blocked_step, Some(2));
    let reason = task.block_reason.as_deref().expect("必须有阻塞原因");
    assert!(
        reason.contains("OpenCode 插件未连接"),
        "必须写清插件未连接：{reason}"
    );
    assert!(reason.contains("Step 2"), "必须写清哪一步失败：{reason}");
}

/// 该步没有配置 Agent（agent_hint 缺失）→ 明确错误 + blocked，原因写清「未配置 Agent」。
#[tokio::test]
async fn missing_agent_hint_blocks_task_with_clear_reason() {
    let (_root, store) = open_sqlite("agentnotify-orc-dispatch-nohint-");
    let driver = Arc::new(FakeDriver::new());
    // 自定义工作流：第 2 步未配置 Agent。
    let workflow = Workflow::new(
        "custom-no-hint",
        "自定义（第 2 步缺 Agent）",
        vec![
            WorkflowStep::new(1, "orchestrator", Some("codex".to_string()), None, false),
            WorkflowStep::new(2, "executor", None, None, false),
            WorkflowStep::new(3, "reviewer", Some("opencode".to_string()), None, false),
        ],
    )
    .expect("自定义工作流必须有效");
    let handler = OrcCommandHandler::with_driver(
        Some(OrcStore::with_repository(workflow, store.clone())),
        agentnotify_orchestration::TemplateResolver::new(),
        None,
        Some(driver),
    );

    let task_id = create_task(&handler).await;
    handler
        .start(OrcTaskIdPayload {
            task_id: task_id.clone(),
        })
        .await
        .expect("开始执行必须成功");
    handler
        .advance(AdvanceOrcTaskPayload {
            task_id: task_id.clone(),
            kind: OrcMessageKindDto::Report,
        })
        .await
        .expect("推进必须成功");

    let tasks = handler.list().await.expect("列出任务必须成功");
    let task = tasks
        .iter()
        .find(|task| task.id == task_id)
        .expect("任务必须还在");
    assert_eq!(task.state, OrcTaskStateDto::Failed);
    assert_eq!(task.blocked_step, Some(2));
    let reason = task.block_reason.as_deref().expect("必须有阻塞原因");
    assert!(
        reason.contains("未配置 Agent"),
        "必须写清未配置 Agent：{reason}"
    );
    assert!(reason.contains("Step 2"), "必须写清哪一步：{reason}");
}

/// 不注入 driver（new / with_presenter）：原有行为零变化——推进 + 呈现，不派活。
/// （P1-3/1-4 既有测试全绿的回归锚点。）
#[tokio::test]
async fn no_driver_keeps_original_advance_behavior() {
    let (_root, store) = open_sqlite("agentnotify-orc-dispatch-nodriver-");
    let workflow = Workflow::preset(false).expect("预置工作流必须有效");
    let handler = OrcCommandHandler::new(Some(OrcStore::with_repository(workflow, store.clone())));

    let created = handler
        .create(CreateOrcTaskPayload {
            goal: "不派活的目标".into(),
            notify_mode: None,
        })
        .await
        .expect("创建任务必须成功");
    assert_eq!(created.state, OrcTaskStateDto::Working);
    assert!(!created.started, "新建任务必须是「待开始」");

    handler
        .start(OrcTaskIdPayload {
            task_id: created.id.clone(),
        })
        .await
        .expect("开始执行必须成功");
    let advanced = handler
        .advance(AdvanceOrcTaskPayload {
            task_id: created.id,
            kind: OrcMessageKindDto::Report,
        })
        .await
        .expect("推进必须成功");
    assert_eq!(advanced.state, OrcTaskStateDto::Working);
    assert_eq!(advanced.current_step, 2);
    assert_eq!(advanced.blocked_step, None, "无 driver 时不得产生阻塞");
}

/// 指令（Instruction）→ BackToWork：重新派活本步，且同一 (task, step) 会话 id 稳定不变。
#[tokio::test]
async fn back_to_work_re_dispatches_stable_session() {
    let (_root, store) = open_sqlite("agentnotify-orc-dispatch-rework-");
    let driver = Arc::new(FakeDriver::new());
    let handler = dispatched_handler(&store, driver.clone());

    let task_id = create_task(&handler).await;
    handler
        .start(OrcTaskIdPayload {
            task_id: task_id.clone(),
        })
        .await
        .expect("开始执行必须成功");
    handler
        .advance(AdvanceOrcTaskPayload {
            task_id: task_id.clone(),
            kind: OrcMessageKindDto::Report,
        })
        .await
        .expect("推进必须成功");
    handler
        .advance(AdvanceOrcTaskPayload {
            task_id: task_id.clone(),
            kind: OrcMessageKindDto::Instruction,
        })
        .await
        .expect("指令必须成功");

    let calls = driver.calls();
    assert_eq!(
        calls.len(),
        3,
        "开始 + 推进 + 回到本步各派活一次：{calls:?}"
    );
    let first_step2 = calls[1].clone();
    let reworked = &calls[2];
    assert_eq!(reworked.task_id, task_id);
    assert_eq!(reworked.agent_id, "opencode");
    assert_eq!(
        reworked.session_id, first_step2.session_id,
        "回到本步必须复用同一 (task, step) 会话"
    );
    assert!(!reworked.open, "回到本步属于续聊，不是新会话");
}

/// 信封渲染衔接：用户模板（含未知占位符）的告警不阻断派活，渲染文本原样送达 driver。
#[tokio::test]
async fn envelope_rendering_feeds_dispatch_text() {
    let (_root, store) = open_sqlite("agentnotify-orc-dispatch-envelope-");
    let driver = Arc::new(FakeDriver::new());
    let workflow = Workflow::preset(false).expect("预置工作流必须有效");
    // 用户模板：占位符 {goal} 生效，未知占位符 {unknown} 原样保留（坏模板不炸）。
    let templates = agentnotify_orchestration::TemplateResolver::from_config_str(
        r#"{"workflows":{"preset-requirement-to-report":{"steps":[
            {"order":1,"harness_template":"【判断】目标：{goal}，未知：{unknown}"}
        ]}}}"#,
    )
    .resolver;
    let handler = OrcCommandHandler::with_driver(
        Some(OrcStore::with_repository(workflow, store.clone())),
        templates,
        None,
        Some(driver.clone()),
    );

    let task_id = handler
        .create(CreateOrcTaskPayload {
            goal: "信封衔接目标".into(),
            notify_mode: None,
        })
        .await
        .expect("创建任务必须成功")
        .id;

    assert!(driver.calls().is_empty(), "创建任务不得立即派活");
    handler
        .start(OrcTaskIdPayload {
            task_id: task_id.clone(),
        })
        .await
        .expect("开始执行必须成功");

    let calls = driver.calls();
    assert_eq!(calls.len(), 1, "开始执行必须派活一次");
    let call = &calls[0];
    assert_eq!(call.task_id, task_id);
    assert!(
        call.envelope.contains("【判断】目标：信封衔接目标"),
        "用户模板必须生效：{}",
        call.envelope
    );
    assert!(
        call.envelope.contains("{unknown}"),
        "未知占位符必须原样保留：{}",
        call.envelope
    );
}
