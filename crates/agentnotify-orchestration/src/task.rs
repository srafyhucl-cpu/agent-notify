//! 编排任务（§3.2 TASK 实体）：A2A Task 为唯一事实源 + 编排语境元数据。
//!
//! 会话语义（§8.3/§8.4）：新会话 = message 的 taskId/contextId 均为 None；
//! 续聊 = 带 taskId + contextId。编排语境序列化在 A2A Task.metadata["orc"]，
//! 不建第二套事实源。

use a2a_rs_core::{Message, Part, Role, Task, TaskState, TaskStatus};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::OrcError;
use crate::workflow::Workflow;

/// 通知节奏（§4.6）：默认只推最终汇报，可切逐步流转。
/// wire 取值与设计文档 `TASK.notify_mode` 一致（final_only / verbose）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotifyMode {
    /// 只推最顶层最终汇报（默认）
    #[default]
    FinalOnly,
    /// 每个 Step 的汇报都推（逐步流转）
    Verbose,
}

impl NotifyMode {
    /// 稳定字符串取值（final_only / verbose），供呈现层使用。
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::FinalOnly => "final_only",
            Self::Verbose => "verbose",
        }
    }
}

/// 编排语境元数据：挂在 A2A `Task.metadata["orc"]` 下（键见 [`ORC_META_KEY`]）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrcMeta {
    pub workflow_id: String,
    /// 当前步骤序号（从 1 开始）
    pub current_step: u32,
    /// 被卡住的步骤（未阻塞为 None）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_step: Option<u32>,
    /// 阻塞原因（未阻塞为 None；写清哪一步失败/谁不可用/未送达）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_reason: Option<String>,
    pub notify_mode: NotifyMode,
    /// 任务目标（用户原话）
    pub goal: String,
    /// 任务名称（短名，≤8 字；用于集群列表与真实会话标题展示）。
    /// 旧任务缺省 None = 由目标推导展示（向后兼容）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// 迭代轮次（从 1 起；「继续迭代」或项目经理判定继续时 +1，回到第 1 步）。
    #[serde(default = "round_default")]
    pub round: u32,
    /// 本轮要求/上一轮结论（新一轮开始时写入；第 1 轮为 None）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub round_input: Option<String>,
    /// 是否已开始执行（人工确认后开始；创建后默认 false，避免"没看清节点就被派活"）。
    /// 旧任务（无此字段）默认 true——它们本就已在运行，保持向后兼容。
    #[serde(default = "started_default_true")]
    pub started: bool,
    /// 任务工作目录（OpenCode 会话创建位置）。旧任务缺省 = 跟随宿主当前项目。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_dir: Option<String>,
    /// 任务开始执行时的步骤配置快照（§3「改配置不影响已创建任务」）：
    /// `start` 预检通过后写入；此后派活/校验/DTO 节点展示优先用快照。
    /// 旧任务/未开始任务缺省 None = 实时合并节点配置（向后兼容）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steps_snapshot: Option<Vec<StepConfigSnapshot>>,
    /// 各步骤产出正文（回流汇总给首节点用，见 [`record_step_report`] 的双重截断）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub step_reports: Vec<StepReport>,
    /// 「项目经理汇报阶段」：最后一步已完成，等待首节点汇总成最终汇报。
    #[serde(default)]
    pub final_report_pending: bool,
}

/// 单步产出记录（回流汇总用）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepReport {
    /// 该产出所属步骤序号
    pub step: u32,
    /// 产出正文（已按上限截断）
    pub body: String,
}

/// 步骤配置快照（`start` 时锁定，§3「改配置不影响已创建任务」）。
///
/// `agent` 必为已配置的非空 Agent id（`start` 预检保证）；`model` 可选（None = 该 Agent 默认模型）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepConfigSnapshot {
    /// 步骤序号（与工作流步骤一一对应）
    pub order: u32,
    /// 任务内角色（展示/校验用；模板固定角色的副本）
    pub role: String,
    /// 该步 Agent（非空）
    pub agent: String,
    /// 该步模型（`provider/model`；None = 该 Agent 默认模型）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// 单步产出记录上限（字符数；超出截断并标注「…（已截断）」）。
pub const ORC_STEP_REPORT_LIMIT: usize = 4000;
/// 全部产出记录总上限（字符数；超出从最早步骤开始裁剪）。
pub const ORC_STEP_REPORTS_TOTAL_LIMIT: usize = 12000;

