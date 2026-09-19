//! 运行时装配层，负责组件生命周期、监督和事件分发。

/// 运行时组件的可观察生命周期状态。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ComponentState {
    Starting,
    Running,
    Stopping,
    Stopped,
    Failed,
    Unknown,
}

/// 后台组件通过稳定名称和状态接入统一监督。
pub trait RuntimeComponent: Send + Sync {
    fn name(&self) -> &'static str;

    fn state(&self) -> ComponentState;
}
