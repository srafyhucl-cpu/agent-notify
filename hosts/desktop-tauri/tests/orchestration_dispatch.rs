//! 编排「派活链路」端到端测试（P2）：create/advance 后真实驱动 Agent。
//! 注入记录型假 driver，断言派活目标（Agent/会话/信封文本/open 语义/派活选项）与
//! 失败 → blocked（§4.6 不自动重推）的自动落库；不经过真实 Agent 插件。

use std::sync::{Arc, RwLock};

use agentnotify_desktop::bridge::dto::{
    AdvanceOrcTaskPayload, CreateOrcTaskPayload, OrcMessageKindDto, OrcTaskIdPayload,
    OrcTaskStateDto, OrcTemplateStepConfigDto, SaveOrcTemplateConfigPayload,
};
use agentnotify_desktop::bridge::error::CommandError;
use agentnotify_desktop::production::agent_driver::{AgentDriver, DispatchOptions};
use agentnotify_desktop::production::orc_handler::OrcCommandHandler;
use agentnotify_desktop::production::settings::ProductionSettingsStore;
use agentnotify_domain::{AgentId, AgentSessionId};
use agentnotify_orchestration::{
    OrcStore, OrcTaskRepository, TemplateResolver, Workflow, WorkflowStep,
};
use agentnotify_storage_sqlite::SqliteStore;

/// 旧预设 id：静态测试沿用该工作流（任务级解析时沿用装配工作流）。
const PRESET_ID: &str = "preset-requirement-to-report";
/// 固定模板「快速修复」：动态模式（模板 + 节点配置）测试用。
const TEMPLATE_QUICKFIX: &str = "template-quickfix";

/// 记录型假 driver：记下每次派活（任务/Agent/会话/信封/open/选项），可配置失败。
struct FakeDriver {
    calls: Arc<RwLock<Vec<DispatchCall>>>,
    fail: Arc<RwLock<Option<String>>>,
}

