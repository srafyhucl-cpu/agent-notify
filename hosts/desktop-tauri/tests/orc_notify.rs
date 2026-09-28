//! P1-4 通知节奏开关端到端测试：全局默认 `orchestration.notify_mode` 继承、
//! `advance`/`mark_blocked` 后按规则外发集群消息（注入假呈现，断言调用次数与消息文本）、
//! 失败提醒不受 notify_mode 限制、真实 settings 缺失/非法回退 final_only。
//!
//! 覆盖（§4.6 / P1-4 验收）：
//! - final_only：中间 Step 推进不推（仅落库），最终汇报/人工确认门才推；
//! - verbose：每个推进类转移都推（带前后缀）；
//! - 失败提醒一律推（写清失败 Step / 原因 / 需人工处理，不自动重推）；
//! - 任务不存在：报错且不产生推送；恢复（recover_blocked）不额外推送（避免与 P1-3 微信回执重复）；
//! - 任务创建：未显式指定继承全局默认，显式指定覆盖（现有语义保持）；
//! - settings 默认读取/非法回退：真实 ProductionSettingsStore + ProductionOrcPresenter。

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use agentnotify_agent_sdk::AgentRegistry;
use agentnotify_channel_sdk::ChannelRegistry;
use agentnotify_desktop::bridge::dto::{
    AdvanceOrcTaskPayload, CreateOrcTaskPayload, MarkBlockedOrcTaskPayload, OrcMessageKindDto,
    OrcTaskIdPayload, OrcTaskStateDto,
};
use agentnotify_desktop::production::service::{KEY_ORCHESTRATION_NOTIFY_MODE, OrcCommandHandler};
use agentnotify_desktop::production::{
    OrcClusterPresenter, ProductionOrcPresenter, ProductionSettingsStore, ProductionTargetProvider,
};
use agentnotify_orchestration::{NotifyMode, OrcStore, TemplateResolver, Workflow};
use agentnotify_storage_sqlite::SqliteStore;
use agentnotify_testkit::MemoryStore;

/// 推送记录：每次 push 的（task_id, 文本）。
type PushedRecords = Arc<Mutex<Vec<(String, String)>>>;

/// 假呈现实现：记录每次 push（task_id, 文本），default_notify_mode 返回注入的默认节奏。
struct FakePresenter {
    mode: NotifyMode,
    pushed: PushedRecords,
}

impl FakePresenter {
    fn new(mode: NotifyMode) -> (Arc<Self>, PushedRecords) {
        let pushed = Arc::new(Mutex::new(Vec::new()));
        (
            Arc::new(Self {
                mode,
                pushed: pushed.clone(),
            }),
            pushed,
        )
    }
}

#[async_trait::async_trait]
impl OrcClusterPresenter for FakePresenter {
    async fn default_notify_mode(&self) -> NotifyMode {
        self.mode
    }

    async fn push(&self, task_id: &str, text: String) {
        self.pushed
            .lock()
            .expect("假呈现锁不应失败")
            .push((task_id.to_owned(), text));
    }
}

/// 打开临时目录下的真实 SQLite 文件（落在 testkit 隔离根，避免 C 盘）。
fn open_sqlite(prefix: &str) -> (tempfile::TempDir, Arc<SqliteStore>) {
    let root = tempfile::Builder::new()
        .prefix(prefix)
        .tempdir_in(agentnotify_testkit::test_temp_root())
        .expect("测试临时目录必须可创建");
    let store =
        Arc::new(SqliteStore::open(root.path().join("state.db")).expect("SQLite 数据库必须可创建"));
    (root, store)
}

/// 装配编排处理器 + 假呈现；返回 (处理器, 推送记录, 任务创建用句柄可自行使用)。
fn handler_with(
    store: &Arc<SqliteStore>,
    workflow: Workflow,
    presenter: Arc<FakePresenter>,
) -> OrcCommandHandler {
    OrcCommandHandler::with_presenter(
        Some(OrcStore::with_repository(workflow, store.clone())),
        TemplateResolver::new(),
        presenter,
    )
}

