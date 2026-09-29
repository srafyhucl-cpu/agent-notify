//! 任务信封（§4.3）：派活时不发用户原话，只发针对当前 Step 的信封。
//!
//! 模板机制：`WorkflowStep.harness_template` 非空 → 用户模板胜出；空/未配置 → 内置默认模板兜底。
//! 占位符表驱动替换：{goal} {workflow_name} {role} {agent_hint} {step_index} {step_total} {next_role}。
//! 汇总信封（项目经理回流，§4）另用 {reports} 占位符：各步骤产出汇总块。
//! 本实现是纯文本占位符替换，不存在解析失败路径；未知占位符原样保留，
//! 异常内容只影响该任务的信封文案，不影响系统。

use crate::task::StepReport;
use crate::workflow::{Workflow, WorkflowStep};

/// 内置默认信封模板（未知角色的通用兜底，随版本发布）。
pub const DEFAULT_ENVELOPE_TEMPLATE: &str = concat!(
    "【任务：{goal}】\n",
    "────────────────────────\n",
    "工作流：{workflow_name}（第 {round} 轮 · Step {step_index}/{step_total}）\n",
    "你的角色：{role}（建议 Agent：{agent_hint}）\n",
    "你的活：只做本步职责，输出结论供下一步使用\n",
    "你不能：推进到下一步（由编排层负责），也不要提前实现后续步骤\n",
    "────────────────────────\n",
    "完成本步后请汇报。{next_role}",
);

/// 首节点（初步判断 / 项目经理）信封：定方向、给决策与风险，不做实现。
pub const ORCHESTRATOR_ENVELOPE_TEMPLATE: &str = concat!(
    "【任务：{goal}】\n",
    "────────────────────────\n",
    "工作流：{workflow_name}（第 {round} 轮 · Step {step_index}/{step_total}）\n",
    "你的角色：初步判断（本任务的项目经理；建议 Agent：{agent_hint}）\n",
    "你的活：\n",
    "1) 读懂目标：明确要交付什么、验收标准是什么；\n",
    "2) 给出技术路线与关键决策（选型、范围边界、依赖与风险）；\n",
    "3) 给下一步一份可执行的输入（先做什么、按什么标准算完成）。\n",
    "你不能：直接开始写实现代码；也不要推进到下一步（由编排层负责）。\n",
    "────────────────────────\n",
    "完成本步后请汇报：结论、关键决策、风险、给下一步的输入。{next_role}",
);

/// 规划节点信封：把目标拆成可执行清单与验收口径。
pub const PLANNER_ENVELOPE_TEMPLATE: &str = concat!(
    "【任务：{goal}】\n",
    "────────────────────────\n",
    "工作流：{workflow_name}（第 {round} 轮 · Step {step_index}/{step_total}）\n",
    "你的角色：规划整理（建议 Agent：{agent_hint}）\n",
    "你的活：\n",
    "1) 把目标拆成可执行的任务清单：每项写清产出物与验收口径；\n",
    "2) 写清实现方案：改动哪些文件/目录、依赖与约束、执行顺序；\n",
    "3) 标注风险与不确定点（写明假设，不要猜）。\n",
    "你不能：开始写实现；也不要推进到下一步（由编排层负责）。\n",
    "────────────────────────\n",
    "完成本步后请汇报：任务清单、实现方案、验收口径、风险。{next_role}",
);

/// 实施节点信封：按方案实现并自检（第 1 步没有前序方案时按目标直接做，2/3/4 步模板通用）。
pub const EXECUTOR_ENVELOPE_TEMPLATE: &str = concat!(
    "【任务：{goal}】\n",
    "────────────────────────\n",
    "工作流：{workflow_name}（第 {round} 轮 · Step {step_index}/{step_total}）\n",
    "你的角色：实施（建议 Agent：{agent_hint}）\n",
    "你的活：\n",
    "1) 按本步要求实现：有前序方案就照方案做，没有前序产出就按目标与验收口径直接做；只改本步范围内的内容；\n",
    "2) 自己先验证（运行/自检），修掉明显问题再汇报；\n",
    "3) 与方案有偏离时，写清偏离点与原因。\n",
    "你不能：擅自扩大范围；也不要推进到下一步（由编排层负责）。\n",
    "────────────────────────\n",
    "完成本步后请汇报：改了什么/产出在哪、怎么验证的、偏离与遗留问题。{next_role}",
);

