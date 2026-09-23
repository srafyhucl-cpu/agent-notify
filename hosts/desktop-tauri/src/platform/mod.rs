use std::{path::PathBuf, sync::Arc, time::Duration};

use agentnotify_application::SecretStore;
pub use agentnotify_ingress::IngressHandler;

#[cfg(windows)]
pub mod windows;

/// 宿主持有的应用目录。所有目录在使用前必须显式创建，缺失时不回退到当前目录。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppPaths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub log_dir: PathBuf,
    pub spool_dir: PathBuf,
    /// 应用自己的临时目录：更新包下载、解压与安装日志都落在这里，不直接用 `%TEMP%`。
    pub temp_dir: PathBuf,
}

impl AppPaths {
    pub fn for_tests(root: impl AsRef<std::path::Path>) -> Self {
        let root = root.as_ref();
        Self {
            config_dir: root.join("config"),
            data_dir: root.join("data"),
            log_dir: root.join("logs"),
            spool_dir: root.join("spool"),
            temp_dir: root.join("temp"),
        }
    }
}

/// 平台能力必须通过本端口暴露给运行时，业务层不得直接依赖具体 Windows API。
pub trait PlatformHost: Send + Sync {
    fn paths(&self) -> AppPaths;

    fn secret_store(&self) -> Arc<dyn SecretStore>;

    fn process_runner(&self) -> Arc<dyn ProcessRunner>;

    fn background_tasks(&self) -> Arc<dyn BackgroundTasks>;

    fn local_ipc(&self) -> Arc<dyn LocalIpc>;

