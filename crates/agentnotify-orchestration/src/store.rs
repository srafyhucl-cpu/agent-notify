//! 编排任务仓储（§5）：`OrcStore` 通过 [`OrcTaskRepository`] trait 访问任务。
//!
//! 架构（B 方案，P1-1）：
//! - 编排 crate 保持纯业务，不依赖 storage-sqlite；
//! - [`OrcTaskRepository`] 是注入边界：`save_task / get_task / list_tasks`；
//! - 默认使用内存实现 [`InMemoryOrcTaskRepository`]（P0 a2a-rs `TaskStore` 薄封装，单测/默认用）；
//! - 持久化实现由调用方注入（desktop 开启 `orchestration.enabled` 时注入 SQLite 实现）。
//!
//! A2A Task 是唯一事实源，编排语境（workflow_id/current_step/notify_mode/blocked）在
//! `metadata["orc"]`；仓储按 Task 整体存取，不做字段级拆分。

use std::sync::Arc;

use a2a_rs_core::{Task, TaskState};
use a2a_rs_server::TaskStore;
use async_trait::async_trait;

use crate::error::{OrcError, OrcRepositoryError};
use crate::step_machine::{MessageKind, StepOutcome, StepState, TransitionAction, transition};
use crate::task::{NotifyMode, OrcTask};
use crate::workflow::Workflow;

/// 编排任务仓储契约：编排层只依赖本 trait，不感知具体存储。
///
/// 实现要求：
/// - `save_task`：按 `task.id` 保存或覆盖（幂等）；
/// - `get_task`：任务不存在返回 `Ok(None)`（由调用方决定语义）；
/// - `list_tasks`：返回全部任务；
/// - 失败的语义差异不得猜测兜底，必须返回明确错误。
#[async_trait]
pub trait OrcTaskRepository: Send + Sync {
    async fn save_task(&self, task: &Task) -> Result<(), OrcRepositoryError>;
    async fn get_task(&self, task_id: &str) -> Result<Option<Task>, OrcRepositoryError>;
    async fn list_tasks(&self) -> Result<Vec<Task>, OrcRepositoryError>;
}

/// 内存任务仓储（默认 / 单测用）：a2a-rs `TaskStore` 的 trait 适配。
#[derive(Clone, Default)]
pub struct InMemoryOrcTaskRepository {
    inner: TaskStore,
}

#[async_trait]
impl OrcTaskRepository for InMemoryOrcTaskRepository {
    async fn save_task(&self, task: &Task) -> Result<(), OrcRepositoryError> {
        self.inner.insert(task.clone()).await;
        Ok(())
    }

    async fn get_task(&self, task_id: &str) -> Result<Option<Task>, OrcRepositoryError> {
        Ok(self.inner.get(task_id).await)
    }

    async fn list_tasks(&self) -> Result<Vec<Task>, OrcRepositoryError> {
        Ok(self.inner.list().await)
    }
}

/// 编排仓储：绑定一个工作流（P0 每工作流一个实例，多工作流 = 多实例）。
/// 内部通过注入的 [`OrcTaskRepository`] 存取 A2A [`Task`]，业务方法返回 [`OrcTask`] 包装。
#[derive(Clone)]
pub struct OrcStore {
    tasks: Arc<dyn OrcTaskRepository>,
    workflow: Arc<Workflow>,
}

impl OrcStore {
    /// 使用内存仓储创建（默认）：不落盘，单测 / 未开启持久化时使用。
    pub fn new(workflow: impl Into<Arc<Workflow>>) -> Self {
        Self::with_repository(workflow, Arc::new(InMemoryOrcTaskRepository::default()))
    }

    /// 注入自定义任务仓储（SQLite 等持久化实现）。
    pub fn with_repository(
        workflow: impl Into<Arc<Workflow>>,
        repository: Arc<dyn OrcTaskRepository>,
    ) -> Self {
        Self {
            tasks: repository,
            workflow: workflow.into(),
        }
    }

    /// 本仓储绑定的工作流。
    pub fn workflow(&self) -> &Workflow {
        &self.workflow
    }

    /// 任务仓储句柄：任务级解析/模板绑定需要复用同一仓储。
    pub fn repository(&self) -> Arc<dyn OrcTaskRepository> {
        self.tasks.clone()
    }

    /// 直接按 id 从仓储读取任务（不绑定工作流；先取任务、再按 `workflow_id` 解析工作流）。
    /// 任务不存在 → `OrcErrorCode::TaskNotFound`；仓储失败 → `OrcErrorCode::Repository`。
    pub async fn fetch_task(
        repository: &Arc<dyn OrcTaskRepository>,
        task_id: &str,
    ) -> Result<OrcTask, OrcError> {
        let task = repository
            .get_task(task_id)
            .await?
            .ok_or_else(|| OrcError::task_not_found(task_id))?;
        OrcTask::from_a2a(task)
    }

    /// 创建任务：工作流第 1 步开工（A2A 状态 Working，新会话语义，§3.3）。
    pub async fn create_task(
        &self,
        goal: &str,
        notify_mode: NotifyMode,
    ) -> Result<OrcTask, OrcError> {
        let task = OrcTask::new(&self.workflow, goal, notify_mode)?;
        let id = task.id().to_string();
        self.tasks.save_task(&task.a2a_task).await?;
        self.get_task(&id).await
    }

    pub async fn get_task(&self, task_id: &str) -> Result<OrcTask, OrcError> {
        let task = self
            .tasks
            .get_task(task_id)
            .await?
            .ok_or_else(|| OrcError::task_not_found(task_id))?;
        OrcTask::from_a2a(task)
    }

    /// 直接保存任务（元数据变更后落库，如 [`OrcTask::mark_started`]）；幂等覆盖。
    pub async fn save(&self, task: OrcTask) -> Result<(), OrcError> {
        self.tasks.save_task(&task.a2a_task).await?;
        Ok(())
    }

    pub async fn list_tasks(&self) -> Result<Vec<OrcTask>, OrcError> {
        let tasks = self.tasks.list_tasks().await?;
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

        self.tasks.save_task(&task.a2a_task).await?;
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
        self.tasks.save_task(&task.a2a_task).await?;
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
        self.tasks.save_task(&task.a2a_task).await?;
        self.get_task(task_id).await
    }
}
