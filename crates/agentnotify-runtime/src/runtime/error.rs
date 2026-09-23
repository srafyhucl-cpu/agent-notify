//! 运行时错误：错误码稳定、消息面向界面；各层错误在这里归一化成 RuntimeError。

use agentnotify_application::{IngestError, ReplyError, StoreError};

use super::RuntimeTargetError;
use crate::TelemetryError;
use crate::migration::MigrationFailure;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuntimeError {
    Store(StoreError),
    Reply(ReplyError),
    IngressSpool {
        code: &'static str,
        message: &'static str,
    },
    Telemetry(TelemetryError),
    InvalidConfiguration {
        field: &'static str,
    },
    IntegrityCheckFailed,
    Ingest(IngestError),
    TargetProvider(RuntimeTargetError),
    Migration(Box<MigrationFailure>),
}

impl RuntimeError {
    pub fn code(&self) -> &str {
        match self {
            Self::Store(error) => error.code(),
            Self::Reply(error) => error.code(),
            Self::IngressSpool { code, .. } => code,
            Self::Telemetry(error) => error.code(),
            Self::InvalidConfiguration { field } => field,
            Self::IntegrityCheckFailed => "database_integrity_failed",
            Self::Ingest(error) => error.code(),
            Self::TargetProvider(error) => error.code(),
            Self::Migration(error) => error.code.as_str(),
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Self::Store(error) => error.message(),
            Self::Reply(error) => error.message(),
            Self::IngressSpool { message, .. } => message,
            Self::Telemetry(error) => error.message(),
            Self::InvalidConfiguration { .. } => "运行时配置无效",
            Self::IntegrityCheckFailed => "数据库完整性检查失败，运行时未启动",
            Self::Ingest(error) => error.message(),
            Self::TargetProvider(error) => error.message(),
            Self::Migration(error) => error.message.as_str(),
        }
    }
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.message())
    }
}

impl std::error::Error for RuntimeError {}

impl From<StoreError> for RuntimeError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}

impl From<ReplyError> for RuntimeError {
    fn from(value: ReplyError) -> Self {
        Self::Reply(value)
    }
}

impl From<IngestError> for RuntimeError {
    fn from(value: IngestError) -> Self {
        Self::Ingest(value)
    }
}

impl From<agentnotify_ingress::SpoolError> for RuntimeError {
    fn from(value: agentnotify_ingress::SpoolError) -> Self {
        Self::IngressSpool {
            code: value.code(),
            message: value.message(),
        }
    }
}

impl From<TelemetryError> for RuntimeError {
    fn from(value: TelemetryError) -> Self {
        Self::Telemetry(value)
    }
}

impl From<MigrationFailure> for RuntimeError {
    fn from(value: MigrationFailure) -> Self {
        Self::Migration(Box::new(value))
    }
}
