//! 编排命令端到端测试（P1-1）：SQLite 真实文件上的 create→advance→mark_blocked→recover，
//! 以及默认关闭（enabled=false）时的明确报错。不经过完整宿主装配，命令层逻辑与
//! `ProductionHostCommandService` 中同一 [`OrcCommandHandler`] 实现。
//!
//! 集群 v1 增补：创建必填模板/工作目录校验、模板列表与节点配置保存（含校验）、
//! start 预检（缺 Agent → 明确报错且保持待开始）。

use std::sync::Arc;

use agentnotify_desktop::bridge::dto::{
    AdvanceOrcTaskPayload, CreateOrcTaskPayload, MarkBlockedOrcTaskPayload, OrcMessageKindDto,
    OrcTaskIdPayload, OrcTaskStateDto, OrcTemplateDto, OrcTemplateStepConfigDto,
    SaveOrcTemplateConfigPayload, UpdateOrcTaskStepPayload,
};
use agentnotify_desktop::production::orc_handler::{OrcCommandHandler, load_harness_templates};
use agentnotify_desktop::production::settings::ProductionSettingsStore;
use agentnotify_orchestration::{OrcStore, TemplateResolver, Workflow};
use agentnotify_storage_sqlite::SqliteStore;

/// 固定模板 id（新任务只开放内置三档模板）。
const TEMPLATE_STANDARD: &str = "template-standard";
const TEMPLATE_QUICKFIX: &str = "template-quickfix";
/// 旧预设 id：仅兼容已存在任务（静态测试沿用该工作流）。
const PRESET_ID: &str = "preset-requirement-to-report";

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

/// 启用编排的处理器：绑定预置工作流 + SQLite 仓储（与 `orchestration_store` 装配方向一致）。
fn enabled_handler(store: &Arc<SqliteStore>) -> OrcCommandHandler {
    let workflow = Workflow::preset(false).expect("预置工作流必须有效");
    OrcCommandHandler::new(Some(OrcStore::with_repository(workflow, store.clone())))
}

/// 动态装配（生产形态）：按 settings 实时解析开关；模板/节点配置按内置模板解析。
fn dynamic_handler(store: &Arc<SqliteStore>, config_dir: &std::path::Path) -> OrcCommandHandler {
    OrcCommandHandler::with_selector(
        None,
        store.clone(),
        ProductionSettingsStore::new(store.clone(), config_dir),
        TemplateResolver::new(),
        None,
        None,
    )
}

/// 测试用工作目录：必须是已存在的目录（创建/开始都会校验）。
fn working_dir(root: &tempfile::TempDir) -> String {
    root.path().to_string_lossy().into_owned()
}

fn create_payload(goal: &str, template_id: &str, dir: &str) -> CreateOrcTaskPayload {
    CreateOrcTaskPayload {
        name: None,
        steps: None,
        goal: goal.into(),
        template_id: template_id.into(),
        working_dir: dir.into(),
        notify_mode: None,
    }
}

/// 把所有节点都配置成同一个 Agent（动态模式的模板节点配置）。
async fn configure_all_steps(handler: &OrcCommandHandler, template_id: &str, agent: &str) {
    let templates = handler
        .list_orc_templates()
        .await
        .expect("列出模板必须成功");
    let template = templates
        .iter()
        .find(|template| template.id == template_id)
        .expect("模板必须存在");
    let steps = template
        .steps
        .iter()
        .map(|step| OrcTemplateStepConfigDto {
            order: step.order,
            agent: Some(agent.into()),
            model: None,
        })
        .collect();
    handler
        .save_orc_template_config(SaveOrcTemplateConfigPayload {
            template_id: template_id.into(),
            steps,
        })
        .await
        .expect("保存节点配置必须成功");
}