/// 一次派活的完整观测（task/agent/session/envelope/open/options）。
#[derive(Clone, Debug, PartialEq)]
struct DispatchCall {
    task_id: String,
    agent_id: String,
    session_id: String,
    envelope: String,
    open: bool,
    options: DispatchOptions,
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
        options: &DispatchOptions,
    ) -> Result<(), CommandError> {
        self.calls.write().expect("测试锁").push(DispatchCall {
            task_id: task_id.to_string(),
            agent_id: agent_id.to_string(),
            session_id: session_id.to_string(),
            envelope: envelope.to_string(),
            open,
            options: options.clone(),
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

/// 动态装配（生产形态）+ 假 driver：模板/节点配置按内置模板解析。
fn dynamic_dispatched_handler(
    store: &Arc<SqliteStore>,
    config_dir: &std::path::Path,
    driver: Arc<FakeDriver>,
) -> OrcCommandHandler {
    OrcCommandHandler::with_selector(
        None,
        store.clone(),
        ProductionSettingsStore::new(store.clone(), config_dir),
        TemplateResolver::new(),
        None,
        Some(driver),
    )
}

/// 测试用工作目录：必须是已存在的目录。
fn working_dir(root: &tempfile::TempDir) -> String {
    root.path().to_string_lossy().into_owned()
}

async fn create_task(handler: &OrcCommandHandler, dir: &str) -> String {
    let created = handler
        .create(CreateOrcTaskPayload {
            name: None,
            steps: None,
            goal: "做一个贪吃蛇游戏".into(),
            template_id: PRESET_ID.into(),
            working_dir: dir.into(),
            notify_mode: None,
        })
        .await
        .expect("创建任务必须成功");
    created.id
}

/// 覆盖「快速修复」模板的节点配置（order/agent/model 逐项指定，覆盖全部步骤）。
async fn save_quickfix_config(
    handler: &OrcCommandHandler,
    steps_config: &[(u32, Option<&str>, Option<&str>)],
) {
    let steps = steps_config
        .iter()
        .map(|(order, agent, model)| OrcTemplateStepConfigDto {
            order: *order,
            agent: agent.map(str::to_string),
            model: model.map(str::to_string),
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

/// 清掉任务步骤快照（模拟升级前的旧任务，走实时合并路径）。
async fn clear_task_snapshot(store: &Arc<SqliteStore>, task_id: &str) {
    let repository: Arc<dyn OrcTaskRepository> = store.clone();
    let mut task = OrcStore::fetch_task(&repository, task_id)
        .await
        .expect("任务必须存在");
    task.set_steps_snapshot(&[]).expect("清除快照必须成功");
    repository
        .save_task(&task.a2a_task)
        .await
        .expect("保存任务必须成功");
}

/// start 派活第 1 步：agent_hint=codex、open=true（新会话）、会话 id 稳定、信封含目标与步数；
/// 创建任务本身不派活（先「待开始」，人工确认后 start）。
#[tokio::test]
async fn start_dispatches_first_step_with_open_session() {
    let (_root, store) = open_sqlite("agentnotify-orc-dispatch-create-");
    let driver = Arc::new(FakeDriver::new());
    let handler = dispatched_handler(&store, driver.clone());
    let dir = working_dir(&_root);

    let task_id = create_task(&handler, &dir).await;
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
    // §4 派活透传：工作目录取任务级；预置工作流没有模型；无人值守默认 true。
    assert_eq!(
        call.options.working_dir.as_deref(),
        Some(dir.as_str()),
        "派活必须透传任务工作目录"
    );
    assert_eq!(call.options.model, None, "未配置模型时不得凭空指定");
    assert!(call.options.unattended, "无人值守默认开启");
}

/// advance(报告) 后派活下一步：agent_hint=opencode、open=false（续聊）、同一 (task, step) 稳定会话。
#[tokio::test]
async fn advance_dispatches_next_step_with_resume() {
    let (_root, store) = open_sqlite("agentnotify-orc-dispatch-advance-");
    let driver = Arc::new(FakeDriver::new());
    let handler = dispatched_handler(&store, driver.clone());
    let dir = working_dir(&_root);

    let task_id = create_task(&handler, &dir).await;
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
    assert_eq!(
        advanced.options.working_dir.as_deref(),
        Some(dir.as_str()),
        "后续步同样透传工作目录"
    );
    assert!(
        advanced.envelope.contains("做一个贪吃蛇游戏"),
        "信封必须含目标"
    );
    assert!(advanced.envelope.contains("Step 2/3"), "信封必须含步数");
    assert!(advanced.envelope.contains("规划"), "信封必须含该步角色");
}

/// 节点配置的模型与无人值守随派活透传（§3/§4）：模型只对 OpenCode 生效，无人值守可显式关闭。
#[tokio::test]
async fn dispatch_options_carry_model_and_unattended_setting() {
    let (_root, store) = open_sqlite("agentnotify-orc-dispatch-options-");
    let driver = Arc::new(FakeDriver::new());
    let handler = dynamic_dispatched_handler(&store, _root.path(), driver.clone());

    // 全部节点配 opencode，第 1 步指定模型；显式关闭无人值守。
    let templates = handler
        .list_orc_templates()
        .await
        .expect("列出模板必须成功");
    let quickfix = templates
        .iter()
        .find(|template| template.id == TEMPLATE_QUICKFIX)
        .expect("快速修复模板必须存在");
    let steps = quickfix
        .steps
        .iter()
        .map(|step| OrcTemplateStepConfigDto {
            order: step.order,
            agent: Some("opencode".into()),
            model: if step.order == 1 {
                Some("anthropic/claude-sonnet-4-5".into())
            } else {
                None
            },
        })
        .collect();
    handler
        .save_orc_template_config(SaveOrcTemplateConfigPayload {
            template_id: TEMPLATE_QUICKFIX.into(),
            steps,
        })
        .await
        .expect("保存节点配置必须成功");
    store
        .write_settings_entries(std::collections::BTreeMap::from([(
            "orchestration.unattended".to_string(),
            serde_json::json!(false),
        )]))
        .await
        .expect("写入无人值守设置必须成功");

    let created = handler
        .create(CreateOrcTaskPayload {
            name: None,
            steps: None,
            goal: "透传选项".into(),
            template_id: TEMPLATE_QUICKFIX.into(),
            working_dir: working_dir(&_root),
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

    let calls = driver.calls();
    assert_eq!(calls.len(), 1, "开始执行必须派活一次");
    let options = &calls[0].options;
    assert_eq!(
        options.model.as_deref(),
        Some("anthropic/claude-sonnet-4-5"),
        "模型必须随派活透传"
    );
    assert!(!options.unattended, "无人值守设置必须随派活透传");
    assert_eq!(
        options.working_dir.as_deref(),
        Some(working_dir(&_root).as_str()),
        "工作目录必须随派活透传"
    );
}

/// driver 失败 → 自动 blocked：推进结果仍落库（返回推进态），但任务变 Failed、
/// 阻塞步骤与原因写清（含「插件未连接」），不自动重推。
#[tokio::test]
async fn dispatch_failure_blocks_task_with_clear_reason() {
    let (_root, store) = open_sqlite("agentnotify-orc-dispatch-fail-");
    let driver = Arc::new(FakeDriver::new());
    let handler = dispatched_handler(&store, driver.clone());

    // 先开始执行成功（Step 1 派活正常），再让后续派活失败。
    let task_id = create_task(&handler, &working_dir(&_root)).await;
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
        reason.contains("无法唤醒"),
        "必须写清是哪个 Agent 没唤醒成功：{reason}"
    );
    assert!(
        reason.contains("OpenCode 插件未连接"),
        "必须带上可执行的处理办法：{reason}"
    );
    assert!(
        !reason.contains(&task_id) && !reason.contains("Step"),
        "面向用户的原因不得包含任务 ID / 内部步骤英文：{reason}"
    );
}

/// start 时锁定节点配置（§3）：start 后清空/修改 node_config，后续派活仍用快照的 agent/model，
/// 且配置变化不再拦截已开始任务的推进。
#[tokio::test]
async fn start_snapshots_config_and_ignores_later_changes() {
    let (_root, store) = open_sqlite("agentnotify-orc-dispatch-snapshot-");
    let driver = Arc::new(FakeDriver::new());
    let handler = dynamic_dispatched_handler(&store, _root.path(), driver.clone());
    save_quickfix_config(
        &handler,
        &[
            (1, Some("opencode"), Some("anthropic/claude-sonnet-4-5")),
            (2, Some("codex"), None),
        ],
    )
    .await;

    let created = handler
        .create(CreateOrcTaskPayload {
            name: None,
            steps: None,
            goal: "快照任务".into(),
            template_id: TEMPLATE_QUICKFIX.into(),
            working_dir: working_dir(&_root),
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

    // start 后清空节点配置（模拟用户在设置页改配置）：快照必须仍然生效。
    handler
        .save_orc_template_config(SaveOrcTemplateConfigPayload {
            template_id: TEMPLATE_QUICKFIX.into(),
            steps: vec![
                OrcTemplateStepConfigDto {
                    order: 1,
                    agent: None,
                    model: None,
                },
                OrcTemplateStepConfigDto {
                    order: 2,
                    agent: None,
                    model: None,
                },
            ],
        })
        .await
        .expect("清空节点配置必须成功");

    handler
        .advance(AdvanceOrcTaskPayload {
            task_id: created.id.clone(),
            kind: OrcMessageKindDto::Report,
        })
        .await
        .expect("推进必须成功");

    let calls = driver.calls();
    assert_eq!(calls.len(), 2, "开始 + 推进各派活一次：{calls:?}");
    assert_eq!(
        calls[0].agent_id, "opencode",
        "第 1 步必须用 start 时快照的 Agent"
    );
    assert_eq!(
        calls[0].options.model.as_deref(),
        Some("anthropic/claude-sonnet-4-5"),
        "第 1 步必须用 start 时快照的模型"
    );
    assert_eq!(
        calls[1].agent_id, "codex",
        "第 2 步必须用快照 Agent，而不是被清空的实时配置"
    );
    assert_eq!(calls[1].options.model, None, "快照未配置模型 = 默认模型");

    let tasks = handler.list().await.expect("列出任务必须成功");
    let task = tasks.iter().find(|task| task.id == created.id).unwrap();
    assert_eq!(
        task.state,
        OrcTaskStateDto::Working,
        "配置清空不得阻塞已开始任务"
    );
    assert_eq!(task.current_step, 2);
    assert_eq!(
        task.workflow.steps[1].agent_hint.as_deref(),
        Some("codex"),
        "DTO 节点展示必须用快照"
    );
}

/// start 之前改配置仍生效：快照发生在 start 时刻，取当时的合并结果。
#[tokio::test]
async fn config_change_before_start_applies_to_snapshot() {
    let (_root, store) = open_sqlite("agentnotify-orc-dispatch-presnapshot-");
    let driver = Arc::new(FakeDriver::new());
    let handler = dynamic_dispatched_handler(&store, _root.path(), driver.clone());
    save_quickfix_config(
        &handler,
        &[(1, Some("codex"), None), (2, Some("codex"), None)],
    )
    .await;

    let created = handler
        .create(CreateOrcTaskPayload {
            name: None,
            steps: None,
            goal: "start 前改配置".into(),
            template_id: TEMPLATE_QUICKFIX.into(),
            working_dir: working_dir(&_root),
            notify_mode: None,
        })
        .await
        .expect("创建任务必须成功");

    // start 前改成 opencode + 模型：start 后的派活必须用新配置。
    save_quickfix_config(
        &handler,
        &[
            (1, Some("opencode"), Some("anthropic/claude-sonnet-4-5")),
            (2, Some("opencode"), None),
        ],
    )
    .await;
    handler
        .start(OrcTaskIdPayload {
            task_id: created.id.clone(),
        })
        .await
        .expect("开始执行必须成功");

    let calls = driver.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].agent_id, "opencode", "快照必须取 start 时刻的配置");
    assert_eq!(
        calls[0].options.model.as_deref(),
        Some("anthropic/claude-sonnet-4-5")
    );
}

/// 无快照（旧任务）继续走实时合并：清掉快照后运行期配置变化照旧生效。
#[tokio::test]
async fn task_without_snapshot_uses_live_node_config() {
    let (_root, store) = open_sqlite("agentnotify-orc-dispatch-live-");
    let driver = Arc::new(FakeDriver::new());
    let handler = dynamic_dispatched_handler(&store, _root.path(), driver.clone());
    save_quickfix_config(
        &handler,
        &[(1, Some("opencode"), None), (2, Some("codex"), None)],
    )
    .await;

    let created = handler
        .create(CreateOrcTaskPayload {
            name: None,
            steps: None,
            goal: "旧任务实时合并".into(),
            template_id: TEMPLATE_QUICKFIX.into(),
            working_dir: working_dir(&_root),
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

    // 模拟旧任务（升级前已开始）：清掉步骤快照。
    clear_task_snapshot(&store, &created.id).await;

    // 实时配置改为 opencode + 模型：无快照任务必须采用它。
    save_quickfix_config(
        &handler,
        &[
            (1, Some("opencode"), None),
            (2, Some("opencode"), Some("anthropic/claude-sonnet-4-5")),
        ],
    )
    .await;
    handler
        .advance(AdvanceOrcTaskPayload {
            task_id: created.id.clone(),
            kind: OrcMessageKindDto::Report,
        })
        .await
        .expect("推进必须成功");

    let calls = driver.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[1].agent_id, "opencode", "无快照任务必须实时合并");
    assert_eq!(
        calls[1].options.model.as_deref(),
        Some("anthropic/claude-sonnet-4-5")
    );
}

/// 无快照 + 实时配置也没 Agent：dispatch 侧兜底明确 blocked（防御路径，不猜测）。
#[tokio::test]
async fn task_without_snapshot_and_cleared_config_blocks_on_dispatch() {
    let (_root, store) = open_sqlite("agentnotify-orc-dispatch-live-missing-");
    let driver = Arc::new(FakeDriver::new());
    let handler = dynamic_dispatched_handler(&store, _root.path(), driver.clone());
    save_quickfix_config(
        &handler,
        &[(1, Some("opencode"), None), (2, Some("codex"), None)],
    )
    .await;

    let created = handler
        .create(CreateOrcTaskPayload {
            name: None,
            steps: None,
            goal: "无快照缺配置".into(),
            template_id: TEMPLATE_QUICKFIX.into(),
            working_dir: working_dir(&_root),
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
    clear_task_snapshot(&store, &created.id).await;
    save_quickfix_config(&handler, &[(1, None, None), (2, None, None)]).await;

    handler
        .advance(AdvanceOrcTaskPayload {
            task_id: created.id.clone(),
            kind: OrcMessageKindDto::Report,
        })
        .await
        .expect("推进必须成功");

    let tasks = handler.list().await.expect("列出任务必须成功");
    let task = tasks.iter().find(|task| task.id == created.id).unwrap();
    assert_eq!(task.state, OrcTaskStateDto::Failed);
    assert_eq!(task.blocked_step, Some(2));
    let reason = task.block_reason.as_deref().expect("必须有阻塞原因");
    assert!(reason.contains("未配置 Agent"), "{reason}");
    assert!(reason.contains("Step 2"), "{reason}");
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
            name: None,
            steps: None,
            goal: "不派活的目标".into(),
            template_id: PRESET_ID.into(),
            working_dir: working_dir(&_root),
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
    let dir = working_dir(&_root);

    let task_id = create_task(&handler, &dir).await;
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
            name: None,
            steps: None,
            goal: "信封衔接目标".into(),
            template_id: PRESET_ID.into(),
            working_dir: working_dir(&_root),
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

/// 自定义工作流（静态装配）：任务级解析沿用装配工作流，行为与静态模式一致。
#[tokio::test]
async fn static_custom_workflow_is_resolved_from_binding() {
    let (_root, store) = open_sqlite("agentnotify-orc-dispatch-custom-");
    let driver = Arc::new(FakeDriver::new());
    let workflow = Workflow::new(
        "custom-static",
        "自定义静态",
        vec![
            WorkflowStep::new(1, "orchestrator", Some("codex".to_string()), None, false),
            WorkflowStep::new(2, "executor", Some("opencode".to_string()), None, false),
        ],
    )
    .expect("自定义工作流必须有效");
    let handler = OrcCommandHandler::with_driver(
        Some(OrcStore::with_repository(workflow, store.clone())),
        TemplateResolver::new(),
        None,
        Some(driver.clone()),
    );

    let created = handler
        .create(CreateOrcTaskPayload {
            name: None,
            steps: None,
            goal: "自定义工作流目标".into(),
            template_id: "custom-static".into(),
            working_dir: working_dir(&_root),
            notify_mode: None,
        })
        .await
        .expect("创建任务必须成功");
    assert_eq!(created.workflow.id, "custom-static");
    handler
        .start(OrcTaskIdPayload {
            task_id: created.id.clone(),
        })
        .await
        .expect("开始执行必须成功");
    assert_eq!(driver.calls()[0].agent_id, "codex");
}