/// 按字符截断正文；超限时追加标注（标注不计入上限，允许少量超出）。
fn truncate_report(body: &str, limit: usize) -> String {
    if body.chars().count() <= limit {
        return body.to_string();
    }
    let head: String = body.chars().take(limit).collect();
    format!("{head}\n…（已截断）")
}

/// 总量保护：从最早步骤开始裁剪，直到总字符数不超过 [`ORC_STEP_REPORTS_TOTAL_LIMIT`]。
fn trim_total_step_reports(reports: &mut Vec<StepReport>) {
    let mut total: usize = reports
        .iter()
        .map(|report| report.body.chars().count())
        .sum();
    while total > ORC_STEP_REPORTS_TOTAL_LIMIT && !reports.is_empty() {
        let excess = total - ORC_STEP_REPORTS_TOTAL_LIMIT;
        let first_len = reports[0].body.chars().count();
        if first_len <= excess {
            total -= first_len;
            reports.remove(0);
        } else {
            reports[0].body = truncate_report(&reports[0].body, first_len - excess);
            total = ORC_STEP_REPORTS_TOTAL_LIMIT;
        }
    }
}

/// `started` 的兼容默认值：旧任务视为已开始（保持历史行为）。
fn started_default_true() -> bool {
    true
}

/// 轮次缺省值：旧任务没有该字段 = 第 1 轮。
fn round_default() -> u32 {
    1
}

/// A2A Task.metadata 中编排语境所在的键。
pub const ORC_META_KEY: &str = "orc";

/// 编排任务：A2A Task 包装 + 工作流语境（workflow_id/current_step/notify_mode/blocked 等）。
///
/// 语境以 [`OrcMeta`] 存在 `a2a_task.metadata["orc"]`，读写都经本包装，
/// 保证 A2A Task 是唯一事实源（§8.3「不建第二套」）。
#[derive(Debug, Clone, PartialEq)]
pub struct OrcTask {
    pub a2a_task: Task,
}

impl OrcTask {
    /// 新建编排任务：当前步骤 = 第 1 步，A2A 状态 = Working，**未开始**（等人工确认后 `start`）。
    /// context_id 取任务自身 id：一个编排任务即一个逻辑会话（§3.3/§8.4）。
    pub fn new(workflow: &Workflow, goal: &str, notify_mode: NotifyMode) -> Result<Self, OrcError> {
        let id = Uuid::new_v4().to_string();
        let meta = OrcMeta {
            workflow_id: workflow.id.clone(),
            current_step: 1,
            blocked_step: None,
            block_reason: None,
            notify_mode,
            goal: goal.to_string(),
            name: None,
            round: 1,
            round_input: None,
            started: false,
            working_dir: None,
            steps_snapshot: None,
            step_reports: Vec::new(),
            final_report_pending: false,
        };
        let mut metadata = serde_json::Map::new();
        metadata.insert(
            ORC_META_KEY.to_string(),
            serde_json::to_value(meta)
                .map_err(|e| OrcError::meta_invalid(&format!("序列化失败：{e}")))?,
        );
        let a2a_task = Task {
            kind: "task".to_string(),
            id: id.clone(),
            context_id: id,
            status: TaskStatus {
                state: TaskState::Working,
                message: None,
                timestamp: None,
            },
            artifacts: None,
            history: None,
            metadata: Some(serde_json::Value::Object(metadata)),
        };
        Ok(Self { a2a_task })
    }

    /// 从 A2A Task 还原编排语境；缺 metadata/编排键/字段损坏 → 明确报错（不猜测兜底）。
    pub fn from_a2a(task: Task) -> Result<Self, OrcError> {
        let _ = Self::meta_of(&task)?;
        Ok(Self { a2a_task: task })
    }

    pub fn id(&self) -> &str {
        &self.a2a_task.id
    }

    /// 任务当前 A2A 状态（唯一事实源）。
    pub fn state(&self) -> TaskState {
        self.a2a_task.status.state
    }

    pub fn set_state(&mut self, state: TaskState) {
        self.a2a_task.status.state = state;
    }

    pub fn meta(&self) -> Result<OrcMeta, OrcError> {
        Self::meta_of(&self.a2a_task)
    }

    pub fn workflow_id(&self) -> Result<String, OrcError> {
        Ok(self.meta()?.workflow_id)
    }

    pub fn current_step(&self) -> Result<u32, OrcError> {
        Ok(self.meta()?.current_step)
    }

