//! Workflow 模型测试：预置工作流形状、构造校验、步骤查询。

use agentnotify_orchestration::{
    AGENT_HINT_CODEX, AGENT_HINT_COMMANDCODE, AGENT_HINT_OPENCODE, OrcErrorCode,
    PRESET_OPENCODE_ONLY_ID, PRESET_ORDER_EXECUTE, PRESET_ORDER_JUDGE, PRESET_ORDER_PLAN,
    PRESET_ORDER_REVIEW, PRESET_WORKFLOW_ID, PRESET_WORKFLOW_NAME, ROLE_EXECUTOR,
    ROLE_ORCHESTRATOR, ROLE_PLANNER, ROLE_REVIEWER, TEMPLATE_FULL_ID, TEMPLATE_FULL_NAME,
    TEMPLATE_IDS, TEMPLATE_QUICKFIX_ID, TEMPLATE_QUICKFIX_NAME, TEMPLATE_STANDARD_ID,
    TEMPLATE_STANDARD_NAME, Workflow, WorkflowStep,
};

/// 便捷构造普通步骤（无门、无模板、无 Agent 提示）。
fn step(order: u32, role: &str) -> WorkflowStep {
    WorkflowStep::new(order, role, None, None, false)
}

/// 预置工作流（不带复核）：3 步，顺序/角色/Agent 提示符合设计（§3.1.1）。
#[test]
fn preset_without_reviewer_shape() {
    let wf = Workflow::preset(false).unwrap();
    assert_eq!(wf.id, PRESET_WORKFLOW_ID);
    assert_eq!(wf.name, PRESET_WORKFLOW_NAME);
    assert_eq!(wf.steps.len(), 3);
    assert_eq!(wf.max_order(), 3);

    let s1 = wf.step(PRESET_ORDER_JUDGE).unwrap();
    assert_eq!(s1.order, 1);
    assert_eq!(s1.role, ROLE_ORCHESTRATOR);
    assert_eq!(s1.agent_hint.as_deref(), Some(AGENT_HINT_CODEX));
    assert!(!s1.human_gate, "判断步不应有人工确认门");
    assert!(s1.harness_template.is_none(), "预置步骤用内置默认信封");

    let s2 = wf.step(PRESET_ORDER_PLAN).unwrap();
    assert_eq!(s2.role, ROLE_PLANNER);
    assert_eq!(s2.agent_hint.as_deref(), Some(AGENT_HINT_OPENCODE));
    assert!(!s2.human_gate);

    let s3 = wf.step(PRESET_ORDER_EXECUTE).unwrap();
    assert_eq!(s3.role, ROLE_EXECUTOR);
    assert_eq!(s3.agent_hint.as_deref(), Some(AGENT_HINT_COMMANDCODE));
    assert!(!s3.human_gate, "无复核时实施步即最后一步，汇报即完成");
}

/// 预置工作流（带复核）：4 步，第 4 步复核默认开启人工确认门。
#[test]
fn preset_with_reviewer_shape() {
    let wf = Workflow::preset(true).unwrap();
    assert_eq!(wf.steps.len(), 4);
    assert_eq!(wf.max_order(), 4);

    // 前三步与无复核版本一致：无门
    for order in [PRESET_ORDER_JUDGE, PRESET_ORDER_PLAN, PRESET_ORDER_EXECUTE] {
        assert!(!wf.step(order).unwrap().human_gate, "第 {order} 步不应有门");
    }

    let s4 = wf.step(PRESET_ORDER_REVIEW).unwrap();
    assert_eq!(s4.role, ROLE_REVIEWER);
    assert!(s4.human_gate, "复核步（最终汇报）应开启人工确认门");
    assert!(s4.agent_hint.is_none(), "复核步 Agent 不预置，由用户选");
}

/// 预置步骤序号严格连续（1..=N），不跳号。
#[test]
fn preset_orders_are_continuous() {
    for include_reviewer in [false, true] {
        let wf = Workflow::preset(include_reviewer).unwrap();
        let orders: Vec<u32> = wf.steps.iter().map(|s| s.order).collect();
        let expect: Vec<u32> = (1..=wf.max_order()).collect();
        assert_eq!(
            orders, expect,
            "include_reviewer={include_reviewer} 序号应连续"
        );
    }
}

/// 空步骤工作流必须明确报错（WorkflowEmpty）。
#[test]
fn new_rejects_empty_workflow() {
    let err = Workflow::new("wf-1", "空", vec![]).unwrap_err();
    assert_eq!(err.code, OrcErrorCode::WorkflowEmpty);
    assert!(err.message.contains("没有任何步骤"), "{}", err.message);
}

/// 跳号/不从 1 开始的步骤必须明确报错（InvalidStepOrder）。
#[test]
fn new_rejects_gapped_orders() {
    for steps in [
        vec![step(2, ROLE_ORCHESTRATOR), step(3, ROLE_PLANNER)],
        vec![step(1, ROLE_ORCHESTRATOR), step(3, ROLE_PLANNER)],
    ] {
        let err = Workflow::new("wf-gap", "跳号", steps).unwrap_err();
        assert_eq!(err.code, OrcErrorCode::InvalidStepOrder);
        assert!(err.message.contains("连续递增"), "{}", err.message);
    }
}

/// is_last / next_step / step 查询行为。
#[test]
fn step_lookup_and_ordering() {
    let wf = Workflow::preset(true).unwrap(); // 4 步
    assert!(!wf.is_last(1));
    assert!(!wf.is_last(3));
    assert!(wf.is_last(4));
    assert!(!wf.is_last(0), "越界不等于最后一步");
    assert!(!wf.is_last(5));

    assert_eq!(wf.next_step(1).unwrap().order, 2);
    assert_eq!(wf.next_step(3).unwrap().order, 4);
    assert!(wf.next_step(4).is_none(), "最后一步没有下一步");

    assert!(wf.step(2).is_some());
    assert!(wf.step(0).is_none(), "order=0 越界");
    assert!(wf.step(99).is_none(), "order 超出范围越界");
}

