//! 任务信封（§4.3）：派活时不发用户原话，只发针对当前 Step 的信封。
//!
//! 模板机制：`WorkflowStep.harness_template` 非空 → 用户模板胜出；空/未配置 → 内置默认模板兜底。
//! 占位符表驱动替换：{goal} {workflow_name} {role} {agent_hint} {step_index} {step_total} {next_role}。
//! 汇总信封（项目经理回流，§4）另用 {reports} 占位符：各步骤产出汇总块。
//! 本实现是纯文本占位符替换，不存在解析失败路径；未知占位符原样保留，
//! 异常内容只影响该任务的信封文案，不影响系统。

use crate::task::StepReport;
use crate::workflow::{Workflow, WorkflowStep};

/// 内置默认信封模板（§4.3 兜底，随版本发布）。
pub const DEFAULT_ENVELOPE_TEMPLATE: &str = concat!(
    "【任务：{goal}】\n",
    "────────────────────────\n",
    "工作流：{workflow_name}（Step {step_index}/{step_total}）\n",
    "你的角色：{role}（建议 Agent：{agent_hint}）\n",
    "你的活：只做本步职责，输出结论供下一步使用\n",
    "你不能：推进到下一步（由编排层负责），也不要提前实现后续步骤\n",
    "────────────────────────\n",
    "完成本步后请汇报。{next_role}",
);

/// 内置默认「汇总信封」模板（§4 项目经理回流）：最后一步完成后发给首节点（项目经理），
/// 汇总各步产出并向用户做最终汇报。用户可在 `harness-templates.json` 按工作流覆盖。
pub const DEFAULT_SUMMARY_ENVELOPE_TEMPLATE: &str = concat!(
    "【任务：{goal}】\n",
    "────────────────────────\n",
    "工作流：{workflow_name}（共 {step_total} 步，均已执行完毕）\n",
    "各步骤产出如下：\n",
    "{reports}\n",
    "────────────────────────\n",
    "你是本次任务的项目经理：请汇总以上全部产出，向用户做最终汇报（结论、关键产出/改动、风险与下一步建议）。\n",
    "不要重复执行各步骤的工作，也不要开始新任务。\n",
    "完成汇总后请汇报。",
);

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
/// 各步骤产出汇总块（仅汇总信封填充；步骤信封中不会被替换）。
pub const PH_REPORTS: &str = "{reports}";

/// 渲染任务信封（纯函数，可单测）。
///
/// - `workflow` / `step`：当前工作流与步骤（角色、序号、模板来源）；
/// - `goal`：任务目标（用户原话）；
/// - `next_role`：下一步的角色（最后一步传 `None`，信封提示任务将完成）。
pub fn render_envelope(
    workflow: &Workflow,
    step: &WorkflowStep,
    goal: &str,
    next_role: Option<&str>,
) -> String {
    let template = match step.harness_template.as_deref() {
        Some(t) if !t.trim().is_empty() => t,
        _ => DEFAULT_ENVELOPE_TEMPLATE,
    };
    render_with_template(workflow, step, goal, next_role, template)
}

/// 按给定模板渲染信封（模板选择由调用方完成；`render_envelope` 与
/// [`crate::template::TemplateResolver`] 共用本实现，保证替换行为完全一致）。
pub(crate) fn render_with_template(
    workflow: &Workflow,
    step: &WorkflowStep,
    goal: &str,
    next_role: Option<&str>,
    template: &str,
) -> String {
    let table = placeholder_table(workflow, step, goal, next_role);
    substitute(template, &table)
}

/// 占位符替换表（表驱动构建：值随工作流/步骤/运行时输入变化）。
fn placeholder_table(
    workflow: &Workflow,
    step: &WorkflowStep,
    goal: &str,
    next_role: Option<&str>,
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
    template: &str,
) -> String {
    let table = vec![
        (PH_GOAL, goal.to_string()),
        (PH_WORKFLOW_NAME, workflow.name.clone()),
        (PH_STEP_TOTAL, workflow.max_order().to_string()),
        (PH_REPORTS, reports.to_string()),
    ];
    substitute(template, &table)
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