fn report(task_id: &str) -> AdvanceOrcTaskPayload {
    AdvanceOrcTaskPayload {
        task_id: task_id.to_owned(),
        kind: OrcMessageKindDto::Report,
    }
}

/// 写入全局默认通知节奏设置（orchestration.notify_mode）。
async fn write_mode(store: &Arc<SqliteStore>, value: &str) {
    let mut entries = BTreeMap::new();
    entries.insert(
        KEY_ORCHESTRATION_NOTIFY_MODE.to_owned(),
        serde_json::json!(value),
    );
    store
        .write_settings_entries(entries)
        .await
        .expect("写设置必须成功");
}

/// final_only：中间 Step 推进不推（仅落库），只有最顶层最终汇报才推。
#[tokio::test]
async fn final_only_pushes_only_the_final_report() {
    let (_root, store) = open_sqlite("agentnotify-orc-notify-final-");
    let (presenter, pushed) = FakePresenter::new(NotifyMode::FinalOnly);
    let handler = handler_with(&store, Workflow::preset(false).unwrap(), presenter);
    let created = handler
        .create(CreateOrcTaskPayload {
            goal: "做一个贪吃蛇游戏".into(),
            notify_mode: Some("final_only".into()),
        })
        .await
        .expect("创建任务必须成功");
    let task_id = created.id.clone();

    // 人工确认开始（「待开始」任务不接受推进；本测试关注呈现，未注入 driver 不派活）。
    handler
        .start(OrcTaskIdPayload {
            task_id: task_id.clone(),
        })
        .await
        .expect("开始执行必须成功");

    // 第 1/2 步推进：final_only 不推（仅落库）
    handler
        .advance(report(&task_id))
        .await
        .expect("推进必须成功");
    handler
        .advance(report(&task_id))
        .await
        .expect("推进必须成功");
    assert!(
        pushed.lock().unwrap().is_empty(),
        "final_only 中间 Step 必须不推：{:?}",
        pushed.lock().unwrap()
    );

    // 第 3 步（最后一步）汇报 → 任务完成，推最终汇报
    handler
        .advance(report(&task_id))
        .await
        .expect("推进必须成功");
    let records = pushed.lock().unwrap();
    assert_eq!(records.len(), 1, "final_only 全程只推一条：{:?}", records);
    let (pushed_task, text) = &records[0];
    assert_eq!(pushed_task, &task_id);
    assert!(
        text.contains(&format!("【集群 {task_id}】")),
        "缺少前缀：{text}"
    );
    assert!(
        text.contains("最终汇报已收到，任务已完成"),
        "缺少正文：{text}"
    );
    assert!(
        text.contains(&format!("【{task_id} · Step 3/3 · 已完成】")),
        "缺少后缀：{text}"
    );
}