/// 只用 OpenCode 的单 Agent 工作流：三步角色各司其职、Agent 提示全是 opencode（§3.1 解耦，R11）。
#[test]
fn preset_opencode_only_shape() {
    let wf = Workflow::preset_opencode_only().unwrap();
    assert_eq!(wf.id, "preset-opencode-only");
    assert_eq!(wf.steps.len(), 3);
    assert_eq!(wf.max_order(), 3);

    for order in [PRESET_ORDER_JUDGE, PRESET_ORDER_PLAN, PRESET_ORDER_EXECUTE] {
        let s = wf.step(order).unwrap();
        assert_eq!(
            s.agent_hint.as_deref(),
            Some(AGENT_HINT_OPENCODE),
            "Step {order} 必须由 OpenCode 承担"
        );
        assert!(!s.human_gate, "单 Agent 流转步骤不应有人工确认门");
    }

    assert_eq!(wf.step(PRESET_ORDER_JUDGE).unwrap().role, ROLE_ORCHESTRATOR);
    assert_eq!(wf.step(PRESET_ORDER_PLAN).unwrap().role, ROLE_PLANNER);
    assert_eq!(wf.step(PRESET_ORDER_EXECUTE).unwrap().role, ROLE_EXECUTOR);
}

/// 用户自定义工作流：自定义角色与 human_gate 原样保留。
#[test]
fn custom_workflow_keeps_user_fields() {
    let steps = vec![
        step(1, ROLE_ORCHESTRATOR),
        WorkflowStep::new(
            2,
            "我在看",
            Some("我的-agent".to_string()),
            Some("自定义模板".to_string()),
            true,
        ),
    ];
    let wf = Workflow::new("wf-custom".to_string(), "自定义".to_string(), steps).unwrap();
    let s2 = wf.step(2).unwrap();
    assert_eq!(s2.role, "我在看");
    assert_eq!(s2.agent_hint.as_deref(), Some("我的-agent"));
    assert_eq!(s2.harness_template.as_deref(), Some("自定义模板"));
    assert!(s2.human_gate);
}

/// 三档固定模板的形状与「不预置 Agent/模型」承诺（P3 设计 §2/§3）。
#[test]
fn fixed_templates_shape() {
    let quickfix = Workflow::template_quickfix().unwrap();
    assert_eq!(quickfix.id, TEMPLATE_QUICKFIX_ID);
    assert_eq!(quickfix.name, TEMPLATE_QUICKFIX_NAME);
    assert_eq!(quickfix.steps.len(), 2);
    assert_eq!(quickfix.step(1).unwrap().role, ROLE_EXECUTOR);
    assert_eq!(quickfix.step(2).unwrap().role, ROLE_REVIEWER);

    let standard = Workflow::template_standard().unwrap();
    assert_eq!(standard.id, TEMPLATE_STANDARD_ID);
    assert_eq!(standard.name, TEMPLATE_STANDARD_NAME);
    assert_eq!(standard.steps.len(), 3);
    let roles: Vec<&str> = standard.steps.iter().map(|s| s.role.as_str()).collect();
    assert_eq!(roles, [ROLE_PLANNER, ROLE_EXECUTOR, ROLE_REVIEWER]);

    let full = Workflow::template_full().unwrap();
    assert_eq!(full.id, TEMPLATE_FULL_ID);
    assert_eq!(full.name, TEMPLATE_FULL_NAME);
    assert_eq!(full.steps.len(), 4);
    let roles: Vec<&str> = full.steps.iter().map(|s| s.role.as_str()).collect();
    assert_eq!(
        roles,
        [
            ROLE_ORCHESTRATOR,
            ROLE_PLANNER,
            ROLE_EXECUTOR,
            ROLE_REVIEWER
        ]
    );

    for wf in [&quickfix, &standard, &full] {
        for (index, step) in wf.steps.iter().enumerate() {
            assert_eq!(step.order, (index + 1) as u32, "序号必须从 1 连续");
            assert!(step.agent_hint.is_none(), "模板不预置 Agent（由用户配置）");
            assert!(step.model.is_none(), "模板不预置模型（由用户配置）");
            assert!(!step.human_gate, "固定模板不含人工确认门");
            assert!(step.harness_template.is_none(), "模板用内置默认信封");
        }
    }
}

/// 内置目录：三档模板 + 旧预设 id 均可解析；未知 id 明确返回 None。
#[test]
fn builtin_lookup_covers_templates_and_legacy_presets() {
    for id in TEMPLATE_IDS {
        let wf = Workflow::builtin(id).unwrap_or_else(|| panic!("模板 {id} 必须可解析"));
        assert_eq!(wf.id, id);
    }
    assert!(
        Workflow::builtin(PRESET_WORKFLOW_ID).is_some(),
        "旧预设 id 仍可解析（老任务兼容）"
    );
    assert!(Workflow::builtin(PRESET_OPENCODE_ONLY_ID).is_some());
    assert!(Workflow::builtin("no-such-workflow").is_none());
}

/// 步骤模型为可选项：默认空（用 Agent 默认模型），`with_model` 可设置。
#[test]
fn step_model_is_optional_and_settable() {
    let plain = step(1, ROLE_EXECUTOR);
    assert!(plain.model.is_none());

    let with_model = step(1, ROLE_EXECUTOR).with_model("anthropic/claude-sonnet-4-5");
    assert_eq!(
        with_model.model.as_deref(),
        Some("anthropic/claude-sonnet-4-5")
    );
}
