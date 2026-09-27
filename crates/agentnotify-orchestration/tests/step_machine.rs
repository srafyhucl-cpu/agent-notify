//! Step 状态机测试：完整转移矩阵 + blocked 分支 + human_gate + A2A 映射。

use agentnotify_orchestration::{
    MessageKind, OrcErrorCode, StepOutcome, StepState, TaskState, TransitionAction, transition,
};

use StepState::{AwaitingConfirm, Blocked, InProgress};
use TransitionAction::{Advance, BackToWork, Complete, Recover, Stay, WaitConfirm};

/// Report + 无门 + 非最后一步 → Advance（可进下一步）。
#[test]
fn report_no_gate_not_last_advances() {
    let o = transition(InProgress, MessageKind::Report, false, false).unwrap();
    assert_eq!(
        o,
        StepOutcome {
            state: InProgress,
            action: Advance
        }
    );
}

/// Report + 无门 + 最后一步 → Complete（任务完成）。
#[test]
fn report_no_gate_last_completes() {
    let o = transition(InProgress, MessageKind::Report, false, true).unwrap();
    assert_eq!(
        o,
        StepOutcome {
            state: InProgress,
            action: Complete
        }
    );
}

/// Report + 有门 → WaitConfirm（等人工确认），与是否最后一步无关。
#[test]
fn report_with_gate_waits_for_confirm() {
    for is_last in [false, true] {
        let o = transition(InProgress, MessageKind::Report, true, is_last).unwrap();
        assert_eq!(
            o,
            StepOutcome {
                state: AwaitingConfirm,
                action: WaitConfirm
            },
            "is_last={is_last} 时都应等确认"
        );
    }
}

/// 未收到汇报先确认 → 明确报错（ConfirmBeforeReport）。
#[test]
fn confirm_before_report_is_error() {
    let err = transition(InProgress, MessageKind::Confirm, true, false).unwrap_err();
    assert_eq!(err.code, OrcErrorCode::ConfirmBeforeReport);
    assert!(err.message.contains("尚未收到"), "{}", err.message);
}

/// Instruction → 回到干活（BackToWork）。
#[test]
fn instruction_sends_back_to_work() {
    let o = transition(InProgress, MessageKind::Instruction, false, false).unwrap();
    assert_eq!(
        o,
        StepOutcome {
            state: InProgress,
            action: BackToWork
        }
    );
}

/// Question / Info 不改变推进状态。
#[test]
fn question_and_info_stay() {
    for kind in [MessageKind::Question, MessageKind::Info] {
        let o = transition(InProgress, kind, false, false).unwrap();
        assert_eq!(
            o,
            StepOutcome {
                state: InProgress,
                action: Stay
            }
        );
    }
}

/// AwaitingConfirm + Confirm + 非最后一步 → Advance。
#[test]
fn confirm_not_last_advances() {
    let o = transition(AwaitingConfirm, MessageKind::Confirm, true, false).unwrap();
    assert_eq!(
        o,
        StepOutcome {
            state: InProgress,
            action: Advance
        }
    );
}

/// AwaitingConfirm + Confirm + 最后一步 → Complete。
#[test]
fn confirm_last_completes() {
    let o = transition(AwaitingConfirm, MessageKind::Confirm, true, true).unwrap();
    assert_eq!(
        o,
        StepOutcome {
            state: InProgress,
            action: Complete
        }
    );
}

/// AwaitingConfirm + 重复汇报 → 幂等 Stay（不重复推进）。
#[test]
fn duplicate_report_is_idempotent() {
    let o = transition(AwaitingConfirm, MessageKind::Report, true, true).unwrap();
    assert_eq!(
        o,
        StepOutcome {
            state: AwaitingConfirm,
            action: Stay
        }
    );
}

/// AwaitingConfirm + Instruction → 回到干活（汇报作废，重新改）。
#[test]
fn instruction_after_report_sends_back_to_work() {
    let o = transition(AwaitingConfirm, MessageKind::Instruction, true, false).unwrap();
    assert_eq!(
        o,
        StepOutcome {
            state: InProgress,
            action: BackToWork
        }
    );
}