/// verbose：每个 Step 的推进/汇报都推（带前后缀，Step 数随推进变化）。
#[tokio::test]
async fn verbose_pushes_every_step_progress() {
    let (_root, store) = open_sqlite("agentnotify-orc-notify-verbose-");
    let (presenter, pushed) = FakePresenter::new(NotifyMode::FinalOnly); // 任务级 verbose 覆盖，与全局默认无关
    let handler = handler_with(&store, Workflow::preset(false).unwrap(), presenter);
    let created = handler
        .create(CreateOrcTaskPayload {
            goal: "做一个贪吃蛇游戏".into(),
            notify_mode: Some("verbose".into()),
        })
        .await
        .expect("创建任务必须成功");
    let task_id = created.id.clone();

    // 人工确认开始（「待开始」任务不接受推进；本测试关注呈现，未注入 driver 不派活）。
    handler
        .start(OrcTaskIdPayload {
            task_id: task_id.clone(),
        })
        .await
        .expect("开始执行必须成功");

    for _ in 0..3 {
        handler
            .advance(report(&task_id))
            .await
            .expect("推进必须成功");
    }

    let records = pushed.lock().unwrap();
    assert_eq!(records.len(), 3, "verbose 每步都推：{:?}", records);
    assert_eq!(records[0].0, task_id);
    assert!(
        records[0].1.contains("Step 1 汇报完成，任务推进到下一步"),
        "{}",
        records[0].1
    );
    assert!(
        records[0]
            .1
            .contains(&format!("【{task_id} · Step 2/3 · 干活中】")),
        "{}",
        records[0].1
    );
    assert!(
        records[1].1.contains("Step 2 汇报完成，任务推进到下一步"),
        "{}",
        records[1].1
    );
    assert!(
        records[1]
            .1
            .contains(&format!("【{task_id} · Step 3/3 · 干活中】")),
        "{}",
        records[1].1
    );
    assert!(
        records[2].1.contains("最终汇报已收到，任务已完成"),
        "{}",
        records[2].1
    );
    assert!(
        records[2]
            .1
            .contains(&format!("【{task_id} · Step 3/3 · 已完成】")),
        "{}",
        records[2].1
    );
}

/// human_gate：final_only 下人工确认门（等待确认）与确认通过都推；中间步仍不推。
#[tokio::test]
async fn final_only_pushes_gate_wait_and_confirm() {
    let (_root, store) = open_sqlite("agentnotify-orc-notify-gate-");
    let (presenter, pushed) = FakePresenter::new(NotifyMode::FinalOnly);
    // 预置工作流含第 4 步「复核/汇总」（human_gate=true）
    let handler = handler_with(&store, Workflow::preset(true).unwrap(), presenter);
    let created = handler
        .create(CreateOrcTaskPayload {
            goal: "带人工确认的任务".into(),
            notify_mode: Some("final_only".into()),
        })
        .await
        .expect("创建任务必须成功");
    let task_id = created.id.clone();

    // 人工确认开始（「待开始」任务不接受推进；本测试关注呈现，未注入 driver 不派活）。
    handler
        .start(OrcTaskIdPayload {
            task_id: task_id.clone(),
        })
        .await
        .expect("开始执行必须成功");

    // 前 3 步无确认门：final_only 不推
    for _ in 0..3 {
        handler
            .advance(report(&task_id))
            .await
            .expect("推进必须成功");
    }
    assert!(pushed.lock().unwrap().is_empty(), "确认门之前的步绝不推");

    // 第 4 步汇报 → 等人确认（InputRequired）：推「等待你的确认」
    let waiting = handler
        .advance(report(&task_id))
        .await
        .expect("推进必须成功");
    assert_eq!(waiting.state, OrcTaskStateDto::InputRequired);
    {
        let records = pushed.lock().unwrap();
        assert_eq!(records.len(), 1, "确认门到达必须推：{:?}", records);
        assert!(
            records[0].1.contains("Step 4 汇报已收到，等待你的确认"),
            "{}",
            records[0].1
        );
        assert!(
            records[0]
                .1
                .contains(&format!("【{task_id} · Step 4/4 · 等待确认】")),
            "{}",
            records[0].1
        );
    }

    // 人确认 → 任务完成：推「确认通过」
    handler
        .advance(AdvanceOrcTaskPayload {
            task_id: task_id.clone(),
            kind: OrcMessageKindDto::Confirm,
        })
        .await
        .expect("确认必须成功");
    let records = pushed.lock().unwrap();
    assert_eq!(records.len(), 2, "确认通过必须推：{:?}", records);
    assert!(
        records[1].1.contains("确认通过，任务已完成"),
        "{}",
        records[1].1
    );
    assert!(
        records[1]
            .1
            .contains(&format!("【{task_id} · Step 4/4 · 已完成】")),
        "{}",
        records[1].1
    );
}