/// 复核节点信封：对照目标与验收口径给结论，不替实施方改。
pub const REVIEWER_ENVELOPE_TEMPLATE: &str = concat!(
    "【任务：{goal}】\n",
    "────────────────────────\n",
    "工作流：{workflow_name}（第 {round} 轮 · Step {step_index}/{step_total}）\n",
    "你的角色：复核（建议 Agent：{agent_hint}）\n",
    "你的活：\n",
    "1) 对照目标与验收口径逐条检查（需求覆盖、正确性、边界情况）；\n",
    "2) 给出明确结论：通过 / 有条件通过 / 不通过；\n",
    "3) 问题按严重程度列出：写清位置与返工建议（不要自己动手改）。\n",
    "你不能：代替实施方修改；也不要推进到下一步（由编排层负责）。\n",
    "────────────────────────\n",
    "完成本步后请汇报：结论、问题清单（按严重度）、返工建议。{next_role}",
);

/// 角色 → 内置默认信封模板（未知角色用通用模板兜底；模板机制与公开的
/// planner / executor / critic 角色分工一致，只约束职责边界，不限制具体做法）。
pub fn default_envelope_template(role: &str) -> &'static str {
    match role {
        "orchestrator" => ORCHESTRATOR_ENVELOPE_TEMPLATE,
        "planner" => PLANNER_ENVELOPE_TEMPLATE,
        "executor" => EXECUTOR_ENVELOPE_TEMPLATE,
        "reviewer" => REVIEWER_ENVELOPE_TEMPLATE,
        _ => DEFAULT_ENVELOPE_TEMPLATE,
    }
}

/// 内置默认「汇总信封」模板（§4 项目经理回流）：最后一步完成后发给首节点（项目经理），
/// 汇总各步产出并向用户做最终汇报；要求给出「本轮判定」标记，供编排层决定是否继续迭代。
/// 用户可在 `harness-templates.json` 按工作流覆盖。
pub const DEFAULT_SUMMARY_ENVELOPE_TEMPLATE: &str = concat!(
    "【任务：{goal}】\n",
    "────────────────────────\n",
    "工作流：{workflow_name}（第 {round} 轮 · 共 {step_total} 步，均已执行完毕）\n",
    "各步骤产出如下：\n",
    "{reports}\n",
    "────────────────────────\n",
    "你是本次任务的项目经理：请汇总以上全部产出，向用户做最终汇报。\n",
    "汇报要求：\n",
    "1) 结论先行：做到什么程度、是否达标；\n",
    "2) 关键产出与位置（文件路径/结果）；\n",
    "3) 遗留问题与风险；\n",
    "4) 最后单独一行给出本轮判定：【结论：达标】或【结论：继续迭代】；\n",
    "   若判定继续迭代，请在判定行下面列出下一轮要解决的问题（一行一条）。\n",
    "不要重复执行各步骤的工作，也不要开始新任务。完成汇总后请汇报。",
);

/// 项目经理判定「继续迭代」的标记（汇总信封要求输出；解析见 [`round_continue_input`]）。
pub const ROUND_VERDICT_CONTINUE: &str = "【结论：继续迭代】";
/// 项目经理判定「达标」的标记（可省略；无标记按达标处理，不猜着继续）。
pub const ROUND_VERDICT_DONE: &str = "【结论：达标】";

/// 无正文步骤在汇总信封中的标注（§4：手动推进没有正文）。
pub const SUMMARY_REPORT_MISSING: &str = "（该步无正文汇报）";

/// 占位符（命名常量，表驱动，避免魔法字符串散落）。
pub const PH_GOAL: &str = "{goal}";
pub const PH_WORKFLOW_NAME: &str = "{workflow_name}";
pub const PH_ROLE: &str = "{role}";
pub const PH_AGENT_HINT: &str = "{agent_hint}";
pub const PH_STEP_INDEX: &str = "{step_index}";
pub const PH_STEP_TOTAL: &str = "{step_total}";
pub const PH_NEXT_ROLE: &str = "{next_role}";
/// 当前轮次（第 N 轮；迭代循环从 1 起）。
pub const PH_ROUND: &str = "{round}";
/// 各步骤产出汇总块（仅汇总信封填充；步骤信封中不会被替换）。
pub const PH_REPORTS: &str = "{reports}";

