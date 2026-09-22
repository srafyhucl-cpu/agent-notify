use super::verify::UpdateVerificationError;

/// 更新流程的统一错误：稳定错误码 + 可直接展示给用户的中文文案。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpdateError {
    code: &'static str,
    message: String,
}

impl UpdateError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn code(&self) -> &'static str {
        self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl std::fmt::Display for UpdateError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.message())
    }
}

impl std::error::Error for UpdateError {}

impl From<UpdateVerificationError> for UpdateError {
    fn from(error: UpdateVerificationError) -> Self {
        Self::new(error.code(), error.message().to_owned())
    }
}