/// create → advance → mark_blocked → recover 全链路（SQLite 真实文件）。
#[tokio::test]
async fn orc_commands_full_chain_on_sqlite() {
    let (_root, store) = open_sqlite("agentnotify-orc-chain-");
    let handler = enabled_handler(&store);
    let dir = working_dir(&_root);

    // 1. create：Working / 第 1 步 / final_only 默认
    let created = handler
        .create(create_payload("做一个贪吃蛇游戏", PRESET_ID, &dir))
        .await
        .expect("创建任务必须成功");
    assert_eq!(created.state, OrcTaskStateDto::Working);
    assert_eq!(created.current_step, 1);
    assert_eq!(created.notify_mode, "final_only");
    assert_eq!(created.goal, "做一个贪吃蛇游戏");
    assert_eq!(created.working_dir.as_deref(), Some(dir.as_str()));
    assert!(!created.finalizing);
    assert!(!created.workflow_id.is_empty());
    let task_id = created.id.clone();
    assert!(!created.started, "新建任务必须是「待开始」");

    // 2. start（人工确认开始）→ 派活第 1 步（无 driver 时仅标记已开始）
    let started = handler
        .start(OrcTaskIdPayload {
            task_id: task_id.clone(),
        })
        .await
        .expect("开始执行必须成功");
    assert!(started.started, "开始后必须标记为已开始");

    // 3. advance（汇报）→ 第 2 步
    let advanced = handler
        .advance(AdvanceOrcTaskPayload {
            task_id: task_id.clone(),
            kind: OrcMessageKindDto::Report,
        })
        .await
        .expect("推进任务必须成功");
    assert_eq!(advanced.state, OrcTaskStateDto::Working);
    assert_eq!(advanced.current_step, 2);

    // 3. mark_blocked：Failed / 阻塞步骤与原因落库（写清哪步失败、谁不可用、未送达）
    let blocked = handler
        .mark_blocked(MarkBlockedOrcTaskPayload {
            task_id: task_id.clone(),
            step: 2,
            reason: "opencode 会话不可用（未登录），消息未送达".into(),
        })
        .await
        .expect("标记阻塞必须成功");
    assert_eq!(blocked.state, OrcTaskStateDto::Failed);
    assert_eq!(blocked.blocked_step, Some(2));
    assert_eq!(
        blocked.block_reason.as_deref(),
        Some("opencode 会话不可用（未登录），消息未送达")
    );

    // 4. recover_blocked：回到干活中、阻塞标记清除
    let recovered = handler
        .recover_blocked(OrcTaskIdPayload { task_id })
        .await
        .expect("恢复任务必须成功");
    assert_eq!(recovered.state, OrcTaskStateDto::Working);
    assert_eq!(recovered.blocked_step, None);
    assert_eq!(recovered.block_reason, None);
    assert_eq!(recovered.current_step, 2, "恢复后回到原步骤继续干活");
}

/// 全链路落盘后重开数据库：任务仍在且状态一致（SQLite 持久化，B 方案核心）。
#[tokio::test]
async fn orc_commands_persist_across_reopen() {
    let root = tempfile::Builder::new()
        .prefix("agentnotify-orc-persist-")
        .tempdir_in(agentnotify_testkit::test_temp_root())
        .expect("测试临时目录必须可创建");
    let db_path = root.path().join("state.db");
    let dir = root.path().to_string_lossy().into_owned();
    let task_id;

    {
        let store = Arc::new(SqliteStore::open(&db_path).expect("SQLite 必须可创建"));
        let handler = enabled_handler(&store);
        let created = handler
            .create(CreateOrcTaskPayload {
                name: None,
                steps: None,
                goal: "持久化验证".into(),
                template_id: PRESET_ID.into(),
                working_dir: dir.clone(),
                notify_mode: Some("verbose".into()),
            })
            .await
            .expect("创建任务必须成功");
        task_id = created.id;
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
    }

    // 重开数据库（模拟应用重启）：任务可读、推进状态保留。
    {
        let store = Arc::new(SqliteStore::open(&db_path).expect("重开 SQLite 必须成功"));
        let handler = enabled_handler(&store);
        let tasks = handler.list().await.expect("列出任务必须成功");
        assert_eq!(tasks.len(), 1, "重开后任务必须仍在");
        assert_eq!(tasks[0].id, task_id);
        assert_eq!(tasks[0].state, OrcTaskStateDto::Working);
        assert_eq!(tasks[0].current_step, 2, "推进结果必须持久化");
        assert_eq!(tasks[0].notify_mode, "verbose");
        assert_eq!(
            tasks[0].working_dir.as_deref(),
            Some(dir.as_str()),
            "工作目录必须持久化"
        );
    }
}

/// 新流程（v3）：创建时提交任务级节点配置（`payload.steps`）→ **创建即锁定快照**；
/// 之后改设置页默认节点配置不影响该任务；锁定快照齐全时可直接开始执行。
#[tokio::test]
async fn create_with_task_steps_locks_snapshot() {
    let (_root, store) = open_sqlite("agentnotify-orc-create-steps-");
    let handler = dynamic_handler(&store, _root.path());
    let dir = working_dir(&_root);

    let created = handler
        .create(CreateOrcTaskPayload {
            name: None,
            steps: Some(vec![
                OrcTemplateStepConfigDto {
                    order: 1,
                    agent: Some("opencode".into()),
                    model: Some("anthropic/claude-sonnet-4-5".into()),
                },
                OrcTemplateStepConfigDto {
                    order: 2,
                    agent: Some("codex".into()),
                    model: None,
                },
                OrcTemplateStepConfigDto {
                    order: 3,
                    agent: Some("commandcode".into()),
                    model: None,
                },
            ]),
            goal: "创建即锁定".into(),
            template_id: TEMPLATE_STANDARD.into(),
            working_dir: dir.clone(),
            notify_mode: Some("final_only".into()),
        })
        .await
        .expect("创建必须成功");
    assert!(!created.started, "新建任务必须是「待开始」");
    let agents: Vec<Option<String>> = created
        .workflow
        .steps
        .iter()
        .map(|step| step.agent_hint.clone())
        .collect();
    assert_eq!(
        agents,
        vec![
            Some("opencode".into()),
            Some("codex".into()),
            Some("commandcode".into())
        ],
        "创建返回的节点必须是提交的任务级配置"
    );
    assert_eq!(
        created.workflow.steps[0].model.as_deref(),
        Some("anthropic/claude-sonnet-4-5")
    );

    // 改设置页默认配置 → 已创建任务仍按自己的快照展示（不被覆盖）。
    configure_all_steps(&handler, TEMPLATE_STANDARD, "other-agent").await;
    let tasks = handler.list().await.expect("列出任务必须成功");
    let task = tasks
        .iter()
        .find(|task| task.id == created.id)
        .expect("任务必须还在");
    assert_eq!(
        task.workflow.steps[0].agent_hint.as_deref(),
        Some("opencode"),
        "创建时锁定的节点配置不能被后续设置改动覆盖"
    );
    assert_eq!(
        task.workflow.steps[2].agent_hint.as_deref(),
        Some("commandcode")
    );

    // 快照齐全 → 开始执行预检通过。
    let started = handler
        .start(OrcTaskIdPayload {
            task_id: created.id.clone(),
        })
        .await
        .expect("锁定快照齐全时开始执行必须成功");
    assert!(started.started);
}