/// 渲染任务信封（纯函数，可单测）。
///
/// - `workflow` / `step`：当前工作流与步骤（角色、序号、模板来源）；
/// - `goal`：任务目标（用户原话）；
/// - `next_role`：下一步的角色（最后一步传 `None`，信封提示任务将完成）；
/// - `round`：当前轮次（第 N 轮迭代，从 1 起）。
pub fn render_envelope(
    workflow: &Workflow,
    step: &WorkflowStep,
    goal: &str,
    next_role: Option<&str>,
    round: u32,
) -> String {
    let template = match step.harness_template.as_deref() {
        Some(t) if !t.trim().is_empty() => t,
        _ => default_envelope_template(&step.role),
    };
    render_with_template(workflow, step, goal, next_role, round, template)
}

/// 按给定模板渲染信封（模板选择由调用方完成；`render_envelope` 与
/// [`crate::template::TemplateResolver`] 共用本实现，保证替换行为完全一致）。
pub(crate) fn render_with_template(
    workflow: &Workflow,
    step: &WorkflowStep,
    goal: &str,
    next_role: Option<&str>,
    round: u32,
    template: &str,
) -> String {
    let table = placeholder_table(workflow, step, goal, next_role, round);
    substitute(template, &table)
}

/// 占位符替换表（表驱动构建：值随工作流/步骤/运行时输入变化）。
fn placeholder_table(
    workflow: &Workflow,
    step: &WorkflowStep,
    goal: &str,
    next_role: Option<&str>,
    round: u32,
) -> Vec<(&'static str, String)> {
    let agent_hint = match step.agent_hint.as_deref() {
        Some(hint) => hint.to_string(),
        None => "未指定".to_string(),
    };
    let next = match next_role {
        Some(role) => format!("下一步：{role} 接手。"),
        None => "已是最后一步，汇报后任务即完成。".to_string(),
    };
    vec![
        (PH_GOAL, goal.to_string()),
        (PH_WORKFLOW_NAME, workflow.name.clone()),
        (PH_ROLE, step.role.clone()),
        (PH_AGENT_HINT, agent_hint),
        (PH_STEP_INDEX, step.order.to_string()),
        (PH_STEP_TOTAL, workflow.max_order().to_string()),
        (PH_NEXT_ROLE, next),
        (PH_ROUND, round.to_string()),
    ]
}

/// 按表顺序替换模板中的已知占位符；未知占位符原样保留。
fn substitute(template: &str, table: &[(&'static str, String)]) -> String {
    let mut out = template.to_string();
    for (key, value) in table {
        out = out.replace(key, value);
    }
    out
}

/// 渲染「汇总信封」（纯函数，可单测）：模板选择由调用方完成（用户配置优先、内置兜底）。
pub(crate) fn render_summary_with_template(
    workflow: &Workflow,
    goal: &str,
    reports: &str,
    round: u32,
    template: &str,
) -> String {
    let table = vec![
        (PH_GOAL, goal.to_string()),
        (PH_WORKFLOW_NAME, workflow.name.clone()),
        (PH_STEP_TOTAL, workflow.max_order().to_string()),
        (PH_ROUND, round.to_string()),
        (PH_REPORTS, reports.to_string()),
    ];
    substitute(template, &table)
}

/// 解析项目经理的轮次判定（纯函数）：正文含「【结论：继续迭代】」时返回下一轮要解决的问题
/// （判定行之后的内容，可为空串）；「达标/完成/通过」或无判定 → None（按完成处理，不猜着继续）。
pub fn round_continue_input(body: &str) -> Option<String> {
    let marker_start = body.find("【结论：")?;
    let after = &body[marker_start..];
    let close = after.find('】')?;
    let verdict = &after["【结论：".len()..close];
    if !verdict.contains("继续") {
        return None;
    }
    Some(after[close + '】'.len_utf8()..].trim().to_string())
}

/// 按固定顺序渲染各步骤产出汇总块（汇总信封的 `{reports}` 内容）：
/// 每步输出「第 k 步 · 角色产出」标题 + 正文；无正文的步骤标注 [`SUMMARY_REPORT_MISSING`]。
/// 比工作流多出的产出记录（理论上不会出现）被忽略，避免汇总块出现无法归属的正文。
pub fn render_step_reports(workflow: &Workflow, reports: &[StepReport]) -> String {
    let mut blocks = Vec::with_capacity(workflow.steps.len());
    for step in &workflow.steps {
        let body = reports
            .iter()
            .find(|report| report.step == step.order)
            .map(|report| report.body.as_str())
            .unwrap_or(SUMMARY_REPORT_MISSING);
        blocks.push(format!(
            "【第 {} 步 · {} 产出】\n{body}",
            step.order, step.role
        ));
    }
    blocks.join("\n\n")
}
