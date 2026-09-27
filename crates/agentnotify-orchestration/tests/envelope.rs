//! 任务信封测试：默认模板占位符替换、用户模板覆盖、最后一步、兜底与未知占位符。

use agentnotify_orchestration::{Workflow, WorkflowStep, render_envelope};

/// 默认模板：占位符全部替换为运行时值。
#[test]
fn default_template_substitutes_all_placeholders() {
    let wf = Workflow::preset(false).unwrap();
    let step1 = wf.step(1).unwrap();
    let envelope = render_envelope(&wf, step1, "做一个贪吃蛇游戏", Some("planner"));

    assert!(
        envelope.contains("【任务：做一个贪吃蛇游戏】"),
        "{envelope}"
    );
    assert!(
        envelope.contains("工作流：需求→判断→规划→实施（Step 1/3）"),
        "{envelope}"
    );
    assert!(
        envelope.contains("你的角色：orchestrator（建议 Agent：codex）"),
        "{envelope}"
    );
    assert!(envelope.contains("你的活：只做本步职责"), "{envelope}");
    assert!(
        envelope.contains("你不能：推进到下一步（由编排层负责）"),
        "{envelope}"
    );
    assert!(envelope.contains("下一步：planner 接手。"), "{envelope}");
    assert!(!envelope.contains("{goal}"), "没有残留占位符：{envelope}");
    assert!(
        !envelope.contains("{step_index}"),
        "没有残留占位符：{envelope}"
    );
}

/// 中间步骤：next_role 来自下一步骤。
#[test]
fn middle_step_names_next_role() {
    let wf = Workflow::preset(false).unwrap();
    let step2 = wf.step(2).unwrap();
    let envelope = render_envelope(&wf, step2, "做一个贪吃蛇游戏", Some("executor"));
    assert!(envelope.contains("Step 2/3"), "{envelope}");
    assert!(envelope.contains("下一步：executor 接手。"), "{envelope}");
}

/// 最后一步：next_role=None → 提示汇报后任务即完成。
#[test]
fn last_step_without_next_role() {
    let wf = Workflow::preset(false).unwrap();
    let last = wf.step(3).unwrap();
    let envelope = render_envelope(&wf, last, "做一个贪吃蛇游戏", None);
    assert!(envelope.contains("Step 3/3"), "{envelope}");
    assert!(
        envelope.contains("已是最后一步，汇报后任务即完成。"),
        "{envelope}"
    );
}

/// 用户模板覆盖生效：只含部分占位符的模板也按表替换，其余文本原样保留。
#[test]
fn user_template_overrides_default() {
    let wf = Workflow::preset(false).unwrap();
    let mut step2 = wf.step(2).unwrap().clone();
    step2.harness_template = Some(
        "【{goal}】{role} {step_index}/{step_total} → {next_role}（{agent_hint}）".to_string(),
    );

    let envelope = render_envelope(&wf, &step2, "贪吃蛇", Some("executor"));
    assert_eq!(
        envelope,
        "【贪吃蛇】planner 2/3 → 下一步：executor 接手。（opencode）"
    );
}

/// 空白模板（Some("  ")）按「未配置」处理，回退内置默认模板。
#[test]
fn blank_template_falls_back_to_default() {
    let wf = Workflow::preset(false).unwrap();
    let mut step1 = wf.step(1).unwrap().clone();
    step1.harness_template = Some("   ".to_string());
    let envelope = render_envelope(&wf, &step1, "贪吃蛇", Some("planner"));
    assert!(envelope.contains("【任务：贪吃蛇】"), "{envelope}");
    assert!(envelope.contains("Step 1/3"), "{envelope}");
}

/// user 模板里未配置的步骤 → 默认模板；手动构造无模板步骤同样兜底。
#[test]
fn no_template_uses_default() {
    let custom_steps = vec![WorkflowStep::new(1, "判断", None, None, false)];
    let wf = Workflow::new("wf-t", "测试", custom_steps).unwrap();
    let envelope = render_envelope(&wf, wf.step(1).unwrap(), "目标文本", None);
    assert!(envelope.contains("【任务：目标文本】"), "{envelope}");
    assert!(
        envelope.contains("你的角色：判断（建议 Agent：未指定）"),
        "{envelope}"
    );
    assert!(envelope.contains("Step 1/1"), "{envelope}");
}

/// 未知占位符原样保留（异常模板只影响该任务文案，不炸系统）。
#[test]
fn unknown_placeholder_stays_literal() {
    let wf = Workflow::preset(false).unwrap();
    let mut step1 = wf.step(1).unwrap().clone();
    step1.harness_template = Some("占位 {unknown} {goal}".to_string());
    let envelope = render_envelope(&wf, &step1, "贪吃蛇", Some("planner"));
    // 已知占位符被替换、未知占位符原样保留
    assert_eq!(envelope, "占位 {unknown} 贪吃蛇");
}

/// 同一模板在多步/多角色下重复渲染互不影响（纯函数）。
#[test]
fn rendering_is_pure_and_repeatable() {
    let wf = Workflow::preset(false).unwrap();
    let e1 = render_envelope(&wf, wf.step(1).unwrap(), "g1", Some("planner"));
    let e2 = render_envelope(&wf, wf.step(1).unwrap(), "g1", Some("planner"));
    assert_eq!(e1, e2);
    let e3 = render_envelope(&wf, wf.step(1).unwrap(), "g2", Some("planner"));
    assert!(e3.contains("g2") && !e3.contains("g1"), "{e3}");
}