/// 新流程拒绝创建「跑不起来」的任务：缺任一节点 Agent → 明确报错且不落库。
#[tokio::test]
async fn create_with_task_steps_requires_every_agent() {
    let (_root, store) = open_sqlite("agentnotify-orc-create-steps-missing-");
    let handler = dynamic_handler(&store, _root.path());
    let dir = working_dir(&_root);

    let error = handler
        .create(CreateOrcTaskPayload {
            name: None,
            steps: Some(vec![
                OrcTemplateStepConfigDto {
                    order: 1,
                    agent: Some("opencode".into()),
                    model: None,
                },
                OrcTemplateStepConfigDto {
                    order: 2,
                    agent: None,
                    model: None,
                },
                OrcTemplateStepConfigDto {
                    order: 3,
                    agent: Some("codex".into()),
                    model: None,
                },
            ]),
            goal: "缺 Agent 的任务".into(),
            template_id: TEMPLATE_STANDARD.into(),
            working_dir: dir,
            notify_mode: None,
        })
        .await
        .expect_err("缺 Agent 必须报错");
    assert_eq!(error.code, "orc_step_agent_missing");
    assert!(
        error.message.contains("第 2 步"),
        "错误必须点名缺失步骤：{}",
        error.message
    );
    assert!(
        handler.list().await.expect("列出任务必须成功").is_empty(),
        "创建失败不得落库"
    );
}

/// 默认关闭（enabled=false → 未装配仓储）：所有编排命令返回明确错误。
#[tokio::test]
async fn orc_commands_report_clear_error_when_disabled() {
    let handler = OrcCommandHandler::new(None);
    let dir = std::env::temp_dir().to_string_lossy().into_owned();

    let err = handler
        .create(create_payload("目标", PRESET_ID, &dir))
        .await
        .expect_err("未启用时创建任务必须报错");
    assert_eq!(err.code, "orchestration_disabled");
    assert!(
        err.message.contains("编排未启用"),
        "错误必须写清未启用：{}",
        err.message
    );

    let err = handler.list().await.expect_err("未启用时列出任务必须报错");
    assert_eq!(err.code, "orchestration_disabled");

    let err = handler
        .advance(AdvanceOrcTaskPayload {
            task_id: "any".into(),
            kind: OrcMessageKindDto::Report,
        })
        .await
        .expect_err("未启用时推进任务必须报错");
    assert_eq!(err.code, "orchestration_disabled");
}

/// 创建校验（§3.1）：未知模板 / 空目录 / 不存在的目录都必须明确报错且不落库。
#[tokio::test]
async fn create_validates_template_and_working_dir() {
    let (_root, store) = open_sqlite("agentnotify-orc-create-validate-");
    let handler = enabled_handler(&store);
    let dir = working_dir(&_root);

    let err = handler
        .create(create_payload("目标", "template-not-exist", &dir))
        .await
        .expect_err("未知模板必须报错");
    assert_eq!(err.code, "orc_template_unknown");
    assert!(
        err.message.contains("template-not-exist"),
        "{}",
        err.message
    );

    let err = handler
        .create(create_payload("目标", PRESET_ID, "   "))
        .await
        .expect_err("空工作目录必须报错");
    assert_eq!(err.code, "orc_working_dir_invalid");

    let missing = _root.path().join("not-a-dir");
    let err = handler
        .create(create_payload(
            "目标",
            PRESET_ID,
            &missing.to_string_lossy(),
        ))
        .await
        .expect_err("不存在的工作目录必须报错");
    assert_eq!(err.code, "orc_working_dir_invalid");
    assert!(
        err.message.contains("工作目录不存在"),
        "错误必须写清目录不存在：{}",
        err.message
    );

    assert!(
        handler.list().await.expect("列出任务必须成功").is_empty(),
        "校验失败不得落库任务"
    );
}

