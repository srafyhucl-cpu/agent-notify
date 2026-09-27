//! agentnotify-orchestration：集群任务编排骨架（P0-A 部分）。
//!
//! 落地设计文档 docs/cluster-architecture.md：
//! - §3.1.1/§3.2 可配置工作流与任务模型（[`workflow`] / [`task`]）；
//! - §3.3/§8.4 会话语义：新会话 vs 续聊（[`task`] 的消息构造辅助）；
//! - §4.3 任务信封 harness（[`envelope`]），用户模板可配置 + 内置默认兜底；
//! - §4.4 Step 推进状态机（[`step_machine`]，纯函数可单测，A2A 生命周期映射）；
//! - §5/§8 a2a-rs 内嵌：A2A Task 为唯一事实源，[`store`] 经 [`store::OrcTaskRepository`]
//!   注入仓储（P1-1 起默认内存实现，SQLite 实现由 storage-sqlite 提供，本 crate 不感知）。
//!
//! 默认关闭：本 crate 不接入 desktop-tauri / runtime / 前端，纯独立 crate + 单测；
//! 微信呈现层、a2a-client 网络调用属于后续部分（P0 其余子项）。

pub mod envelope;
pub mod error;
pub mod step_machine;
pub mod store;
pub mod task;
pub mod workflow;

pub use a2a_rs_core::{Message, Part, Role, Task, TaskState, TaskStatus};
pub use envelope::render_envelope;
pub use error::{OrcError, OrcErrorCode, OrcRepositoryError};
pub use step_machine::{MessageKind, StepOutcome, StepState, TransitionAction, transition};
pub use store::{InMemoryOrcTaskRepository, OrcStore, OrcTaskRepository};
pub use task::{NotifyMode, OrcMeta, OrcTask, continue_message, new_session_message};
pub use workflow::{
    AGENT_HINT_CODEX, AGENT_HINT_COMMANDCODE, AGENT_HINT_OPENCODE, PRESET_ORDER_EXECUTE,
    PRESET_ORDER_JUDGE, PRESET_ORDER_PLAN, PRESET_ORDER_REVIEW, PRESET_WORKFLOW_ID,
    PRESET_WORKFLOW_NAME, ROLE_EXECUTOR, ROLE_ORCHESTRATOR, ROLE_PLANNER, ROLE_REVIEWER, Workflow,
    WorkflowStep,
};
