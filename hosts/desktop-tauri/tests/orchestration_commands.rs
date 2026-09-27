//! 编排命令端到端测试（P1-1）：SQLite 真实文件上的 create→advance→mark_blocked→recover，
//! 以及默认关闭（enabled=false）时的明确报错。不经过完整宿主装配，命令层逻辑与
//! `ProductionHostCommandService` 中同一 [`OrcCommandHandler`] 实现。

use std::sync::Arc;

use agentnotify_desktop::bridge::dto::{
    AdvanceOrcTaskPayload, CreateOrcTaskPayload, MarkBlockedOrcTaskPayload, OrcMessageKindDto,
    OrcTaskIdPayload, OrcTaskStateDto,
};
use agentnotify_desktop::production::service::{OrcCommandHandler, load_harness_templates};
use agentnotify_orchestration::{OrcStore, Workflow};
use agentnotify_storage_sqlite::SqliteStore;

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

/// create → advance → mark_blocked → recover 全链路（SQLite 真实文件）。
#[tokio::test]
async fn orc_commands_full_chain_on_sqlite() {
    let (_root, store) = open_sqlite("agentnotify-orc-chain-");
    let handler = enabled_handler(&store);

    // 1. create：Working / 第 1 步 / final_only 默认
    let created = handler
        .create(CreateOrcTaskPayload {
            goal: "做一个贪吃蛇游戏".into(),
            notify_mode: None,
        })
        .await
        .expect("创建任务必须成功");
    assert_eq!(created.state, OrcTaskStateDto::Working);
    assert_eq!(created.current_step, 1);
    assert_eq!(created.notify_mode, "final_only");
    assert_eq!(created.goal, "做一个贪吃蛇游戏");
    assert!(!created.workflow_id.is_empty());
    let task_id = created.id.clone();

    // 2. advance（汇报）→ 第 2 步
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
    let task_id;

    {
        let store = Arc::new(SqliteStore::open(&db_path).expect("SQLite 必须可创建"));
        let handler = enabled_handler(&store);
        let created = handler
            .create(CreateOrcTaskPayload {
                goal: "持久化验证".into(),
                notify_mode: Some("verbose".into()),
            })
            .await
            .expect("创建任务必须成功");
        task_id = created.id;
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
    }
}

/// 默认关闭（enabled=false → 未装配仓储）：所有编排命令返回明确错误。
#[tokio::test]
async fn orc_commands_report_clear_error_when_disabled() {
    let handler = OrcCommandHandler::new(None);

    let err = handler
        .create(CreateOrcTaskPayload {
            goal: "目标".into(),
            notify_mode: None,
        })
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

/// 输入校验：空目标 / 未知通知节奏 / 未启用之外的业务错误都要明确暴露。
#[tokio::test]
async fn orc_commands_validate_inputs_and_expose_business_errors() {
    let (_root, store) = open_sqlite("agentnotify-orc-validate-");
    let handler = enabled_handler(&store);

    let err = handler
        .create(CreateOrcTaskPayload {
            goal: "   ".into(),
            notify_mode: None,
        })
        .await
        .expect_err("空目标必须报错");
    assert_eq!(err.code, "orc_goal_empty");

    let err = handler
        .create(CreateOrcTaskPayload {
            goal: "目标".into(),
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

    // 已完成任务不允许标记阻塞（序号 1→2→3→完成）。
    let created = handler
        .create(CreateOrcTaskPayload {
            goal: "不要阻塞".into(),
            notify_mode: None,
        })
        .await
        .expect("创建任务必须成功");
    for _ in 0..3 {
        handler
            .advance(AdvanceOrcTaskPayload {
                task_id: created.id.clone(),
                kind: OrcMessageKindDto::Report,
            })
            .await
            .expect("推进必须成功");
    }
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

/// DTO 序列化必须使用稳定字符串（UI 契约），并保持脱敏（不暴露内部元数据）。
#[test]
fn orc_state_dto_serializes_to_stable_snake_case_strings() {
    let json = serde_json::to_value(OrcTaskStateDto::InputRequired).expect("必须可序列化");
    assert_eq!(json, serde_json::json!("input_required"));

    let json = serde_json::to_value(OrcMessageKindDto::Instruction).expect("必须可序列化");
    assert_eq!(json, serde_json::json!("instruction"));
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