/// start 预检（§3）：缺任一节点 Agent → 明确报错且任务保持「待开始」；
/// 工作目录已不存在 → 同样明确报错；两者都在派活之前拦下。
#[tokio::test]
async fn start_precheck_requires_agents_and_working_dir() {
    let (_root, store) = open_sqlite("agentnotify-orc-start-precheck-");
    let handler = dynamic_handler(&store, _root.path());
    let dir = working_dir(&_root);

    // 内置模板不预填 Agent：未配置时 start 必须报错且任务保持待开始。
    let created = handler
        .create(create_payload("预检目标", TEMPLATE_STANDARD, &dir))
        .await
        .expect("创建任务必须成功");
    assert_eq!(created.workflow_id, TEMPLATE_STANDARD);
    assert!(!created.started);
    assert_eq!(
        created.workflow.steps.len(),
        3,
        "标准交付模板必须是 3 个节点"
    );

    let err = handler
        .start(OrcTaskIdPayload {
            task_id: created.id.clone(),
        })
        .await
        .expect_err("缺 Agent 时开始执行必须报错");
    assert_eq!(err.code, "orc_step_agent_missing");
    assert!(
        err.message.contains("第 1 步未选择 Agent"),
        "错误必须写清哪一步：{}",
        err.message
    );
    assert!(
        err.message.contains("设置 → 编排"),
        "错误必须给出配置入口：{}",
        err.message
    );
    let tasks = handler.list().await.expect("列出任务必须成功");
    let task = tasks.iter().find(|task| task.id == created.id).unwrap();
    assert_eq!(task.state, OrcTaskStateDto::Working);
    assert!(!task.started, "预检失败必须保持「待开始」");

    // 配置全部节点后 start 成功（无 driver：只标记已开始）。
    configure_all_steps(&handler, TEMPLATE_STANDARD, "opencode").await;
    let started = handler
        .start(OrcTaskIdPayload {
            task_id: created.id.clone(),
        })
        .await
        .expect("配置齐全后开始执行必须成功");
    assert!(started.started);

    // 工作目录在开始前被删除 → 明确报错（预检 2）。
    configure_all_steps(&handler, TEMPLATE_QUICKFIX, "opencode").await;
    let vanished = _root.path().join("vanished");
    std::fs::create_dir_all(&vanished).expect("子目录必须可创建");
    let created2 = handler
        .create(create_payload(
            "目录预检",
            TEMPLATE_QUICKFIX,
            &vanished.to_string_lossy(),
        ))
        .await
        .expect("创建任务必须成功");
    std::fs::remove_dir(&vanished).expect("子目录必须可删除");
    let err = handler
        .start(OrcTaskIdPayload {
            task_id: created2.id.clone(),
        })
        .await
        .expect_err("工作目录不存在时必须报错");
    assert_eq!(err.code, "orc_working_dir_invalid");
    assert!(err.message.contains("工作目录不存在"), "{}", err.message);
}

/// 输入校验：空目标 / 未知通知节奏 / 未启用之外的业务错误都要明确暴露。
#[tokio::test]
async fn orc_commands_validate_inputs_and_expose_business_errors() {
    let (_root, store) = open_sqlite("agentnotify-orc-validate-");
    let handler = enabled_handler(&store);
    let dir = working_dir(&_root);

    let err = handler
        .create(create_payload("   ", PRESET_ID, &dir))
        .await
        .expect_err("空目标必须报错");
    assert_eq!(err.code, "orc_goal_empty");

    let err = handler
        .create(CreateOrcTaskPayload {
            name: None,
            steps: None,
            goal: "目标".into(),
            template_id: PRESET_ID.into(),
            working_dir: dir.clone(),
            notify_mode: Some("noisy".into()),
        })
        .await
        .expect_err("未知通知节奏必须报错");
    assert_eq!(err.code, "orc_notify_mode_invalid");
    assert!(err.message.contains("noisy"), "{}", err.message);

    let err = handler
        .advance(AdvanceOrcTaskPayload {
            task_id: "no-such-task".into(),
            kind: OrcMessageKindDto::Report,
        })
        .await
        .expect_err("推进不存在的任务必须报错");
    assert_eq!(err.code, "orc.task_not_found");

    // 未开始的任务不接受推进：必须先「开始执行」。
    let pending = handler
        .create(create_payload("未开始的任务", PRESET_ID, &dir))
        .await
        .expect("创建任务必须成功");
    let err = handler
        .advance(AdvanceOrcTaskPayload {
            task_id: pending.id.clone(),
            kind: OrcMessageKindDto::Report,
        })
        .await
        .expect_err("未开始任务推进必须报错");
    assert_eq!(err.code, "orc_task_not_started");

    // 最后一步完成 → 进入「项目经理汇总」而非直接完成；首节点汇总到达后才 Completed，
    // 此时不允许再标记阻塞（序号 1→2→3→汇总→完成）。
    let created = handler
        .create(create_payload("不要阻塞", PRESET_ID, &dir))
        .await
        .expect("创建任务必须成功");
    handler
        .start(OrcTaskIdPayload {
            task_id: created.id.clone(),
        })
        .await
        .expect("开始执行必须成功");
    for _ in 0..3 {
        handler
            .advance(AdvanceOrcTaskPayload {
                task_id: created.id.clone(),
                kind: OrcMessageKindDto::Report,
            })
            .await
            .expect("推进必须成功");
    }
    let finalizing = handler
        .list()
        .await
        .expect("列出任务必须成功")
        .into_iter()
        .find(|task| task.id == created.id)
        .expect("任务必须存在");
    assert!(finalizing.finalizing, "最后一步完成必须进入汇总阶段");
    assert_eq!(finalizing.state, OrcTaskStateDto::Working);

    handler
        .report_from_agent(&created.id, 1, "项目经理最终汇报", false)
        .await
        .expect("首节点汇总回注必须成功");
    let err = handler
        .mark_blocked(MarkBlockedOrcTaskPayload {
            task_id: created.id.clone(),
            step: 3,
            reason: "不该出现".into(),
        })
        .await
        .expect_err("已完成任务标记阻塞必须报错");
    assert_eq!(err.code, "orc.cannot_block_terminal");
}

