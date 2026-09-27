//! 编排任务仓储（§5）：a2a-rs `TaskStore`（内存）的薄封装。
//!
//! P0 纯内存态、可单测；不碰网络/文件系统。A2A Task 是唯一事实源，
//! 编排语境（workflow_id/current_step/notify_mode/blocked）在 `metadata["orc"]`。

use std::sync::Arc;

use a2a_rs_core::TaskState;
use a2a_rs_server::TaskStore;

use crate::error::OrcError;
use crate::step_machine::{MessageKind, StepOutcome, StepState, TransitionAction, transition};
use crate::task::{NotifyMode, OrcTask};
use crate::workflow::Workflow;

/// 编排仓储：绑定一个工作流（P0 每工作流一个实例，多工作流 = 多实例）。
/// 内部存 A2A [`Task`](a2a_rs_core::Task)，业务方法返回 [`OrcTask`] 包装。
#[derive(Clone)]
pub struct OrcStore {
    tasks: TaskStore,
    workflow: Arc<Workflow>,
}

impl OrcStore {
    pub fn new(workflow: impl Into<Arc<Workflow>>) -> Self {
        Self {
            tasks: TaskStore::new(),
            workflow: workflow.into(),
        }
    }

    /// 本仓储绑定的工作流。
    pub fn workflow(&self) -> &Workflow {
        &self.workflow
    }

    /// 创建任务：工作流第 1 步开工（A2A 状态 Working，新会话语义，§3.3）。
    pub async fn create_task(
        &self,
        goal: &str,
        notify_mode: NotifyMode,
    ) -> Result<OrcTask, OrcError> {
        let task = OrcTask::new(&self.workflow, goal, notify_mode)?;
        let id = task.id().to_string();
        self.tasks.insert(task.a2a_task.clone()).await;
        self.get_task(&id).await
    }

    pub async fn get_task(&self, task_id: &str) -> Result<OrcTask, OrcError> {
        let task = self
            .tasks
            .get(task_id)
            .await
            .ok_or_else(|| OrcError::task_not_found(task_id))?;
        OrcTask::from_a2a(task)
    }

    pub async fn list_tasks(&self) -> Result<Vec<OrcTask>, OrcError> {
        let tasks = self.tasks.list().await;
        tasks.into_iter().map(OrcTask::from_a2a).collect()
    }

    /// 消息驱动推进（设计稿的 advance_step 语义）：状态机计算转移 → 落库 A2A Task 状态。
    ///
    /// 按当前步骤的 human_gate / 是否最后一步驱动规则（§4.4）：
    /// - WaitConfirm → `TaskState::InputRequired`（waiting_report 映射，§8.3）；
    /// - Advance → current_step + 1、回到 Working；
    /// - Complete → `TaskState::Completed`；
    /// - BackToWork / Recover → 回到 Working（Recover 同时清除阻塞标记）。
    pub async fn on_message(
        &self,
        task_id: &str,
        kind: MessageKind,
    ) -> Result<StepOutcome, OrcError> {
        let mut task = self.get_task(task_id).await?;
        let order = task.current_step()?;
        let step = self
            .workflow
            .step(order)
            .ok_or_else(|| OrcError::step_not_found(order))?;
        let state = StepState::from_task_state(task.state())?;
        let outcome = transition(state, kind, step.human_gate, self.workflow.is_last(order))?;

        match outcome.action {
            TransitionAction::Stay => {}
            TransitionAction::WaitConfirm => task.set_state(TaskState::InputRequired),
            TransitionAction::Advance => {
                task.set_current_step(order + 1)?;
                task.set_state(TaskState::Working);
            }
            TransitionAction::Complete => task.set_state(TaskState::Completed),
            TransitionAction::BackToWork => task.set_state(TaskState::Working),
            TransitionAction::Recover => {
                task.set_state(TaskState::Working);
                task.clear_blocked()?;
            }
        }

        self.tasks.insert(task.a2a_task).await;
        Ok(outcome)
    }

    /// 投递失败 → blocked（不自动重推，等人工处理，§4.6）。
    ///
    /// `step`：哪一步失败；`reason`：谁不可用/未送达的原因。重复标记幂等（覆盖原因）；
    /// 已终止（完成/取消/拒绝）的任务不允许标记阻塞。
    pub async fn mark_blocked(
        &self,
        task_id: &str,
        step: u32,
        reason: &str,
    ) -> Result<OrcTask, OrcError> {
        let mut task = self.get_task(task_id).await?;
        match task.state() {
            TaskState::Completed | TaskState::Canceled | TaskState::Rejected => {
                return Err(OrcError::cannot_block_terminal(task_id));
            }
            _ => {}
        }
        task.set_state(TaskState::Failed);
        task.set_blocked(step, reason)?;
        self.tasks.insert(task.a2a_task.clone()).await;
        self.get_task(task_id).await
    }

    /// blocked → 用户/桌面端重新发起：恢复干活（§4.4 blocked → step_k_active）。
    /// 非阻塞任务调用 → 明确报错。
    pub async fn recover_blocked(&self, task_id: &str) -> Result<OrcTask, OrcError> {
        let mut task = self.get_task(task_id).await?;
        if task.state() != TaskState::Failed {
            return Err(OrcError::task_not_blocked(task_id));
        }
        task.set_state(TaskState::Working);
        task.clear_blocked()?;
        self.tasks.insert(task.a2a_task.clone()).await;
        self.get_task(task_id).await
    }
}
