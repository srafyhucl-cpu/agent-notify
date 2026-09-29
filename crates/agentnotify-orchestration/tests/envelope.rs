//! 任务信封测试：角色模板占位符替换、用户模板覆盖、最后一步、轮次与判定解析。

use agentnotify_orchestration::{Workflow, WorkflowStep, render_envelope, round_continue_input};

/// 首节点（orchestrator）：按角色的内置模板，占位符全部替换为运行时值。
#[test]
fn default_template_substitutes_all_placeholders() {
    let wf = Workflow::preset(false).unwrap();
    let step1 = wf.step(1).unwrap();
    let envelope = render_envelope(&wf, step1, "做一个贪吃蛇游戏", Some("planner"), 1);

    assert!(
        envelope.contains("【任务：做一个贪吃蛇游戏】"),
        "{envelope}"
    );
    assert!(
        envelope.contains("工作流：需求→判断→规划→实施（第 1 轮 · Step 1/3）"),
        "{envelope}"
    );
    assert!(
        envelope.contains("你的角色：初步判断（本任务的项目经理；建议 Agent：codex）"),
        "{envelope}"
    );
    // 角色化提示词：项目经理要定方向、给决策，而不是通用套话
    assert!(envelope.contains("读懂目标"), "{envelope}");
    assert!(envelope.contains("关键决策"), "{envelope}");
    assert!(
        envelope.contains("你不能：直接开始写实现代码"),
        "{envelope}"
    );
    assert!(envelope.contains("下一步：planner 接手。"), "{envelope}");
    assert!(!envelope.contains("{goal}"), "没有残留占位符：{envelope}");
    assert!(
        !envelope.contains("{step_index}") && !envelope.contains("{round}"),
        "没有残留占位符：{envelope}"
    );
}

/// 中间步骤：next_role 来自下一步骤；轮次占位符随入参变化。
#[test]
fn middle_step_names_next_role() {
    let wf = Workflow::preset(false).unwrap();
    let step2 = wf.step(2).unwrap();
    let envelope = render_envelope(&wf, step2, "做一个贪吃蛇游戏", Some("executor"), 2);
    assert!(envelope.contains("第 2 轮 · Step 2/3"), "{envelope}");
    assert!(envelope.contains("下一步：executor 接手。"), "{envelope}");
}

/// 角色模板差异：实施/复核各有其职责边界与汇报要求。
#[test]
fn role_templates_are_specific() {
    let wf = Workflow::preset(false).unwrap();
    let executor = render_envelope(&wf, wf.step(3).unwrap(), "g", None, 1);
    assert!(executor.contains("有前序方案就照方案做"), "{executor}");
    assert!(executor.contains("自己先验证"), "{executor}");

    let reviewer_step = WorkflowStep::new(3, "reviewer", Some("opencode".to_string()), None, false);
    let reviewer = render_envelope(&wf, &reviewer_step, "g", None, 1);
    assert!(reviewer.contains("逐条检查"), "{reviewer}");
    assert!(
        reviewer.contains("通过 / 有条件通过 / 不通过"),
        "{reviewer}"
    );
    assert!(reviewer.contains("不要自己动手改"), "{reviewer}");

    let planner_step = WorkflowStep::new(2, "planner", Some("opencode".to_string()), None, false);
    let planner = render_envelope(&wf, &planner_step, "g", None, 1);
    assert!(planner.contains("任务清单"), "{planner}");
    assert!(planner.contains("验收口径"), "{planner}");
}

/// 三档内置模板（2/3/4 步）都按角色渲染：占位符全部替换、步号正确、角色提示词可读。
#[test]
fn builtin_templates_render_for_all_step_counts() {
    for id in ["template-quickfix", "template-standard", "template-full"] {
        let wf = Workflow::builtin(id).unwrap_or_else(|| panic!("内置模板必须存在：{id}"));
        let total = wf.steps.len();
        assert!(total == 2 || total == 3 || total == 4, "{id} 步数异常");
        for (index, step) in wf.steps.iter().enumerate() {
            let next = wf.steps.get(index + 1).map(|next| next.role.as_str());
            let text = render_envelope(&wf, step, "做一个鹈鹕骑车图", next, 2);
            assert!(
                !text.contains('{'),
                "占位符必须全部替换（{id} 第 {} 步）：{text}",
                step.order
            );
            assert!(
                text.contains(&format!("第 2 轮 · Step {}/{}", step.order, total)),
                "步号与轮次必须正确（{id} 第 {} 步）：{text}",
                step.order
            );
            match step.role.as_str() {
                "reviewer" => assert!(text.contains("结论"), "{id} 复核必须给结论：{text}"),
                "executor" => assert!(text.contains("实现"), "{id} 实施必须讲实现：{text}"),
                "planner" => assert!(text.contains("清单"), "{id} 规划必须给清单：{text}"),
                "orchestrator" => assert!(text.contains("决策"), "{id} 判断必须给决策：{text}"),
                other => panic!("{id} 出现未知角色：{other}"),
            }
        }
    }
}