/// 模板列表与节点配置保存（§3）：三档模板按固定顺序返回；保存校验后立即生效、可重读。
#[tokio::test]
async fn templates_and_node_config_round_trip() {
    let (_root, store) = open_sqlite("agentnotify-orc-templates-config-");
    let handler = dynamic_handler(&store, _root.path());

    // 初始：三档模板，节点未配置（不预填）。
    let templates = handler
        .list_orc_templates()
        .await
        .expect("列出模板必须成功");
    let ids: Vec<&str> = templates.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(
        ids,
        vec![TEMPLATE_QUICKFIX, TEMPLATE_STANDARD, "template-full"]
    );
    assert!(
        templates
            .iter()
            .flat_map(|t| &t.steps)
            .all(|step| step.agent.is_none() && step.model.is_none()),
        "模板节点默认不预填"
    );

    // 保存标准交付：全部节点配 opencode，第 1 步指定模型；返回保存后全量。
    let saved = handler
        .save_orc_template_config(SaveOrcTemplateConfigPayload {
            template_id: TEMPLATE_STANDARD.into(),
            steps: vec![
                OrcTemplateStepConfigDto {
                    order: 1,
                    agent: Some("opencode".into()),
                    model: Some("anthropic/claude-sonnet-4-5".into()),
                },
                OrcTemplateStepConfigDto {
                    order: 2,
                    agent: Some("opencode".into()),
                    model: None,
                },
                OrcTemplateStepConfigDto {
                    order: 3,
                    agent: Some("opencode".into()),
                    model: None,
                },
            ],
        })
        .await
        .expect("保存节点配置必须成功");
    let standard = saved
        .iter()
        .find(|t| t.id == TEMPLATE_STANDARD)
        .expect("返回全量必须含标准交付");
    assert_eq!(standard.steps[0].agent.as_deref(), Some("opencode"));
    assert_eq!(
        standard.steps[0].model.as_deref(),
        Some("anthropic/claude-sonnet-4-5")
    );
    assert!(
        standard.steps[1].model.is_none(),
        "未指定模型 = 用 Agent 默认"
    );

    // 重新装配处理器（模拟重启）后配置仍在（settings 持久化）。
    let reloaded = dynamic_handler(&store, _root.path())
        .list_orc_templates()
        .await
        .expect("重新列出模板必须成功");
    let standard = reloaded.iter().find(|t| t.id == TEMPLATE_STANDARD).unwrap();
    assert_eq!(
        standard.steps[0].model.as_deref(),
        Some("anthropic/claude-sonnet-4-5")
    );

    // 校验：未知模板 / 步骤数不符 / order 不符 / 模型格式 / 非 OpenCode 指定模型。
    let err = handler
        .save_orc_template_config(SaveOrcTemplateConfigPayload {
            template_id: "template-nope".into(),
            steps: Vec::new(),
        })
        .await
        .expect_err("未知模板必须报错");
    assert_eq!(err.code, "orc_template_unknown");

    let err = handler
        .save_orc_template_config(SaveOrcTemplateConfigPayload {
            template_id: TEMPLATE_STANDARD.into(),
            steps: vec![OrcTemplateStepConfigDto {
                order: 1,
                agent: Some("opencode".into()),
                model: None,
            }],
        })
        .await
        .expect_err("步骤数不符必须报错");
    assert_eq!(err.code, "orc_template_steps_invalid");

    let err = handler
        .save_orc_template_config(SaveOrcTemplateConfigPayload {
            template_id: TEMPLATE_STANDARD.into(),
            steps: vec![
                OrcTemplateStepConfigDto {
                    order: 2,
                    agent: None,
                    model: None,
                },
                OrcTemplateStepConfigDto {
                    order: 1,
                    agent: None,
                    model: None,
                },
                OrcTemplateStepConfigDto {
                    order: 3,
                    agent: None,
                    model: None,
                },
            ],
        })
        .await
        .expect_err("order 不符必须报错");
    assert_eq!(err.code, "orc_template_steps_invalid");

    let err = handler
        .save_orc_template_config(SaveOrcTemplateConfigPayload {
            template_id: TEMPLATE_STANDARD.into(),
            steps: vec![
                OrcTemplateStepConfigDto {
                    order: 1,
                    agent: Some("opencode".into()),
                    model: Some("no-slash".into()),
                },
                OrcTemplateStepConfigDto {
                    order: 2,
                    agent: None,
                    model: None,
                },
                OrcTemplateStepConfigDto {
                    order: 3,
                    agent: None,
                    model: None,
                },
            ],
        })
        .await
        .expect_err("模型格式非法必须报错");
    assert_eq!(err.code, "orc_model_invalid");

    let err = handler
        .save_orc_template_config(SaveOrcTemplateConfigPayload {
            template_id: TEMPLATE_STANDARD.into(),
            steps: vec![
                OrcTemplateStepConfigDto {
                    order: 1,
                    agent: Some("codex".into()),
                    model: Some("anthropic/claude-sonnet-4-5".into()),
                },
                OrcTemplateStepConfigDto {
                    order: 2,
                    agent: None,
                    model: None,
                },
                OrcTemplateStepConfigDto {
                    order: 3,
                    agent: None,
                    model: None,
                },
            ],
        })
        .await
        .expect_err("非 OpenCode 指定模型必须报错");
    assert_eq!(err.code, "orc_model_agent_unsupported");
    assert_eq!(err.message, "该 Agent 暂不支持指定模型");

    // 保存失败不得破坏已保存配置。
    let still = handler
        .list_orc_templates()
        .await
        .expect("列出模板必须成功");
    let standard = still.iter().find(|t| t.id == TEMPLATE_STANDARD).unwrap();
    assert_eq!(
        standard.steps[1].agent.as_deref(),
        Some("opencode"),
        "校验失败不得清掉已保存配置"
    );
}

