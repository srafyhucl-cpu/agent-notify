//! 可配置工作流（§3.1.1 / §3.2）：阶段流水线模型 + 预置模板。
//!
//! 工作流 = 用户可配置的阶段序列；每个 Step 只关心「角色 + 建议 Agent + 信封模板 + 人工确认门」，
//! 允许不同 Step 复用同一 Agent（会话隔离是编排层细节，节点配置里只有 agent_hint）。

use crate::error::OrcError;

/// 角色常量：orchestrator=初步判断，planner=规划拆解，executor=实施，reviewer=复核。
/// 角色是任务内本地属性，不绑定 Agent 全局身份（用户可自定义新角色）。
pub const ROLE_ORCHESTRATOR: &str = "orchestrator";
pub const ROLE_PLANNER: &str = "planner";
pub const ROLE_EXECUTOR: &str = "executor";
pub const ROLE_REVIEWER: &str = "reviewer";

/// 预置工作流的建议 Agent（每个节点均可自由改选，允许复用同一 Agent）。
pub const AGENT_HINT_CODEX: &str = "codex";
pub const AGENT_HINT_OPENCODE: &str = "opencode";
pub const AGENT_HINT_COMMANDCODE: &str = "commandcode";

/// 预置工作流步骤序号（命名常量，避免魔法数字）。
pub const PRESET_ORDER_JUDGE: u32 = 1;
pub const PRESET_ORDER_PLAN: u32 = 2;
pub const PRESET_ORDER_EXECUTE: u32 = 3;
/// 可选第 4 步：复核/汇总（预置默认关闭）
pub const PRESET_ORDER_REVIEW: u32 = 4;

/// 预置工作流标识与名称。
pub const PRESET_WORKFLOW_ID: &str = "preset-requirement-to-report";
pub const PRESET_WORKFLOW_NAME: &str = "需求→判断→规划→实施";
/// 旧版「只用 OpenCode」预设 id（仅兼容已存在任务，新任务不再开放）。
pub const PRESET_OPENCODE_ONLY_ID: &str = "preset-opencode-only";

/// 固定模板「快速修复」标识与名称（P3：新任务只开放三档模板，节点 Agent/模型由用户配置）。
pub const TEMPLATE_QUICKFIX_ID: &str = "template-quickfix";
pub const TEMPLATE_QUICKFIX_NAME: &str = "快速修复";
/// 固定模板「标准交付」标识与名称（推荐默认）。
pub const TEMPLATE_STANDARD_ID: &str = "template-standard";
pub const TEMPLATE_STANDARD_NAME: &str = "标准交付";
/// 固定模板「完整评估」标识与名称。
pub const TEMPLATE_FULL_ID: &str = "template-full";
pub const TEMPLATE_FULL_NAME: &str = "完整评估";
/// 新任务可选的内置模板 id（顺序即 UI 展示顺序）。
pub const TEMPLATE_IDS: [&str; 3] = [TEMPLATE_QUICKFIX_ID, TEMPLATE_STANDARD_ID, TEMPLATE_FULL_ID];

/// 单个工作流步骤（§3.2 WORKFLOW_STEP）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowStep {
    /// 第几步（从 1 开始，连续递增）
    pub order: u32,
    /// 任务内角色（orchestrator/planner/executor/reviewer 或用户自定义）
    pub role: String,
    /// 建议 Agent（可为空；同一 Agent 被多个 Step 选中时，编排层按 Step 隔离会话）
    pub agent_hint: Option<String>,
    /// 该步使用的模型（形如 `provider/model`；空 = 用该 Agent 的默认模型）。
    /// P3 起由用户按节点配置（settings `orchestration.node_config`），内置模板不预置。
    pub model: Option<String>,
    /// 该步的任务信封模板（空 = 用内置默认模板兜底，§4.3）
    pub harness_template: Option<String>,
    /// 是否需人确认才可推进下一步（human_gate）
    pub human_gate: bool,
}

impl WorkflowStep {
    /// 构造单步。`harness_template` 空 = 内置默认信封；`human_gate=true` 需人确认进下一步。
    /// `model` 默认空（用该 Agent 默认模型），需要时用 [`WorkflowStep::with_model`] 设置。
    pub fn new(
        order: u32,
        role: impl Into<String>,
        agent_hint: Option<String>,
        harness_template: Option<String>,
        human_gate: bool,
    ) -> Self {
        Self {
            order,
            role: role.into(),
            agent_hint,
            model: None,
            harness_template,
            human_gate,
        }
    }

    /// 设置该步模型（`provider/model`），链式构造用。
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }
}

/// 可配置工作流：任务按步骤逐个推进的阶段流水线。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workflow {
    pub id: String,
    pub name: String,
    /// 唯一阶段来源（TASK.current_step 挂 WORKFLOW_STEP.order）
    pub steps: Vec<WorkflowStep>,
}