/// 失败提醒不受 notify_mode 限制：final_only 与 verbose 都一律推（写清 Step/原因/不重推）。
#[tokio::test]
async fn failure_reminder_always_pushes_regardless_of_mode() {
    let reason = "opencode 会话不可用（未登录），消息未送达";
    for mode in ["final_only", "verbose"] {
        let (_root, store) = open_sqlite(&format!("agentnotify-orc-notify-fail-{mode}-"));
        let (presenter, pushed) = FakePresenter::new(NotifyMode::FinalOnly);
        let handler = handler_with(&store, Workflow::preset(false).unwrap(), presenter);
        let created = handler
            .create(CreateOrcTaskPayload {
                goal: "失败提醒测试".into(),
                notify_mode: Some(mode.into()),
            })
            .await
            .expect("创建任务必须成功");
        let task_id = created.id.clone();

        handler
            .mark_blocked(MarkBlockedOrcTaskPayload {
                task_id: task_id.clone(),
                step: 2,
                reason: reason.into(),
            })
            .await
            .expect("标记阻塞必须成功");

        let records = pushed.lock().unwrap();
        assert_eq!(records.len(), 1, "{mode} 失败提醒必须推：{:?}", records);
        let (pushed_task, text) = &records[0];
        assert_eq!(pushed_task, &task_id);
        assert!(
            text.contains(&format!("【集群 {task_id}】")),
            "缺少前缀：{text}"
        );
        assert!(text.contains("Step 2 失败"), "{text}");
        assert!(text.contains(reason), "必须写清失败原因：{text}");
        assert!(text.contains("需人工处理"), "{text}");
        assert!(text.contains("不会自动重推"), "{text}");
        assert!(
            text.contains(&format!("【{task_id} · Step 2/3 · 已阻塞】")),
            "缺少后缀：{text}"
        );
    }
}

/// 推进不存在的任务：明确报错且不产生任何推送。
#[tokio::test]
async fn advance_missing_task_errors_without_push() {
    let (_root, store) = open_sqlite("agentnotify-orc-notify-missing-");
    let (presenter, pushed) = FakePresenter::new(NotifyMode::FinalOnly);
    let handler = handler_with(&store, Workflow::preset(false).unwrap(), presenter);

    let err = handler
        .advance(report("no-such-task"))
        .await
        .expect_err("推进不存在的任务必须报错");
    assert_eq!(err.code, "orc.task_not_found");
    assert!(pushed.lock().unwrap().is_empty(), "任务不存在绝不推送");
}

/// 恢复（recover_blocked）不额外推送：失败提醒已推过，恢复由 P1-3 微信回执/桌面端反馈覆盖。
#[tokio::test]
async fn recover_blocked_does_not_add_push() {
    let (_root, store) = open_sqlite("agentnotify-orc-notify-recover-");
    let (presenter, pushed) = FakePresenter::new(NotifyMode::FinalOnly);
    let handler = handler_with(&store, Workflow::preset(false).unwrap(), presenter);
    let created = handler
        .create(CreateOrcTaskPayload {
            goal: "恢复不重复推".into(),
            notify_mode: Some("final_only".into()),
        })
        .await
        .expect("创建任务必须成功");
    let task_id = created.id.clone();

    handler
        .mark_blocked(MarkBlockedOrcTaskPayload {
            task_id: task_id.clone(),
            step: 1,
            reason: "codex 会话不可用".into(),
        })
        .await
        .expect("标记阻塞必须成功");
    handler
        .recover_blocked(OrcTaskIdPayload {
            task_id: task_id.clone(),
        })
        .await
        .expect("恢复必须成功");

    let records = pushed.lock().unwrap();
    assert_eq!(records.len(), 1, "恢复不额外推送：{:?}", records);
    assert!(records[0].1.contains("已阻塞"), "{}", records[0].1);
}