/// 节点配置合并进任务：任务按模板解析时应用用户配置的 Agent/模型（§3）。
#[tokio::test]
async fn task_resolution_merges_node_config() {
    let (_root, store) = open_sqlite("agentnotify-orc-node-merge-");
    let handler = dynamic_handler(&store, _root.path());
    configure_all_steps(&handler, TEMPLATE_QUICKFIX, "opencode").await;

    let created = handler
        .create(create_payload(
            "合并目标",
            TEMPLATE_QUICKFIX,
            &working_dir(&_root),
        ))
        .await
        .expect("创建任务必须成功");
    assert_eq!(created.workflow.id, TEMPLATE_QUICKFIX);
    assert_eq!(
        created.workflow.steps[0].agent_hint.as_deref(),
        Some("opencode")
    );
    assert_eq!(
        created.workflow.steps[1].agent_hint.as_deref(),
        Some("opencode")
    );
}

/// DTO 序列化必须使用稳定字符串（UI 契约），并保持脱敏（不暴露内部元数据）。
#[test]
fn orc_state_dto_serializes_to_stable_snake_case_strings() {
    let json = serde_json::to_value(OrcTaskStateDto::InputRequired).expect("必须可序列化");
    assert_eq!(json, serde_json::json!("input_required"));

    let json = serde_json::to_value(OrcMessageKindDto::Instruction).expect("必须可序列化");
    assert_eq!(json, serde_json::json!("instruction"));

    // 模板 DTO 字段用 camelCase（前端按此生成类型）。
    let template = OrcTemplateDto {
        id: "template-standard".into(),
        name: "标准交付".into(),
        steps: vec![agentnotify_desktop::bridge::dto::OrcTemplateStepDto {
            order: 1,
            role: "planner".into(),
            agent: Some("opencode".into()),
            model: Some("anthropic/claude-sonnet-4-5".into()),
        }],
    };
    let json = serde_json::to_value(template).expect("模板必须可序列化");
    assert_eq!(json["steps"][0]["agent"], "opencode");
    assert_eq!(json["steps"][0]["model"], "anthropic/claude-sonnet-4-5");
    assert!(json.get("templateId").is_none(), "模板 id 字段名是 id");
}

/// P1-5 装配：`load_harness_templates` 从 `config_dir/harness-templates.json` 读用户模板；
/// 文件缺失 = 正常未配置（内置默认兜底）；内容损坏 = 告警日志 + 全部回退默认，编排不瘫痪。
#[tokio::test]
async fn load_harness_templates_assembly_seam() {
    // 有效配置：用户模板（按 workflow_id + step order）生效
    {
        let root = tempfile::Builder::new()
            .prefix("agentnotify-orc-templates-valid-")
            .tempdir_in(agentnotify_testkit::test_temp_root())
            .expect("测试临时目录必须可创建");
        let config_dir = root.path().join("config");
        std::fs::create_dir_all(&config_dir).expect("config 目录必须可创建");
        std::fs::write(
            config_dir.join("harness-templates.json"),
            r#"{"workflows":{"preset-requirement-to-report":{"steps":[
                {"order":1,"harness_template":"判断步模板：{goal}"}
            ]}}}"#,
        )
        .expect("写入模板配置必须成功");

        let templates = load_harness_templates(&config_dir);
        assert_eq!(
            templates.user_template("preset-requirement-to-report", 1),
            Some("判断步模板：{goal}")
        );
    }
    // 文件缺失：正常未配置，无用户模板（内置默认兜底）
    {
        let root = tempfile::Builder::new()
            .prefix("agentnotify-orc-templates-missing-")
            .tempdir_in(agentnotify_testkit::test_temp_root())
            .expect("测试临时目录必须可创建");
        let config_dir = root.path().join("config");
        let templates = load_harness_templates(&config_dir);
        assert!(
            templates
                .user_template("preset-requirement-to-report", 1)
                .is_none(),
            "配置缺失时必须回退内置默认"
        );
    }
    // 配置损坏：tracing::warn 告警（无法直接断言日志，断言回退行为）+ 全部回退默认
    {
        let root = tempfile::Builder::new()
            .prefix("agentnotify-orc-templates-corrupt-")
            .tempdir_in(agentnotify_testkit::test_temp_root())
            .expect("测试临时目录必须可创建");
        let config_dir = root.path().join("config");
        std::fs::create_dir_all(&config_dir).expect("config 目录必须可创建");
        std::fs::write(config_dir.join("harness-templates.json"), "{ 不是合法 JSON")
            .expect("写入模板配置必须成功");

        let templates = load_harness_templates(&config_dir);
        assert!(
            templates
                .user_template("preset-requirement-to-report", 1)
                .is_none(),
            "配置损坏时必须回退内置默认，编排不瘫痪"
        );
    }
}

