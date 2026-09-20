//! 运行时装配层，负责组件生命周期、监督和事件分发。

mod event_bus;
mod ingress;
mod migration;
mod platform;
mod runtime;
mod supervisor;
mod telemetry;

/// 运行时组件的可观察生命周期状态。
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
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

pub use event_bus::{EventBus, RuntimeEvent};
pub use migration::{
    MigrationConfig, MigrationFailure, MigrationIssue, MigrationReportSummary, MigrationSnapshot,
    MigrationState, MigrationWarning,
};
pub use runtime::{
    AppRuntime, DiagnosticItem, DiagnosticLevel, ResolvedRuntimeTargets, RuntimeConfig,
    RuntimeError, RuntimeHandle, RuntimeSnapshot, RuntimeTargetError, RuntimeTargetProvider,
    start_migration_diagnostics,
};
pub use supervisor::{ComponentFailure, ComponentSnapshot, RuntimeState, Supervisor};
pub use telemetry::{
    RedactingWriter, TelemetryConfig, TelemetryError, TelemetryGuard, init_telemetry,
    redact_sensitive,
};
