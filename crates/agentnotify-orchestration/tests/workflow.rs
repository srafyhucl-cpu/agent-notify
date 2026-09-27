//! Workflow 模型测试：预置工作流形状、构造校验、步骤查询。

use agentnotify_orchestration::{
    AGENT_HINT_CODEX, AGENT_HINT_COMMANDCODE, AGENT_HINT_OPENCODE, OrcErrorCode,
    PRESET_ORDER_EXECUTE, PRESET_ORDER_JUDGE, PRESET_ORDER_PLAN, PRESET_ORDER_REVIEW,
    PRESET_WORKFLOW_ID, PRESET_WORKFLOW_NAME, ROLE_EXECUTOR, ROLE_ORCHESTRATOR, ROLE_PLANNER,
    ROLE_REVIEWER, Workflow, WorkflowStep,
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
