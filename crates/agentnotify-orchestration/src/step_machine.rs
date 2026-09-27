//! Step 推进状态机（§4.4）：纯函数、消息驱动、无 IO，可直接单测。
//!
//! A2A 生命周期映射（§8.3）：waiting_report ↔ `TaskState::InputRequired`、blocked ↔ `TaskState::Failed`，
//! 直接用 a2a 的 `TaskState`，不另存第二套状态。

use a2a_rs_core::TaskState;

use crate::error::OrcError;

/// 单个 Step 的推进状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepState {
    /// 干活中：等该步 Agent 的汇报
    InProgress,
    /// 汇报已到、等人确认（本步 human_gate 开启）
    AwaitingConfirm,
    /// 投递失败/任务失败：卡住等人工处理，不自动重推
    Blocked,
}

impl StepState {
    /// 映射到 A2A TaskState（§8.3：waiting_report≈input-required、blocked≈failed）。
    pub fn to_task_state(&self) -> TaskState {
        match self {
            Self::InProgress => TaskState::Working,
            Self::AwaitingConfirm => TaskState::InputRequired,
            Self::Blocked => TaskState::Failed,
        }
    }

    /// 从 A2A TaskState 还原 Step 状态；只接受推进中的状态，终态（Completed 等）明确报错。
    pub fn from_task_state(state: TaskState) -> Result<Self, OrcError> {
        match state {
            TaskState::Working => Ok(Self::InProgress),
            TaskState::InputRequired => Ok(Self::AwaitingConfirm),
            TaskState::Failed => Ok(Self::Blocked),
            other => Err(OrcError::invalid_task_state(&format!("{other:?}"))),
        }
    }
}

/// 消息 kind（§4 消息总线）：汇报 / 指令 / 确认 / 提问 / 信息。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageKind {
    /// 该步 Agent 的汇报（完成任务推进的主事件）
    Report,
    /// 人的指令（要求回到本步修改；blocked 任务用它重新发起）
    Instruction,
    /// 人的确认（通过 human_gate）
    Confirm,
    /// 提问（不改变推进状态）
    Question,
    /// 普通信息（不改变推进状态）
    Info,
}

/// 转移类别：驱动编排层决定推进 current_step / 完成 / 落库。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionAction {
    /// 状态不变（Info/Question/重复汇报）
    Stay,
    /// 汇报到达，等待人确认（human_gate）
    WaitConfirm,
    /// 本步完成 → 推进到下一步
    Advance,
    /// 本步完成且是最后一步 → 任务完成
    Complete,
    /// 收到指令 → 回到本步干活
    BackToWork,
    /// 从 blocked 恢复 → 重新干活
    Recover,
}

/// 一次转移的结果：转移后的 Step 状态 + 驱动编排的类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StepOutcome {
    pub state: StepState,
    pub action: TransitionAction,
}

/// 状态机转移函数（纯函数）：输入（当前 Step 状态, 消息 kind, 本步是否 human_gate, 本步是否最后一步），
/// 输出转移结果或明确错误（不猜测兜底）。
///
/// 规则（§4.4）：
/// - `Report`：human_gate 关闭 → 推进下一步/完成；开启 → 等人确认（waiting_report ↔ InputRequired）。
/// - `Confirm`：通过确认门 → 推进/完成；未收到汇报的 Confirm 是错误。
/// - `Instruction`：回到本步干活（blocked 任务 = 人工重新发起）。
/// - `Question`/`Info`：不改变推进状态。
/// - `Blocked` 中除重新发起（Instruction）外一律明确拒绝。
pub fn transition(
    state: StepState,
    kind: MessageKind,
    human_gate: bool,
    is_last: bool,
) -> Result<StepOutcome, OrcError> {
    use MessageKind::{Confirm, Info, Instruction, Question, Report};
    use StepState::{AwaitingConfirm, Blocked, InProgress};
    use TransitionAction::{Advance, BackToWork, Complete, Recover, Stay, WaitConfirm};

    match (state, kind) {
        (InProgress, Report) => {
            if human_gate {
                Ok(StepOutcome {
                    state: AwaitingConfirm,
                    action: WaitConfirm,
                })
            } else if is_last {
                Ok(StepOutcome {
                    state: InProgress,
                    action: Complete,
                })
            } else {
                Ok(StepOutcome {
                    state: InProgress,
                    action: Advance,
                })
            }
        }
        // AwaitingConfirm 只在 human_gate=true 时可达；此时 Confirm 即通过确认门
        (AwaitingConfirm, Confirm) if is_last => Ok(StepOutcome {
            state: InProgress,
            action: Complete,
        }),
        (AwaitingConfirm, Confirm) => Ok(StepOutcome {
            state: InProgress,
            action: Advance,
        }),
        (AwaitingConfirm, Report) => Ok(StepOutcome {
            state: AwaitingConfirm,
            action: Stay,
        }), // 重复汇报幂等，不重复推进
        (AwaitingConfirm, Instruction) => Ok(StepOutcome {
            state: InProgress,
            action: BackToWork,
        }),
        (AwaitingConfirm, Question | Info) => Ok(StepOutcome {
            state: AwaitingConfirm,
            action: Stay,
        }),
        (InProgress, Confirm) => Err(OrcError::confirm_before_report()),
        (InProgress, Instruction) => Ok(StepOutcome {
            state: InProgress,
            action: BackToWork,
        }),
        (InProgress, Question | Info) => Ok(StepOutcome {
            state: InProgress,
            action: Stay,
        }),
        // blocked：只接受人工重新发起（Instruction），其余消息明确拒绝
        (Blocked, Instruction) => Ok(StepOutcome {
            state: InProgress,
            action: Recover,
        }),
        (Blocked, _) => Err(OrcError::blocked_awaiting_resume()),
    }
}
