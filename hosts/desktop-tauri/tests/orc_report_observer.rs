//! 编排「汇报自动回注」端到端测试（§4.4）：Agent 干完活上报 `session.completed`
//! 且 sessionId 为 `task-<id>-step-<n>` 时，自动推进任务（复用 advance，含下一步派活）。
//! 注入记录型假 driver 与假呈现；不经过真实 Agent 插件。
//!
//! 集群 v1 增补：项目经理汇总回流（最后一步 → 汇总信封 → 首节点产出 → 最终汇报）、
//! 汇总派活失败 blocked 与恢复自动重派、失败正文识别、编排会话通知过滤。

use std::sync::{Arc, Mutex, RwLock};

use agentnotify_agent_sdk::AgentEventEnvelope;
use agentnotify_desktop::bridge::dto::{CreateOrcTaskPayload, OrcTaskIdPayload, OrcTaskStateDto};
use agentnotify_desktop::bridge::error::CommandError;
use agentnotify_desktop::production::agent_driver::{AgentDriver, DispatchOptions};
use agentnotify_desktop::production::orc_event_filter::OrcEventFilter;
use agentnotify_desktop::production::orc_handler::OrcCommandHandler;
use agentnotify_desktop::production::orc_notify::OrcClusterPresenter;
use agentnotify_desktop::production::orc_report_observer::OrcReportObserver;
use agentnotify_domain::{AgentId, AgentSessionId, RequestId};
use agentnotify_orchestration::{NotifyMode, OrcStore, TemplateResolver, Workflow};
use agentnotify_runtime::{AgentEventFilter, AgentEventObserver};
use agentnotify_storage_sqlite::SqliteStore;

/// 旧预设 id：静态测试沿用该工作流（任务级解析时沿用装配工作流）。
const PRESET_ID: &str = "preset-requirement-to-report";

/// 一次派活的观测（task/agent/session/envelope/open/options），与 orchestration_dispatch.rs 同一写法。
#[derive(Clone, Debug, PartialEq)]
struct DispatchCall {
    task_id: String,
    agent_id: String,
    session_id: String,
    envelope: String,
    open: bool,
    options: DispatchOptions,
}

/// 记录型假 driver：记下每次派活，恒成功；可配置「第 N 次之后失败」。
struct FakeDriver {
    calls: Arc<RwLock<Vec<DispatchCall>>>,
    fail_after: Arc<RwLock<Option<usize>>>,
    fail_reason: Arc<RwLock<Option<String>>>,
}

impl FakeDriver {
    fn new() -> Self {
        Self {
            calls: Arc::new(RwLock::new(Vec::new())),
            fail_after: Arc::new(RwLock::new(None)),
            fail_reason: Arc::new(RwLock::new(None)),
        }
    }

    /// 第 `after` 次之后（严格大于）的派活全部失败，原因写进 blocked。
    fn fail_after(&self, after: usize, reason: &str) {
        *self.fail_after.write().expect("测试锁") = Some(after);
        *self.fail_reason.write().expect("测试锁") = Some(reason.to_string());
    }

    /// 恢复正常派活（故障排除后重新发起用）。
    fn clear_failure(&self) {
        *self.fail_after.write().expect("测试锁") = None;
        *self.fail_reason.write().expect("测试锁") = None;
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
        let call = DispatchCall {
            task_id: task_id.to_string(),
            agent_id: agent_id.to_string(),
            session_id: session_id.to_string(),
            envelope: envelope.to_string(),
            open,
            options: options.clone(),
        };
        let mut calls = self.calls.write().expect("测试锁");
        calls.push(call);
        let call_index = calls.len();
        let fail_after = *self.fail_after.read().expect("测试锁");
        if let Some(after) = fail_after {
            if call_index > after {
                let reason = self
                    .fail_reason
                    .read()
                    .expect("测试锁")
                    .clone()
                    .unwrap_or_else(|| "假 driver 失败".to_string());
                return Err(CommandError::new("fake_driver_failed", reason));
            }
        }
        Ok(())
    }
}