    pub fn blocked_step(&self) -> Result<Option<u32>, OrcError> {
        Ok(self.meta()?.blocked_step)
    }

    pub fn notify_mode(&self) -> Result<NotifyMode, OrcError> {
        Ok(self.meta()?.notify_mode)
    }

    pub fn goal(&self) -> Result<String, OrcError> {
        Ok(self.meta()?.goal)
    }

    pub fn set_current_step(&mut self, step: u32) -> Result<(), OrcError> {
        let mut meta = self.meta()?;
        meta.current_step = step;
        self.put_meta(&meta)
    }

    /// 记录阻塞：step = 哪一步失败，reason = 谁不可用/未送达的原因（§4.6）。
    pub fn set_blocked(&mut self, step: u32, reason: &str) -> Result<(), OrcError> {
        let mut meta = self.meta()?;
        meta.blocked_step = Some(step);
        meta.block_reason = Some(reason.to_string());
        self.put_meta(&meta)
    }

    /// 清除阻塞标记（人工重新发起后）。
    pub fn clear_blocked(&mut self) -> Result<(), OrcError> {
        let mut meta = self.meta()?;
        meta.blocked_step = None;
        meta.block_reason = None;
        self.put_meta(&meta)
    }

    /// 是否已开始执行（创建后需人工确认 `start` 才派活；旧任务视为已开始）。
    pub fn is_started(&self) -> Result<bool, OrcError> {
        Ok(self.meta()?.started)
    }

    /// 标记为已开始执行（幂等；`start` 命令在派活前调用）。
    pub fn mark_started(&mut self) -> Result<(), OrcError> {
        let mut meta = self.meta()?;
        if meta.started {
            return Ok(());
        }
        meta.started = true;
        self.put_meta(&meta)
    }

    /// 任务工作目录（None = 跟随宿主当前项目；旧任务缺省）。
    pub fn working_dir(&self) -> Result<Option<String>, OrcError> {
        Ok(self.meta()?.working_dir)
    }

    /// 任务名称（None = 旧任务缺省，由展示侧按目标推导）。
    pub fn name(&self) -> Result<Option<String>, OrcError> {
        Ok(self.meta()?.name)
    }