/// AwaitingConfirm + Question / Info → Stay（仍等人确认）。
#[test]
fn question_info_during_await_confirm_stay() {
    for kind in [MessageKind::Question, MessageKind::Info] {
        let o = transition(AwaitingConfirm, kind, true, false).unwrap();
        assert_eq!(
            o,
            StepOutcome {
                state: AwaitingConfirm,
                action: Stay
            }
        );
    }
}

/// Blocked + Instruction（人工重新发起）→ Recover，回到干活。
#[test]
fn blocked_instruction_recovers() {
    let o = transition(Blocked, MessageKind::Instruction, false, false).unwrap();
    assert_eq!(
        o,
        StepOutcome {
            state: InProgress,
            action: Recover
        }
    );
}

/// Blocked 中其余消息（Report/Confirm/Question/Info）一律明确拒绝。
#[test]
fn blocked_rejects_non_resume_messages() {
    for kind in [
        MessageKind::Report,
        MessageKind::Confirm,
        MessageKind::Question,
        MessageKind::Info,
    ] {
        let err = transition(Blocked, kind, false, false).unwrap_err();
        assert_eq!(
            err.code,
            OrcErrorCode::BlockedAwaitingResume,
            "kind={kind:?}"
        );
        assert!(err.message.contains("已阻塞"), "{}", err.message);
    }
}

/// A2A 映射（§8.3）：waiting_report ↔ InputRequired、blocked ↔ Failed，直接复用 a2a TaskState。
#[test]
fn task_state_mapping() {
    assert_eq!(InProgress.to_task_state(), TaskState::Working);
    assert_eq!(AwaitingConfirm.to_task_state(), TaskState::InputRequired);
    assert_eq!(Blocked.to_task_state(), TaskState::Failed);

    assert_eq!(
        StepState::from_task_state(TaskState::Working).unwrap(),
        InProgress
    );
    assert_eq!(
        StepState::from_task_state(TaskState::InputRequired).unwrap(),
        AwaitingConfirm
    );
    assert_eq!(
        StepState::from_task_state(TaskState::Failed).unwrap(),
        Blocked
    );
}

/// 终态（Completed 等）无法映射回 Step 状态 → 明确报错，不做猜测兜底。
#[test]
fn terminal_state_mapping_rejected() {
    for state in [
        TaskState::Completed,
        TaskState::Canceled,
        TaskState::Rejected,
        TaskState::Submitted,
        TaskState::Unspecified,
        TaskState::AuthRequired,
    ] {
        let err = StepState::from_task_state(state).unwrap_err();
        assert_eq!(err.code, OrcErrorCode::InvalidTaskState, "state={state:?}");
        assert!(err.message.contains("无法映射"), "{}", err.message);
    }
}

/// human_gate 完整闭环：Report(有门) → AwaitConfirm → Confirm → Complete。
#[test]
fn human_gate_full_sequence() {
    let o1 = transition(InProgress, MessageKind::Report, true, true).unwrap();
    assert_eq!(
        o1,
        StepOutcome {
            state: AwaitingConfirm,
            action: WaitConfirm
        }
    );

    let o2 = transition(o1.state, MessageKind::Confirm, true, true).unwrap();
    assert_eq!(
        o2,
        StepOutcome {
            state: InProgress,
            action: Complete
        }
    );
}

/// 无门闭环：Report(无门) → （下一步 Advance，最后一步 Complete）。
#[test]
fn no_gate_full_sequence() {
    let o1 = transition(InProgress, MessageKind::Report, false, false).unwrap();
    assert_eq!(o1.action, Advance);
    let o2 = transition(InProgress, MessageKind::Report, false, true).unwrap();
    assert_eq!(o2.action, Complete);
}

/// 修改闭环：Report(有门) → Instruction 回去改 → 再次 Report → 仍等确认（幂等门语义）。
#[test]
fn instruct_after_gate_then_report_again() {
    let o1 = transition(InProgress, MessageKind::Report, true, false).unwrap();
    assert_eq!(o1.action, WaitConfirm);
    let o2 = transition(o1.state, MessageKind::Instruction, true, false).unwrap();
    assert_eq!(
        o2,
        StepOutcome {
            state: InProgress,
            action: BackToWork
        }
    );
    let o3 = transition(o2.state, MessageKind::Report, true, false).unwrap();
    assert_eq!(
        o3,
        StepOutcome {
            state: AwaitingConfirm,
            action: WaitConfirm
        }
    );
}