/// 推送记录：每次 push 的（task_id, 文本）。
type PushedRecords = Arc<Mutex<Vec<(String, String)>>>;

/// 假呈现：记录每次 push（task_id, 文本），默认节奏 final_only。
struct FakePresenter {
    pushed: PushedRecords,
}

impl FakePresenter {
    fn new() -> (Arc<Self>, PushedRecords) {
        let pushed: PushedRecords = Arc::new(Mutex::new(Vec::new()));
        (
            Arc::new(Self {
                pushed: pushed.clone(),
            }),
            pushed,
        )
    }
}

#[async_trait::async_trait]
impl OrcClusterPresenter for FakePresenter {
    async fn default_notify_mode(&self) -> NotifyMode {
        NotifyMode::FinalOnly
    }

    async fn push(&self, task_id: &str, text: String) {
        self.pushed
            .lock()
            .expect("假呈现锁不应失败")
            .push((task_id.to_owned(), text));
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

/// 测试用工作目录：必须是已存在的目录（创建任务时校验）。
fn working_dir(root: &tempfile::TempDir) -> String {
    root.path().to_string_lossy().into_owned()
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

/// 带假呈现的处理器（断言最终汇报推送）。
fn report_handler_with_presenter(
    store: &Arc<SqliteStore>,
    driver: Arc<FakeDriver>,
    presenter: Arc<FakePresenter>,
) -> Arc<OrcCommandHandler> {
    let workflow = Workflow::preset(false).expect("预置工作流必须有效");
    Arc::new(OrcCommandHandler::with_driver(
        Some(OrcStore::with_repository(workflow, store.clone())),
        TemplateResolver::new(),
        Some(presenter),
        Some(driver),
    ))
}

async fn create_task(handler: &OrcCommandHandler, dir: &str) -> String {
    handler
        .create(CreateOrcTaskPayload {
            goal: "做一个贪吃蛇游戏".into(),
            template_id: PRESET_ID.into(),
            working_dir: dir.into(),
            notify_mode: None,
        })
        .await
        .expect("创建任务必须成功")
        .id
}

fn completed_envelope(session_id: &str, body: &str) -> AgentEventEnvelope {
    AgentEventEnvelope {
        request_id: RequestId::new("req-orc-report-test").expect("有效请求标识"),
        agent_id: AgentId::new("opencode").expect("有效 Agent 标识"),
        payload: serde_json::json!({
            "eventType": "session.completed",
            "sessionId": session_id,
            "title": "【opencode】会话",
            "body": body,
        }),
    }
}

/// 失败终态事件（插件契约）：`eventType` 仍为 session.completed，但带显式 `failed: true` 标记。
fn failed_envelope(session_id: &str, body: &str) -> AgentEventEnvelope {
    AgentEventEnvelope {
        request_id: RequestId::new("req-orc-report-failed").expect("有效请求标识"),
        agent_id: AgentId::new("opencode").expect("有效 Agent 标识"),
        payload: serde_json::json!({
            "eventType": "session.completed",
            "sessionId": session_id,
            "title": "【opencode】会话（任务失败）",
            "body": body,
            "failed": true,
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
            template_id: PRESET_ID.into(),
            working_dir: working_dir(&_root),
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
        .observe(&completed_envelope(
            &format!("task-{}-step-1", created.id),
            "已完成第 1 步的判断，结论：可行。",
        ))
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
    assert_eq!(
        calls[1].agent_id, "opencode",
        "第 2 步建议 Agent 必须是 opencode"
    );
    assert_eq!(calls[1].session_id, format!("task-{}-step-2", created.id));
    assert!(!calls[1].open, "后续步必须续聊同一会话");
}

/// 项目经理回流（§4）：最后一步完成 → 汇总信封回首节点 → 首节点产出 = 最终汇报。
#[tokio::test]
async fn project_manager_flow_dispatches_summary_and_completes() {
    let (_root, store) = open_sqlite("agentnotify-orc-report-pm-");
    let driver = Arc::new(FakeDriver::new());
    let (presenter, pushed) = FakePresenter::new();
    let handler = report_handler_with_presenter(&store, driver.clone(), presenter);
    let observer = OrcReportObserver::new(handler.clone());

    let task_id = create_task(&handler, &working_dir(&_root)).await;
    handler
        .start(OrcTaskIdPayload {
            task_id: task_id.clone(),
        })
        .await
        .expect("开始执行必须成功");

    // 逐步上报：step 1 → 2 → 3（最后一步）→ 进入汇总并派活汇总信封。
    observer
        .observe(&completed_envelope(
            &format!("task-{task_id}-step-1"),
            "第一步判断：需求可行。",
        ))
        .await;
    observer
        .observe(&completed_envelope(
            &format!("task-{task_id}-step-2"),
            "第二步规划：拆成三个子任务。",
        ))
        .await;
    observer
        .observe(&completed_envelope(
            &format!("task-{task_id}-step-3"),
            "第三步实施：已完成并通过自测。",
        ))
        .await;

    let tasks = handler.list().await.expect("列出任务必须成功");
    let task = tasks
        .iter()
        .find(|t| t.id == task_id)
        .expect("任务必须存在");
    assert!(task.finalizing, "最后一步完成必须进入汇总阶段");
    assert_eq!(task.state, OrcTaskStateDto::Working, "汇总阶段仍为干活中");
    assert!(
        pushed.lock().unwrap().is_empty(),
        "final_only 汇总阶段不推（只推最终汇报）"
    );

    // 第 4 次派活 = 汇总信封（resume 首节点会话），带各步产出与项目经理指令。
    let calls = driver.calls();
    assert_eq!(calls.len(), 4, "三步派活 + 汇总派活：{calls:?}");
    let summary = &calls[3];
    assert_eq!(summary.session_id, format!("task-{task_id}-step-1"));
    assert!(!summary.open, "汇总必须 resume 首节点会话");
    assert!(
        summary.envelope.contains("项目经理"),
        "汇总信封必须含项目经理指令：{}",
        summary.envelope
    );
    assert!(
        summary.envelope.contains("需求可行")
            && summary.envelope.contains("拆成三个子任务")
            && summary.envelope.contains("已完成并通过自测"),
        "汇总信封必须带各步产出：{}",
        summary.envelope
    );

    // 首节点汇总产出到达 → 任务完成 + 最终汇报推送。
    observer
        .observe(&completed_envelope(
            &format!("task-{task_id}-step-1"),
            "最终汇报：贪吃蛇已完成，可直接使用。",
        ))
        .await;

    let tasks = handler.list().await.expect("列出任务必须成功");
    let task = tasks
        .iter()
        .find(|t| t.id == task_id)
        .expect("任务必须存在");
    assert_eq!(task.state, OrcTaskStateDto::Completed);
    assert!(!task.finalizing, "完成后必须清除汇总标记");

    let pushed = pushed.lock().unwrap();
    assert_eq!(pushed.len(), 1, "final_only 只推最终汇报：{pushed:?}");
    assert_eq!(pushed[0].0, task_id);
    assert!(
        pushed[0].1.contains("最终汇报：贪吃蛇已完成，可直接使用。"),
        "{}",
        pushed[0].1
    );
    assert!(
        pushed[0]
            .1
            .contains(&format!("【{task_id} · Step 3/3 · 已完成】")),
        "{}",
        pushed[0].1
    );
}

/// 汇总派活失败 → blocked（step 1，原因写清）+ 失败提醒；恢复后自动重派汇总信封。
#[tokio::test]
async fn summary_dispatch_failure_blocks_and_recover_redispatches() {
    let (_root, store) = open_sqlite("agentnotify-orc-report-pm-fail-");
    let driver = Arc::new(FakeDriver::new());
    let (presenter, pushed) = FakePresenter::new();
    let handler = report_handler_with_presenter(&store, driver.clone(), presenter);
    let observer = OrcReportObserver::new(handler.clone());

    let task_id = create_task(&handler, &working_dir(&_root)).await;
    handler
        .start(OrcTaskIdPayload {
            task_id: task_id.clone(),
        })
        .await
        .expect("开始执行必须成功");
    // 前 3 次派活正常；第 4 次（汇总）失败。
    driver.fail_after(3, "OpenCode 插件未连接，请启动 OpenCode 后重试");

    for step in 1..=3 {
        observer
            .observe(&completed_envelope(
                &format!("task-{task_id}-step-{step}"),
                &format!("第 {step} 步产出"),
            ))
            .await;
    }

    let tasks = handler.list().await.expect("列出任务必须成功");
    let task = tasks
        .iter()
        .find(|t| t.id == task_id)
        .expect("任务必须存在");
    assert_eq!(task.state, OrcTaskStateDto::Failed, "汇总派活失败必须阻塞");
    assert_eq!(task.blocked_step, Some(1));
    assert!(task.finalizing, "阻塞后仍应处于汇总阶段（恢复要重派汇总）");
    let reason = task.block_reason.as_deref().expect("必须有阻塞原因");
    assert!(
        reason.contains("汇总汇报派活失败"),
        "必须写清汇总派活失败：{reason}"
    );
    assert!(reason.contains("插件未连接"), "{reason}");

    // 失败提醒不受通知节奏限制：推一条失败提醒（final_only 也要推）。
    {
        let pushed = pushed.lock().unwrap();
        assert_eq!(pushed.len(), 1, "必须推失败提醒：{pushed:?}");
        assert!(pushed[0].1.contains("汇总汇报派活失败"), "{}", pushed[0].1);
    }

    // 恢复：故障排除后清阻塞并自动重派汇总信封（不再需要用户再点「发指令」）。
    driver.clear_failure();
    let recovered = handler
        .recover_blocked(OrcTaskIdPayload {
            task_id: task_id.clone(),
        })
        .await
        .expect("恢复必须成功");
    assert_eq!(recovered.state, OrcTaskStateDto::Working);
    assert!(recovered.finalizing);

    let calls = driver.calls();
    assert_eq!(calls.len(), 5, "恢复必须自动重派：{calls:?}");
    let resent = &calls[4];
    assert_eq!(resent.session_id, format!("task-{task_id}-step-1"));
    assert!(!resent.open, "重派汇总仍是 resume");
    assert!(
        resent.envelope.contains("项目经理"),
        "重派必须是汇总信封：{}",
        resent.envelope
    );

    // 恢复后任务回到干活中、不再阻塞。
    let tasks = handler.list().await.expect("列出任务必须成功");
    let task = tasks
        .iter()
        .find(|t| t.id == task_id)
        .expect("任务必须存在");
    assert_eq!(task.state, OrcTaskStateDto::Working);
    assert_eq!(task.blocked_step, None);

    // 首节点汇总产出到达 → 完成。
    observer
        .observe(&completed_envelope(
            &format!("task-{task_id}-step-1"),
            "最终汇报：已汇总。",
        ))
        .await;
    let tasks = handler.list().await.expect("列出任务必须成功");
    assert_eq!(
        tasks.iter().find(|t| t.id == task_id).unwrap().state,
        OrcTaskStateDto::Completed
    );
}

/// 普通步骤执行失败（显式 failed 标记）→ blocked 且**不推进**，失败提醒照发。
#[tokio::test]
async fn middle_step_failure_blocks_without_advance() {
    let (_root, store) = open_sqlite("agentnotify-orc-report-step-fail-");
    let driver = Arc::new(FakeDriver::new());
    let (presenter, pushed) = FakePresenter::new();
    let handler = report_handler_with_presenter(&store, driver.clone(), presenter);
    let observer = OrcReportObserver::new(handler.clone());

    let task_id = create_task(&handler, &working_dir(&_root)).await;
    handler
        .start(OrcTaskIdPayload {
            task_id: task_id.clone(),
        })
        .await
        .expect("开始执行必须成功");
    assert_eq!(driver.calls().len(), 1, "开始只派活 Step 1");

    observer
        .observe(&failed_envelope(
            &format!("task-{task_id}-step-1"),
            "任务执行失败：所选模型不可用",
        ))
        .await;

    let tasks = handler.list().await.expect("列出任务必须成功");
    let task = tasks
        .iter()
        .find(|t| t.id == task_id)
        .expect("任务必须存在");
    assert_eq!(task.state, OrcTaskStateDto::Failed, "失败必须阻塞");
    assert_eq!(task.blocked_step, Some(1));
    assert_eq!(task.current_step, 1, "失败不得推进步骤");
    let reason = task.block_reason.as_deref().expect("必须有阻塞原因");
    assert!(
        reason.contains("Step 1 执行失败"),
        "必须写清哪一步失败：{reason}"
    );
    assert!(reason.contains("所选模型不可用"), "{reason}");
    assert!(!task.finalizing, "普通步骤失败不得进入汇总阶段：{task:?}");

    assert_eq!(
        driver.calls().len(),
        1,
        "失败不得派活下一步：{:?}",
        driver.calls()
    );
    let pushed = pushed.lock().unwrap();
    assert_eq!(pushed.len(), 1, "必须推失败提醒：{pushed:?}");
    assert!(pushed[0].1.contains("Step 1 失败"), "{}", pushed[0].1);
    assert!(pushed[0].1.contains("所选模型不可用"), "{}", pushed[0].1);
}

/// 首节点汇总回合失败（显式 failed 标记）→ blocked 写清原因。
#[tokio::test]
async fn summary_round_failure_blocks_with_clear_reason() {
    let (_root, store) = open_sqlite("agentnotify-orc-report-pm-round-fail-");
    let driver = Arc::new(FakeDriver::new());
    let handler = report_handler(&store, driver.clone());
    let observer = OrcReportObserver::new(handler.clone());

    let task_id = create_task(&handler, &working_dir(&_root)).await;
    handler
        .start(OrcTaskIdPayload {
            task_id: task_id.clone(),
        })
        .await
        .expect("开始执行必须成功");
    for step in 1..=3 {
        observer
            .observe(&completed_envelope(
                &format!("task-{task_id}-step-{step}"),
                &format!("第 {step} 步产出"),
            ))
            .await;
    }

    observer
        .observe(&failed_envelope(
            &format!("task-{task_id}-step-1"),
            "任务执行失败：所选模型不可用",
        ))
        .await;

    let tasks = handler.list().await.expect("列出任务必须成功");
    let task = tasks
        .iter()
        .find(|t| t.id == task_id)
        .expect("任务必须存在");
    assert_eq!(task.state, OrcTaskStateDto::Failed);
    assert_eq!(task.blocked_step, Some(1));
    let reason = task.block_reason.as_deref().expect("必须有阻塞原因");
    assert!(
        reason.contains("首节点汇总回合失败"),
        "必须写清汇总回合失败：{reason}"
    );
    assert!(reason.contains("所选模型不可用"), "{reason}");
}

/// 旧插件兼容：失败终态无显式标记时，正文「任务执行失败：」仍识别为失败（不当作完成推进）。
#[tokio::test]
async fn legacy_failure_body_still_blocks() {
    let (_root, store) = open_sqlite("agentnotify-orc-report-legacy-fail-");
    let driver = Arc::new(FakeDriver::new());
    let handler = report_handler(&store, driver.clone());
    let observer = OrcReportObserver::new(handler.clone());

    let task_id = create_task(&handler, &working_dir(&_root)).await;
    handler
        .start(OrcTaskIdPayload {
            task_id: task_id.clone(),
        })
        .await
        .expect("开始执行必须成功");

    observer
        .observe(&completed_envelope(
            &format!("task-{task_id}-step-1"),
            "任务执行失败：旧插件未带标记",
        ))
        .await;

    let tasks = handler.list().await.expect("列出任务必须成功");
    let task = tasks
        .iter()
        .find(|t| t.id == task_id)
        .expect("任务必须存在");
    assert_eq!(task.state, OrcTaskStateDto::Failed);
    assert_eq!(task.blocked_step, Some(1));
    assert_eq!(task.current_step, 1, "失败不得推进步骤");
    let reason = task.block_reason.as_deref().expect("必须有阻塞原因");
    assert!(reason.contains("Step 1 执行失败"), "{reason}");
    assert!(reason.contains("旧插件未带标记"), "{reason}");
}

/// 非编排会话（普通 OpenCode 会话）完成 → 任务不受影响。
#[tokio::test]
async fn foreign_session_is_ignored() {
    let (_root, store) = open_sqlite("agentnotify-orc-report-foreign-");
    let driver = Arc::new(FakeDriver::new());
    let handler = report_handler(&store, driver.clone());
    let observer = OrcReportObserver::new(handler.clone());

    let task_id = create_task(&handler, &working_dir(&_root)).await;

    observer
        .observe(&completed_envelope(
            "ses_not_an_orc_session",
            "普通会话完成",
        ))
        .await;

    let tasks = handler.list().await.expect("列出任务必须成功");
    let task = tasks
        .iter()
        .find(|t| t.id == task_id)
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

    let task_id = create_task(&handler, &working_dir(&_root)).await;

    // 任务在 step 1，但事件来自 step 3 的会话（过期/错位）。
    observer
        .observe(&completed_envelope(
            &format!("task-{task_id}-step-3"),
            "过期产出",
        ))
        .await;

    let tasks = handler.list().await.expect("列出任务必须成功");
    let task = tasks
        .iter()
        .find(|t| t.id == task_id)
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

    let task_id = create_task(&handler, &working_dir(&_root)).await;

    let envelope = AgentEventEnvelope {
        request_id: RequestId::new("req-orc-report-failed").expect("有效请求标识"),
        agent_id: AgentId::new("opencode").expect("有效 Agent 标识"),
        payload: serde_json::json!({
            "eventType": "session.failed",
            "sessionId": format!("task-{task_id}-step-1"),
        }),
    };
    observer.observe(&envelope).await;

    let tasks = handler.list().await.expect("列出任务必须成功");
    let task = tasks
        .iter()
        .find(|t| t.id == task_id)
        .expect("任务必须存在");
    assert_eq!(task.current_step, 1, "非完成事件不得推进任务");
}

/// 通知过滤（§5）：编排会话抑制通知，普通会话与同形未知任务放行。
#[tokio::test]
async fn event_filter_suppresses_orchestration_sessions() {
    let (_root, store) = open_sqlite("agentnotify-orc-report-filter-");
    let driver = Arc::new(FakeDriver::new());
    let handler = report_handler(&store, driver.clone());
    let task_id = create_task(&handler, &working_dir(&_root)).await;

    let filter = OrcEventFilter::new(_root.path().join("state.db"));
    let orc_event = completed_envelope(&format!("task-{task_id}-step-2"), "步骤产出");
    assert!(
        !filter.allow_notification(&orc_event),
        "已知编排会话必须抑制通知"
    );
    let foreign = completed_envelope("ses_regular_session", "普通会话完成");
    assert!(filter.allow_notification(&foreign), "普通会话必须放行");
    let unknown_task = completed_envelope(
        "task-00000000-0000-4000-8000-000000000000-step-1",
        "未知任务",
    );
    assert!(
        filter.allow_notification(&unknown_task),
        "任务表里不存在的同形会话必须放行"
    );
}