impl Workflow {
    /// 校验并构造：步骤非空、序号从 1 连续递增；不满足则明确报错。
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        steps: Vec<WorkflowStep>,
    ) -> Result<Self, OrcError> {
        let id = id.into();
        if steps.is_empty() {
            return Err(OrcError::workflow_empty(&id));
        }
        for (index, step) in steps.iter().enumerate() {
            let expect = (index + 1) as u32;
            if step.order != expect {
                return Err(OrcError::step_order_invalid(index + 1, step.order));
            }
        }
        Ok(Self {
            id,
            name: name.into(),
            steps,
        })
    }

    /// 预置工作流「需求 → Codex 判断 → OpenCode 规划 → CommandCode 实施 →（可选复核）」。
    ///
    /// - 阶段 1–3（判断/规划/实施）：无人工确认门，汇报即推进；
    /// - `include_reviewer=true` 追加阶段 4「复核/汇总」，默认关闭；复核步开启人工确认门
    ///   （任务最终汇报 + 人确认后完成）。
    pub fn preset(include_reviewer: bool) -> Result<Self, OrcError> {
        let mut steps = vec![
            WorkflowStep::new(
                PRESET_ORDER_JUDGE,
                ROLE_ORCHESTRATOR,
                Some(AGENT_HINT_CODEX.to_string()),
                None,
                false,
            ),
            WorkflowStep::new(
                PRESET_ORDER_PLAN,
                ROLE_PLANNER,
                Some(AGENT_HINT_OPENCODE.to_string()),
                None,
                false,
            ),
            WorkflowStep::new(
                PRESET_ORDER_EXECUTE,
                ROLE_EXECUTOR,
                Some(AGENT_HINT_COMMANDCODE.to_string()),
                None,
                false,
            ),
        ];
        if include_reviewer {
            steps.push(WorkflowStep::new(
                PRESET_ORDER_REVIEW,
                ROLE_REVIEWER,
                None,
                None,
                true,
            ));
        }
        Self::new(PRESET_WORKFLOW_ID, PRESET_WORKFLOW_NAME, steps)
    }

    /// 只用 OpenCode 的单 Agent 工作流：三步判断/规划/实施全部由 opencode 承担
    /// （节点自由选 Agent，允许复用同一 Agent；§3.1 编排与 Agent 数量解耦，R11）。
    pub fn preset_opencode_only() -> Result<Self, OrcError> {
        let steps = vec![
            WorkflowStep::new(
                PRESET_ORDER_JUDGE,
                ROLE_ORCHESTRATOR,
                Some(AGENT_HINT_OPENCODE.to_string()),
                None,
                false,
            ),
            WorkflowStep::new(
                PRESET_ORDER_PLAN,
                ROLE_PLANNER,
                Some(AGENT_HINT_OPENCODE.to_string()),
                None,
                false,
            ),
            WorkflowStep::new(
                PRESET_ORDER_EXECUTE,
                ROLE_EXECUTOR,
                Some(AGENT_HINT_OPENCODE.to_string()),
                None,
                false,
            ),
        ];
        Self::new(
            PRESET_OPENCODE_ONLY_ID,
            "OpenCode 三步流转（判断→规划→实施）",
            steps,
        )
    }

    /// 固定模板「快速修复」（2 步）：实施 → 复核。
    pub fn template_quickfix() -> Result<Self, OrcError> {
        let steps = vec![
            WorkflowStep::new(1, ROLE_EXECUTOR, None, None, false),
            WorkflowStep::new(2, ROLE_REVIEWER, None, None, false),
        ];
        Self::new(TEMPLATE_QUICKFIX_ID, TEMPLATE_QUICKFIX_NAME, steps)
    }

    /// 固定模板「标准交付」（3 步，推荐默认）：规划 → 实施 → 复核。
    pub fn template_standard() -> Result<Self, OrcError> {
        let steps = vec![
            WorkflowStep::new(1, ROLE_PLANNER, None, None, false),
            WorkflowStep::new(2, ROLE_EXECUTOR, None, None, false),
            WorkflowStep::new(3, ROLE_REVIEWER, None, None, false),
        ];
        Self::new(TEMPLATE_STANDARD_ID, TEMPLATE_STANDARD_NAME, steps)
    }

    /// 固定模板「完整评估」（4 步）：判断 → 规划 → 实施 → 复核。
    pub fn template_full() -> Result<Self, OrcError> {
        let steps = vec![
            WorkflowStep::new(1, ROLE_ORCHESTRATOR, None, None, false),
            WorkflowStep::new(2, ROLE_PLANNER, None, None, false),
            WorkflowStep::new(3, ROLE_EXECUTOR, None, None, false),
            WorkflowStep::new(4, ROLE_REVIEWER, None, None, false),
        ];
        Self::new(TEMPLATE_FULL_ID, TEMPLATE_FULL_NAME, steps)
    }

    /// 按 id 解析内置工作流（三档固定模板 + 旧预设兼容）；未知 id 返回 None。
    ///
    /// 注：`preset-requirement-to-report` 恒解析为不带复核的 3 步版本（生产装配形态；
    /// 带复核的 4 步变体仅测试使用，历史上无持久化任务依赖它）。
    pub fn builtin(id: &str) -> Option<Self> {
        let built = match id {
            TEMPLATE_QUICKFIX_ID => Self::template_quickfix(),
            TEMPLATE_STANDARD_ID => Self::template_standard(),
            TEMPLATE_FULL_ID => Self::template_full(),
            PRESET_WORKFLOW_ID => Self::preset(false),
            PRESET_OPENCODE_ONLY_ID => Self::preset_opencode_only(),
            _ => return None,
        };
        // 内置模板是静态常量组合，构造失败属于代码缺陷；测试逐一覆盖。
        built.ok()
    }

    /// 最后一步的序号（= 步骤数）。
    pub fn max_order(&self) -> u32 {
        self.steps.len() as u32
    }

    /// 按序号取步骤；越界（含 order=0）返回 None。
    pub fn step(&self, order: u32) -> Option<&WorkflowStep> {
        self.steps.get((order as usize).checked_sub(1)?)
    }

    /// 下一步骤；当前已是最后一步则返回 None。
    pub fn next_step(&self, order: u32) -> Option<&WorkflowStep> {
        self.step(order + 1)
    }

    /// 是否为最后一步。
    pub fn is_last(&self, order: u32) -> bool {
        order == self.max_order()
    }
}