    /// 设置任务名称（空串 = 清除；长度校验由调用方负责）。
    pub fn set_name(&mut self, name: &str) -> Result<(), OrcError> {
        let mut meta = self.meta()?;
        let trimmed = name.trim();
        meta.name = if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        };
        self.put_meta(&meta)
    }

    /// 更新任务目标（仅未开始任务允许；由调用方校验后写入）。
    pub fn set_goal(&mut self, goal: &str) -> Result<(), OrcError> {
        let mut meta = self.meta()?;
        meta.goal = goal.trim().to_string();
        self.put_meta(&meta)
    }

    /// 更新通知节奏（运行中改只影响后续推送；由调用方解析后传入）。
    pub fn set_notify_mode(&mut self, mode: NotifyMode) -> Result<(), OrcError> {
        let mut meta = self.meta()?;
        meta.notify_mode = mode;
        self.put_meta(&meta)
    }

    /// 当前迭代轮次（旧任务缺省 1）。
    pub fn round(&self) -> Result<u32, OrcError> {
        Ok(self.meta()?.round.max(1))
    }

    /// 本轮要求/上一轮结论（第 1 轮为 None）。
    pub fn round_input(&self) -> Result<Option<String>, OrcError> {
        Ok(self.meta()?.round_input)
    }

    /// 开始新一轮迭代（用户「继续迭代」或项目经理判定继续）：
    /// 轮次 +1、写入本轮要求、回到第 1 步、退出汇总/阻塞，状态回到 Working。
    /// 调用方负责校验「本轮已结束」等前置条件。
    pub fn begin_round(&mut self, input: Option<&str>) -> Result<(), OrcError> {
        let mut meta = self.meta()?;
        meta.round = meta.round.max(1).saturating_add(1);
        meta.round_input = input
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        meta.current_step = 1;
        meta.final_report_pending = false;
        meta.blocked_step = None;
        meta.block_reason = None;
        self.put_meta(&meta)?;
        self.set_state(TaskState::Working);
        Ok(())
    }

    /// 设置任务工作目录（创建任务时由宿主写入；空串 = 清除）。
    pub fn set_working_dir(&mut self, dir: &str) -> Result<(), OrcError> {
        let mut meta = self.meta()?;
        let trimmed = dir.trim();
        meta.working_dir = if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        };
        self.put_meta(&meta)
    }

    /// 步骤配置快照（start 时锁定；未快照的旧任务/未开始任务为 None = 实时合并）。
    pub fn steps_snapshot(&self) -> Result<Option<Vec<StepConfigSnapshot>>, OrcError> {
        Ok(self.meta()?.steps_snapshot)
    }

    /// 写入步骤配置快照（start 预检通过后写入）；空切片 = 清除快照（回退实时合并）。
    pub fn set_steps_snapshot(&mut self, snapshot: &[StepConfigSnapshot]) -> Result<(), OrcError> {
        let mut meta = self.meta()?;
        meta.steps_snapshot = if snapshot.is_empty() {
            None
        } else {
            Some(snapshot.to_vec())
        };
        self.put_meta(&meta)
    }

    /// 记录某步产出正文（回流汇总用）：单步与总量双重截断；同一步重复上报覆盖旧值。
    pub fn record_step_report(&mut self, step: u32, body: &str) -> Result<(), OrcError> {
        let mut meta = self.meta()?;
        let truncated = truncate_report(body.trim(), ORC_STEP_REPORT_LIMIT);
        if let Some(existing) = meta
            .step_reports
            .iter_mut()
            .find(|report| report.step == step)
        {
            existing.body = truncated;
        } else {
            meta.step_reports.push(StepReport {
                step,
                body: truncated,
            });
            meta.step_reports.sort_by_key(|report| report.step);
        }
        trim_total_step_reports(&mut meta.step_reports);
        self.put_meta(&meta)
    }

    /// 某步的产出正文（未记录为 None）。
    pub fn step_report(&self, step: u32) -> Result<Option<String>, OrcError> {
        Ok(self
            .meta()?
            .step_reports
            .into_iter()
            .find(|report| report.step == step)
            .map(|report| report.body))
    }

    /// 是否处于「项目经理汇报阶段」（最后一步已完成，等待首节点汇总）。
    pub fn is_finalizing(&self) -> Result<bool, OrcError> {
        Ok(self.meta()?.final_report_pending)
    }

    /// 进入/退出「项目经理汇报阶段」。
    pub fn set_final_report_pending(&mut self, pending: bool) -> Result<(), OrcError> {
        let mut meta = self.meta()?;
        meta.final_report_pending = pending;
        self.put_meta(&meta)
    }

    fn meta_of(task: &Task) -> Result<OrcMeta, OrcError> {
        let metadata = task
            .metadata
            .as_ref()
            .ok_or_else(|| OrcError::meta_invalid("metadata 缺失"))?;
        let orc = metadata
            .get(ORC_META_KEY)
            .ok_or_else(|| OrcError::meta_invalid(&format!("缺少 {ORC_META_KEY} 键")))?;
        serde_json::from_value(orc.clone())
            .map_err(|e| OrcError::meta_invalid(&format!("解析失败：{e}")))
    }

    fn put_meta(&mut self, meta: &OrcMeta) -> Result<(), OrcError> {
        let value = serde_json::to_value(meta)
            .map_err(|e| OrcError::meta_invalid(&format!("序列化失败：{e}")))?;
        let metadata = self
            .a2a_task
            .metadata
            .get_or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
        let map = metadata
            .as_object_mut()
            .ok_or_else(|| OrcError::meta_invalid("metadata 不是 JSON 对象"))?;
        map.insert(ORC_META_KEY.to_string(), value);
        Ok(())
    }
}

/// 新会话消息（A2A 语义 = 发起新任务/新会话）：task_id/context_id 均为 None，wire 上不出现。
pub fn new_session_message(role: Role, text: impl Into<String>) -> Message {
    Message {
        kind: "message".to_string(),
        message_id: Uuid::new_v4().to_string(),
        context_id: None,
        task_id: None,
        role,
        parts: vec![Part::text(text)],
        metadata: None,
        extensions: vec![],
        reference_task_ids: None,
    }
}

/// 续聊消息：带 task_id + context_id（wire 名 taskId/contextId，§8.4）。
pub fn continue_message(
    role: Role,
    text: impl Into<String>,
    task_id: &str,
    context_id: &str,
) -> Message {
    Message {
        kind: "message".to_string(),
        message_id: Uuid::new_v4().to_string(),
        context_id: Some(context_id.to_string()),
        task_id: Some(task_id.to_string()),
        role,
        parts: vec![Part::text(text)],
        metadata: None,
        extensions: vec![],
        reference_task_ids: None,
    }
}