    fn system_ui(&self) -> Arc<dyn SystemUi>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProcessOutcome {
    Completed(ProcessOutput),
    UnknownResult { reason: ProcessUnknownReason },
}

impl ProcessOutcome {
    pub const fn is_unknown(&self) -> bool {
        matches!(self, Self::UnknownResult { .. })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProcessUnknownReason {
    Timeout,
    Cancelled,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessOutput {
    exit_code: Option<i32>,
    success: bool,
    stdout: CapturedOutput,
    stderr: CapturedOutput,
}

impl ProcessOutput {
    pub fn new(
        exit_code: Option<i32>,
        success: bool,
        stdout: CapturedOutput,
        stderr: CapturedOutput,
    ) -> Self {
        Self {
            exit_code,
            success,
            stdout,
            stderr,
        }
    }

    pub const fn exit_code(&self) -> Option<i32> {
        self.exit_code
    }

    pub const fn success(&self) -> bool {
        self.success
    }

    pub const fn stdout(&self) -> &CapturedOutput {
        &self.stdout
    }

    pub const fn stderr(&self) -> &CapturedOutput {
        &self.stderr
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapturedOutput {
    bytes: Vec<u8>,
    truncated: bool,
}

impl CapturedOutput {
    pub fn new(bytes: Vec<u8>, truncated: bool) -> Self {
        Self { bytes, truncated }
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub const fn truncated(&self) -> bool {
        self.truncated
    }
}

#[async_trait::async_trait]
pub trait ProcessRunner: Send + Sync {
    async fn run(&self, request: ProcessRequest) -> Result<ProcessOutcome, ProcessError>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessError {
    code: String,
    message: String,
}

impl ProcessError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn code(&self) -> &str {
        &self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

#[derive(Clone, Debug)]
pub struct ProcessRequest {
    pub(crate) program: PathBuf,
    pub(crate) args: Vec<std::ffi::OsString>,
    pub(crate) cwd: Option<PathBuf>,
    pub(crate) env: std::collections::BTreeMap<std::ffi::OsString, std::ffi::OsString>,
    pub(crate) env_allowlist: Vec<std::ffi::OsString>,
    pub(crate) timeout: Duration,
    pub(crate) cancel: Option<tokio::sync::watch::Receiver<bool>>,
}

/// 子进程默认超时：外部 CLI / Hook 都按这个上限收敛，防止挂死。
pub const DEFAULT_PROCESS_TIMEOUT: Duration = Duration::from_secs(30);

impl ProcessRequest {
    pub fn new(program: impl Into<std::ffi::OsString>) -> Self {
        Self {
            program: PathBuf::from(program.into()),
            args: Vec::new(),
            cwd: None,
            env: std::collections::BTreeMap::new(),
            env_allowlist: Vec::new(),
            timeout: DEFAULT_PROCESS_TIMEOUT,
            cancel: None,
        }
    }

    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<std::ffi::OsString>,
    {
        self.args = args.into_iter().map(Into::into).collect();
        self
    }

    pub fn current_dir(mut self, directory: impl Into<PathBuf>) -> Self {
        self.cwd = Some(directory.into());
        self
    }

    pub fn env(
        mut self,
        key: impl Into<std::ffi::OsString>,
        value: impl Into<std::ffi::OsString>,
    ) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }

    pub fn env_allowlist<I, S>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<std::ffi::OsString>,
    {
        self.env_allowlist = names.into_iter().map(Into::into).collect();
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn cancel_on(mut self, cancel: tokio::sync::watch::Receiver<bool>) -> Self {
        self.cancel = Some(cancel);
        self
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaskState {
    Running,
    Completed,
    Cancelled,
    Failed,
    Unknown,
}

impl TaskState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackgroundTaskSnapshot {
    pub name: String,
    pub state: TaskState,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackgroundTaskError {
    code: String,
    message: String,
}

impl BackgroundTaskError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn code(&self) -> &str {
        &self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

pub type BackgroundTaskFuture =
    std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'static>>;
pub type BackgroundTaskFactory =
    Box<dyn FnOnce(TaskCancellation) -> BackgroundTaskFuture + Send + 'static>;

#[derive(Clone)]
pub struct TaskCancellation {
    receiver: tokio::sync::watch::Receiver<bool>,
}

impl TaskCancellation {
    pub fn new(receiver: tokio::sync::watch::Receiver<bool>) -> Self {
        Self { receiver }
    }

    pub async fn cancelled(&self) {
        let mut receiver = self.receiver.clone();
        if *receiver.borrow() {
            return;
        }
        while receiver.changed().await.is_ok() {
            if *receiver.borrow() {
                return;
            }
        }
    }
}

#[async_trait::async_trait]
pub trait BackgroundTasks: Send + Sync {
    fn spawn(&self, name: &str, task: BackgroundTaskFactory) -> Result<(), BackgroundTaskError>;

    async fn shutdown(&self) -> Vec<BackgroundTaskSnapshot>;

    fn snapshots(&self) -> Vec<BackgroundTaskSnapshot>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IpcSubmitResult {
    Submitted,
    Spooled,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalIpcError {
    code: String,
    message: String,
}

impl LocalIpcError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn code(&self) -> &str {
        &self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

#[async_trait::async_trait]
pub trait LocalIpc: Send + Sync {
    async fn serve(
        &self,
        handler: Arc<dyn IngressHandler>,
        cancel: tokio::sync::watch::Receiver<bool>,
    ) -> Result<(), LocalIpcError>;

    async fn connect(
        &self,
        payload: &[u8],
        connect_timeout: Duration,
    ) -> Result<IpcSubmitResult, LocalIpcError>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TrayState {
    Running,
    Paused,
    Stopped,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SystemUiError {
    code: String,
    message: String,
}

impl SystemUiError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn code(&self) -> &str {
        &self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

#[async_trait::async_trait]
pub trait SystemUi: Send + Sync {
    async fn show_main_window(&self) -> Result<(), SystemUiError>;

    async fn hide_main_window(&self) -> Result<(), SystemUiError>;

    async fn set_tray_state(&self, state: TrayState) -> Result<(), SystemUiError>;

    async fn show_system_notification(&self, title: &str, body: &str) -> Result<(), SystemUiError>;

    async fn open_log_dir(&self) -> Result<(), SystemUiError>;
}
