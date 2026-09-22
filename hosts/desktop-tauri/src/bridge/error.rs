use std::fmt::Display;

use serde::{Deserialize, Serialize};
use specta::Type;

/// UI 可见错误。这里只允许稳定错误码、用户文案和诊断引用，不允许放原始响应或凭据。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CommandError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnostic_id: Option<String>,
}

impl CommandError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            retryable: false,
            diagnostic_id: None,
        }
    }

    pub fn with_retryable(mut self, retryable: bool) -> Self {
        self.retryable = retryable;
        self
    }

    pub fn with_diagnostic_id(mut self, diagnostic_id: impl Into<String>) -> Self {
        self.diagnostic_id = Some(diagnostic_id.into());
        self
    }

    pub fn code(&self) -> &str {
        &self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn unavailable() -> Self {
        Self::new(
            "host_command_unavailable",
            "桌面宿主尚未完成初始化，请稍后重试",
        )
        .with_retryable(true)
    }

    /// 宿主不可用，但原因具体（初始化中或初始化失败），用于避免笼统的"请稍后重试"。
    pub fn unavailable_message(message: &str) -> Self {
        Self::new("host_command_unavailable", message).with_retryable(true)
    }
}

impl Display for CommandError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for CommandError {}