/// P1-5：`OrcCommandHandler` 可注入模板解析器（with_templates），默认构造保持向后兼容。
#[tokio::test]
async fn orc_handler_injects_harness_templates() {
    let (_root, store) = open_sqlite("agentnotify-orc-templates-inject-");
    std::fs::write(
        _root.path().join("harness-templates.json"),
        r#"{"workflows":{"preset-requirement-to-report":{"steps":[
            {"order":1,"harness_template":"注入模板：{goal}"}
        ]}}}"#,
    )
    .expect("写入模板配置必须成功");

    let templates = load_harness_templates(_root.path());
    let handler = OrcCommandHandler::with_templates(
        Some(OrcStore::with_repository(
            Workflow::preset(false).unwrap(),
            store.clone(),
        )),
        templates,
    );
    assert_eq!(
        handler
            .templates()
            .user_template("preset-requirement-to-report", 1),
        Some("注入模板：{goal}")
    );

    // 默认构造（new）不带用户模板：向后兼容，P0 测试与行为不变
    let plain = OrcCommandHandler::new(None);
    assert!(
        plain
            .templates()
            .user_template("preset-requirement-to-report", 1)
            .is_none(),
        "默认构造必须只含内置默认模板"
    );
}

/// §12.4：任务结束前可改任一步的模型/思考强度（只改步骤快照，Agent/结构不动）。
#[tokio::test]
async fn update_task_step_edits_snapshot_model_and_variant() {
    let (_root, store) = open_sqlite("agentnotify-orc-update-step-");
    let handler = dynamic_handler(&store, _root.path());
    let dir = working_dir(&_root);

    let created = handler
        .create(CreateOrcTaskPayload {
            name: None,
            steps: Some(vec![
                OrcTemplateStepConfigDto {
                    order: 1,
                    agent: Some("opencode".into()),
                    model: None,
                },
                OrcTemplateStepConfigDto {
                    order: 2,
                    agent: Some("opencode".into()),
                    model: None,
                },
                OrcTemplateStepConfigDto {
                    order: 3,
                    agent: Some("codex".into()),
                    model: None,
                },
            ]),
            goal: "改模型与强度".into(),
            template_id: TEMPLATE_STANDARD.into(),
            working_dir: dir,
            notify_mode: None,
        })
        .await
        .expect("创建必须成功");
    let task_id = created.id.clone();
    handler
        .start(OrcTaskIdPayload {
            task_id: task_id.clone(),
        })
        .await
        .expect("开始必须成功");

    // 运行中改第 2 步：model + variant 生效（返回 DTO 立即可见，且 trim）。
    let updated = handler
        .update_task_step(UpdateOrcTaskStepPayload {
            task_id: task_id.clone(),
            order: 2,
            model: Some("opencode-go/space-bunny-free".into()),
            variant: Some(" high ".into()),
        })
        .await
        .expect("运行中修改必须成功");
    assert_eq!(
        updated.workflow.steps[1].model.as_deref(),
        Some("opencode-go/space-bunny-free"),
        "返回 DTO 必须体现新模型"
    );
    assert_eq!(
        updated.workflow.steps[1].variant.as_deref(),
        Some("high"),
        "强度必须 trim 后写入"
    );
    assert_eq!(updated.workflow.steps[0].model, None, "其他步骤不受影响");
    assert_eq!(updated.current_step, 1, "修改节点配置不改变任务进度");

    // 列表（重新按快照解析）同样展示新值。
    let listed = handler.list().await.expect("列出任务必须成功");
    let task = listed
        .iter()
        .find(|task| task.id == task_id)
        .expect("任务必须存在");
    assert_eq!(task.workflow.steps[1].variant.as_deref(), Some("high"));

    // model=None = 不改模型，只改强度。
    let only_variant = handler
        .update_task_step(UpdateOrcTaskStepPayload {
            task_id: task_id.clone(),
            order: 2,
            model: None,
            variant: Some("xhigh".into()),
        })
        .await
        .expect("只改强度必须成功");
    assert_eq!(
        only_variant.workflow.steps[1].model.as_deref(),
        Some("opencode-go/space-bunny-free"),
        "未提交模型时保持原模型"
    );
    assert_eq!(
        only_variant.workflow.steps[1].variant.as_deref(),
        Some("xhigh")
    );

    // 空 model = 清除模型，强度一并清空。
    let cleared = handler
        .update_task_step(UpdateOrcTaskStepPayload {
            task_id: task_id.clone(),
            order: 2,
            model: Some("  ".into()),
            variant: Some("max".into()),
        })
        .await
        .expect("清除模型必须成功");
    assert_eq!(cleared.workflow.steps[1].model, None);
    assert_eq!(
        cleared.workflow.steps[1].variant, None,
        "模型清除时强度一并清空"
    );

    // 非法模型格式 → 明确报错 + 示例写法。
    let err = handler
        .update_task_step(UpdateOrcTaskStepPayload {
            task_id: task_id.clone(),
            order: 2,
            model: Some("no-slash".into()),
            variant: None,
        })
        .await
        .expect_err("非法模型必须报错");
    assert_eq!(err.code, "orc_model_invalid");
    assert!(
        err.message.contains("provider/model"),
        "错误必须给出格式示例：{}",
        err.message
    );

    // 非 OpenCode 步（第 3 步 codex）不支持指定模型。
    let err = handler
        .update_task_step(UpdateOrcTaskStepPayload {
            task_id: task_id.clone(),
            order: 3,
            model: Some("opencode-go/space-bunny-free".into()),
            variant: None,
        })
        .await
        .expect_err("非 OpenCode 指定模型必须报错");
    assert_eq!(err.code, "orc_model_agent_unsupported");
    assert_eq!(err.message, "该 Agent 暂不支持指定模型");

    // 步骤不存在 → 明确报错。
    let err = handler
        .update_task_step(UpdateOrcTaskStepPayload {
            task_id,
            order: 9,
            model: None,
            variant: None,
        })
        .await
        .expect_err("不存在的步骤必须报错");
    assert_eq!(err.code, "orc.step_not_found");
}