/// 任务创建继承：未显式指定 → 全局默认（presenter 提供）；显式指定 → 任务级覆盖；非法值报错。
#[tokio::test]
async fn create_inherits_global_default_and_explicit_overrides() {
    let (_root, store) = open_sqlite("agentnotify-orc-notify-inherit-");
    let (presenter, _pushed) = FakePresenter::new(NotifyMode::Verbose);
    let handler = handler_with(&store, Workflow::preset(false).unwrap(), presenter);

    // 未显式指定 → 继承全局默认 verbose
    let inherited = handler
        .create(CreateOrcTaskPayload {
            goal: "继承全局默认".into(),
            notify_mode: None,
        })
        .await
        .expect("创建任务必须成功");
    assert_eq!(inherited.notify_mode, "verbose");

    // 显式指定 → 任务级覆盖（全局默认 verbose 被覆盖为 final_only，现有语义保持）
    let overridden = handler
        .create(CreateOrcTaskPayload {
            goal: "显式覆盖".into(),
            notify_mode: Some("final_only".into()),
        })
        .await
        .expect("创建任务必须成功");
    assert_eq!(overridden.notify_mode, "final_only");

    // 显式非法值 → 明确报错（不猜测兜底）
    let err = handler
        .create(CreateOrcTaskPayload {
            goal: "非法节奏".into(),
            notify_mode: Some("noisy".into()),
        })
        .await
        .expect_err("非法通知节奏必须报错");
    assert_eq!(err.code, "orc_notify_mode_invalid");
}

/// 真实 settings 链：`orchestration.notify_mode` 缺失/非法回退 final_only 并告警，
/// verbose 生效；任务创建继承真实全局默认。
#[tokio::test]
async fn global_default_reads_real_settings_and_falls_back() {
    let (_root, store) = open_sqlite("agentnotify-orc-notify-settings-");
    let settings = ProductionSettingsStore::new(store.clone(), _root.path());
    let targets = Arc::new(ProductionTargetProvider::new(
        store.clone(),
        settings.clone(),
        Arc::new(MemoryStore::default()),
        Arc::new(AgentRegistry::default()),
        Arc::new(ChannelRegistry::default()),
    ));
    let presenter: Arc<dyn OrcClusterPresenter> = Arc::new(ProductionOrcPresenter::new(
        settings.clone(),
        targets,
        Arc::new(ChannelRegistry::default()),
        None,
    ));

    // 缺失（未配置）→ final_only
    assert_eq!(presenter.default_notify_mode().await, NotifyMode::FinalOnly);

    // verbose → 生效
    write_mode(&store, "verbose").await;
    assert_eq!(presenter.default_notify_mode().await, NotifyMode::Verbose);

    // 非法取值 → 回退 final_only（不猜测）
    write_mode(&store, "noisy").await;
    assert_eq!(presenter.default_notify_mode().await, NotifyMode::FinalOnly);

    // 显式 final_only → 保持
    write_mode(&store, "final_only").await;
    assert_eq!(presenter.default_notify_mode().await, NotifyMode::FinalOnly);

    // 端到端继承：真实 presenter 装配进 handler，任务创建不带 notify_mode 时继承真实设置
    let handler = OrcCommandHandler::with_presenter(
        Some(OrcStore::with_repository(
            Workflow::preset(false).unwrap(),
            store.clone(),
        )),
        TemplateResolver::new(),
        presenter.clone(),
    );
    write_mode(&store, "verbose").await;
    let created = handler
        .create(CreateOrcTaskPayload {
            goal: "settings 继承".into(),
            notify_mode: None,
        })
        .await
        .expect("创建任务必须成功");
    assert_eq!(created.notify_mode, "verbose");

    write_mode(&store, "garbage").await;
    let created = handler
        .create(CreateOrcTaskPayload {
            goal: "settings 回退".into(),
            notify_mode: None,
        })
        .await
        .expect("创建任务必须成功");
    assert_eq!(created.notify_mode, "final_only");
}
