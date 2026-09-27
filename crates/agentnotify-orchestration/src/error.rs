//! 编排层统一错误：稳定错误码 + 中文可读消息（SafeError 风格）。
//!
//! 约定：`code` 稳定、可程序化判断；`message` 面向用户（微信/桌面端可直接展示），
//! 失败一律明确暴露，不做猜测式兜底。

use std::fmt;

/// 稳定错误码：对外可见，不随文案变动。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OrcErrorCode {
    /// 工作流没有任何步骤
    WorkflowEmpty,
    /// 工作流步骤序号不连续
    InvalidStepOrder,
    /// 当前步骤超出工作流范围
    StepNotFound,
    /// 任务不存在
    TaskNotFound,
    /// 任务元数据缺失或损坏
    MetaInvalid,
    /// 任务状态无法映射到 Step 状态机
    InvalidTaskState,
    /// 未收到汇报先确认
    ConfirmBeforeReport,
    /// 任务已阻塞，需人工重新发起
    BlockedAwaitingResume,
    /// 解除未阻塞的任务
    TaskNotBlocked,
    /// 已终止的任务不允许标记阻塞
    CannotBlockTerminal,
    /// 状态机未覆盖的非法转移
    InvalidTransition,
}

impl OrcErrorCode {
    /// 稳定错误码字符串（`orc.*` 前缀，可进日志/协议/呈现层判断）。
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::WorkflowEmpty => "orc.workflow_empty",
            Self::InvalidStepOrder => "orc.step_order_invalid",
            Self::StepNotFound => "orc.step_not_found",
            Self::TaskNotFound => "orc.task_not_found",
            Self::MetaInvalid => "orc.meta_invalid",
            Self::InvalidTaskState => "orc.invalid_task_state",
            Self::ConfirmBeforeReport => "orc.confirm_before_report",
            Self::BlockedAwaitingResume => "orc.blocked_awaiting_resume",
            Self::TaskNotBlocked => "orc.task_not_blocked",
            Self::CannotBlockTerminal => "orc.cannot_block_terminal",
            Self::InvalidTransition => "orc.invalid_transition",
        }
    }
}

impl fmt::Display for OrcErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 编排层错误。`message` 为中文、面向用户；`code` 稳定用于程序化判断。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrcError {
    pub code: OrcErrorCode,
    pub message: String,
}

impl OrcError {
    pub fn new(code: OrcErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn workflow_empty(id: &str) -> Self {
        Self::new(
            OrcErrorCode::WorkflowEmpty,
            format!("工作流 {id} 没有任何步骤，无法创建任务"),
        )
    }

    pub fn step_order_invalid(index: usize, order: u32) -> Self {
        Self::new(
            OrcErrorCode::InvalidStepOrder,
            format!("工作流步骤序号必须从 1 连续递增：第 {index} 个步骤的序号为 {order}"),
        )
    }

    pub fn step_not_found(order: u32) -> Self {
        Self::new(
            OrcErrorCode::StepNotFound,
            format!("工作流中不存在第 {order} 步"),
        )
    }

    pub fn task_not_found(id: &str) -> Self {
        Self::new(OrcErrorCode::TaskNotFound, format!("任务不存在：{id}"))
    }

    pub fn meta_invalid(context: &str) -> Self {
        Self::new(
            OrcErrorCode::MetaInvalid,
            format!("任务编排元数据缺失或损坏：{context}"),
        )
    }

    pub fn invalid_task_state(state: &str) -> Self {
        Self::new(
            OrcErrorCode::InvalidTaskState,
            format!("任务当前状态 {state} 无法映射到正在进行的步骤"),
        )
    }

    pub fn confirm_before_report() -> Self {
        Self::new(
            OrcErrorCode::ConfirmBeforeReport,
            "尚未收到当前步骤的汇报，确认无效".to_string(),
        )
    }

    pub fn blocked_awaiting_resume() -> Self {
        Self::new(
            OrcErrorCode::BlockedAwaitingResume,
            "任务已阻塞，需人工处理后重新发起".to_string(),
        )
    }

    pub fn task_not_blocked(id: &str) -> Self {
        Self::new(
            OrcErrorCode::TaskNotBlocked,
            format!("任务 {id} 未处于阻塞状态，无法恢复"),
        )
    }

    pub fn cannot_block_terminal(id: &str) -> Self {
        Self::new(
            OrcErrorCode::CannotBlockTerminal,
            format!("任务 {id} 已终止，不允许标记为阻塞"),
        )
    }

    pub fn invalid_transition(reason: &str) -> Self {
        Self::new(
            OrcErrorCode::InvalidTransition,
            format!("非法状态转移：{reason}"),
        )
    }
}

impl fmt::Display for OrcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for OrcError {}