/// §12.4：终态只读（Completed/Canceled/Rejected）；无步骤快照的旧任务明确拒绝并指路。
#[tokio::test]
async fn update_task_step_rejects_terminal_and_snapshotless_tasks() {
    // 1) 旧任务（未锁定步骤快照）→ 明确指路「设置 → 编排」或重新创建。
    {
        let (_root, store) = open_sqlite("agentnotify-orc-update-step-legacy-");
        let handler = enabled_handler(&store);
        let created = handler
            .create(create_payload("旧任务", PRESET_ID, &working_dir(&_root)))
            .await
            .expect("创建必须成功");
        let err = handler
            .update_task_step(UpdateOrcTaskStepPayload {
                task_id: created.id,
                order: 1,
                model: Some("opencode-go/space-bunny-free".into()),
                variant: None,
            })
            .await
            .expect_err("无步骤快照必须拒绝");
        assert_eq!(err.code, "orc_task_step_locked");
        assert!(
            err.message.contains("没有步骤快照"),
            "错误必须写清原因：{}",
            err.message
        );
        assert!(
            err.message.contains("重新创建任务"),
            "错误必须给出出路：{}",
            err.message
        );
    }

    // 2) 完成任务（走完两轮 + 汇总）→ 终态只读。
    {
        let (_root, store) = open_sqlite("agentnotify-orc-update-step-done-");
        let handler = dynamic_handler(&store, _root.path());
        let created = handler
            .create(CreateOrcTaskPayload {
                name: None,
                steps: Some(vec![
                    OrcTemplateStepConfigDto {
                        order: 1,
                        agent: Some("opencode".into()),
                        model: None,
                    },
                    OrcTemplateStepConfigDto {
                        order: 2,
                        agent: Some("opencode".into()),
                        model: None,
                    },
                ]),
                goal: "完成后只读".into(),
                template_id: TEMPLATE_QUICKFIX.into(),
                working_dir: working_dir(&_root),
                notify_mode: None,
            })
            .await
            .expect("创建必须成功");
        let task_id = created.id.clone();
        handler
            .start(OrcTaskIdPayload {
                task_id: task_id.clone(),
            })
            .await
            .expect("开始必须成功");
        handler
            .report_from_agent(&task_id, 1, "第 1 步完成", false)
            .await
            .expect("第 1 步汇报必须成功");
        handler
            .report_from_agent(&task_id, 2, "第 2 步完成", false)
            .await
            .expect("第 2 步汇报必须成功");
        handler
            .report_from_agent(&task_id, 1, "汇总完成", false)
            .await
            .expect("汇总回合必须成功");
        let done = handler
            .list()
            .await
            .expect("列出任务必须成功")
            .into_iter()
            .find(|task| task.id == task_id)
            .expect("任务必须存在");
        assert_eq!(done.state, OrcTaskStateDto::Completed);

        let err = handler
            .update_task_step(UpdateOrcTaskStepPayload {
                task_id,
                order: 1,
                model: None,
                variant: Some("high".into()),
            })
            .await
            .expect_err("完成任务必须只读");
        assert_eq!(err.code, "orc_task_step_locked");
        assert_eq!(err.message, "任务已结束：不能再修改节点模型");
    }
}