/// 最后一步：next_role=None → 提示汇报后任务即完成。
#[test]
fn last_step_without_next_role() {
    let wf = Workflow::preset(false).unwrap();
    let last = wf.step(3).unwrap();
    let envelope = render_envelope(&wf, last, "做一个贪吃蛇游戏", None, 1);
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

    let envelope = render_envelope(&wf, &step2, "贪吃蛇", Some("executor"), 1);
    assert_eq!(
        envelope,
        "【贪吃蛇】planner 2/3 → 下一步：executor 接手。（opencode）"
    );
}

/// 空白模板（Some("  ")）按「未配置」处理，回退按角色的内置默认模板。
#[test]
fn blank_template_falls_back_to_default() {
    let wf = Workflow::preset(false).unwrap();
    let mut step1 = wf.step(1).unwrap().clone();
    step1.harness_template = Some("   ".to_string());
    let envelope = render_envelope(&wf, &step1, "贪吃蛇", Some("planner"), 1);
    assert!(envelope.contains("【任务：贪吃蛇】"), "{envelope}");
    assert!(envelope.contains("Step 1/3"), "{envelope}");
    assert!(envelope.contains("项目经理"), "按角色回退：{envelope}");
}

/// 未知角色 → 通用兜底模板；手动构造无模板步骤同样兜底。
#[test]
fn no_template_uses_default() {
    let custom_steps = vec![WorkflowStep::new(1, "判断", None, None, false)];
    let wf = Workflow::new("wf-t", "测试", custom_steps).unwrap();
    let envelope = render_envelope(&wf, wf.step(1).unwrap(), "目标文本", None, 1);
    assert!(envelope.contains("【任务：目标文本】"), "{envelope}");
    assert!(
        envelope.contains("你的角色：判断（建议 Agent：未指定）"),
        "{envelope}"
    );
    assert!(envelope.contains("Step 1/1"), "{envelope}");
    assert!(envelope.contains("只做本步职责"), "通用模板：{envelope}");
}

/// 未知占位符原样保留（异常模板只影响该任务文案，不炸系统）。
#[test]
fn unknown_placeholder_stays_literal() {
    let wf = Workflow::preset(false).unwrap();
    let mut step1 = wf.step(1).unwrap().clone();
    step1.harness_template = Some("占位 {unknown} {goal}".to_string());
    let envelope = render_envelope(&wf, &step1, "贪吃蛇", Some("planner"), 1);
    // 已知占位符被替换、未知占位符原样保留
    assert_eq!(envelope, "占位 {unknown} 贪吃蛇");
}

/// 同一模板在多步/多角色下重复渲染互不影响（纯函数）。
#[test]
fn rendering_is_pure_and_repeatable() {
    let wf = Workflow::preset(false).unwrap();
    let e1 = render_envelope(&wf, wf.step(1).unwrap(), "g1", Some("planner"), 1);
    let e2 = render_envelope(&wf, wf.step(1).unwrap(), "g1", Some("planner"), 1);
    assert_eq!(e1, e2);
    let e3 = render_envelope(&wf, wf.step(1).unwrap(), "g2", Some("planner"), 1);
    assert!(e3.contains("g2") && !e3.contains("g1"), "{e3}");
}

/// 轮次判定解析：只有明确「继续迭代」才返回下一轮问题；达标/无判定按完成处理（不猜）。
#[test]
fn round_verdict_parsing() {
    let continued =
        round_continue_input("本轮汇报……\n【结论：继续迭代】\n- 翅膀握住车把\n- 腿自然弯曲")
            .expect("继续迭代必须被识别");
    assert!(continued.contains("翅膀握住车把"), "{continued}");
    assert!(continued.contains("腿自然弯曲"), "{continued}");

    let empty_input = round_continue_input("【结论：继续迭代】").expect("允许无问题清单");
    assert!(empty_input.is_empty(), "{empty_input}");

    assert!(round_continue_input("【结论：达标】").is_none());
    assert!(round_continue_input("【结论：通过】").is_none());
    assert!(round_continue_input("本轮完成，没有问题。").is_none());
}
