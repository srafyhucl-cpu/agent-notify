mod paths;
mod process;
mod secrets;
mod system_ui;
mod tasks;

use std::{path::Path, sync::Arc, time::Duration};

use agentnotify_application::SecretStore;
use agentnotify_ingress::{PipeError, SubmitResult};

pub use paths::AppPathsError;
pub use process::{MAX_PROCESS_OUTPUT_BYTES, WindowsProcessRunner};
pub use secrets::{
    CREDENTIAL_SERVICE_NAME, CredentialBackend, WindowsCredentialBackend, WindowsSecretStore,
    credential_target,
};
pub use system_ui::{
    MAIN_WINDOW_LABEL, TRAY_STATE_EVENT, WindowsSystemUi, validate_existing_directory,
};
pub use tasks::WindowsBackgroundTasks;

pub use super::{
    AppPaths, BackgroundTaskError, BackgroundTaskFactory, BackgroundTaskFuture,
    BackgroundTaskSnapshot, BackgroundTasks, CapturedOutput, IpcSubmitResult, LocalIpc,
    LocalIpcError, PlatformHost, ProcessError, ProcessOutcome, ProcessOutput, ProcessRequest,
    ProcessRunner, ProcessUnknownReason, SystemUi, SystemUiError, TaskCancellation, TaskState,
    TrayState,
};

#[derive(Clone, Debug, Default)]
pub struct WindowsLocalIpc {
    pipe_name: Option<String>,
}

impl WindowsLocalIpc {
    pub fn with_pipe_name(name: impl Into<String>) -> Self {
        Self {
            pipe_name: Some(name.into()),
        }
    }
}

#[async_trait::async_trait]
impl LocalIpc for WindowsLocalIpc {
    async fn serve(
        &self,
        handler: Arc<dyn super::IngressHandler>,
        cancel: tokio::sync::watch::Receiver<bool>,
    ) -> Result<(), LocalIpcError> {
        let result = match self.pipe_name.clone() {
            Some(name) => agentnotify_ingress::serve_on(name, handler, cancel).await,
            None => agentnotify_ingress::serve(handler, cancel).await,
        };
        result.map_err(map_pipe_error)
    }

    async fn connect(
        &self,
        payload: &[u8],
        connect_timeout: Duration,
    ) -> Result<IpcSubmitResult, LocalIpcError> {
        let name = match &self.pipe_name {
            Some(name) => name.clone(),
            None => agentnotify_ingress::pipe_name().map_err(map_pipe_error)?,
        };
        let result = agentnotify_ingress::connect_and_submit(&name, payload, connect_timeout)
            .await
            .map_err(map_pipe_error)?;
        Ok(match result {
            SubmitResult::Submitted => IpcSubmitResult::Submitted,
            SubmitResult::Spooled => IpcSubmitResult::Spooled,
        })
    }
}

pub struct WindowsPlatformHost {
    paths: AppPaths,
    secret_store: Arc<dyn SecretStore>,
    process_runner: Arc<dyn super::ProcessRunner>,
    background_tasks: Arc<dyn BackgroundTasks>,
    local_ipc: Arc<dyn LocalIpc>,
    system_ui: Arc<dyn SystemUi>,
}

impl WindowsPlatformHost {
    pub fn from_environment() -> Result<Self, AppPathsError> {
        let paths = AppPaths::from_environment()?;
        paths.ensure()?;
        Ok(Self::with_paths(
            paths.clone(),
            Arc::new(WindowsSystemUi::unavailable(paths)),
        ))
    }

    pub fn for_tests(root: impl AsRef<Path>) -> Result<Self, AppPathsError> {
        let paths = AppPaths::for_tests(root);
        paths.ensure()?;
        Ok(Self::with_paths(
            paths.clone(),
            Arc::new(WindowsSystemUi::unavailable(paths)),
        ))
    }

    pub fn with_system_ui(paths: AppPaths, system_ui: Arc<dyn SystemUi>) -> Self {
        Self::with_paths(paths, system_ui)
    }

    fn with_paths(paths: AppPaths, system_ui: Arc<dyn SystemUi>) -> Self {
        Self {
            paths,
            secret_store: Arc::new(WindowsSecretStore::new()),
            process_runner: Arc::new(WindowsProcessRunner),
            background_tasks: Arc::new(WindowsBackgroundTasks::new()),
            local_ipc: Arc::new(WindowsLocalIpc::default()),
            system_ui,
        }
    }

    pub fn paths(&self) -> AppPaths {
        self.paths.clone()
    }

    pub fn secret_store(&self) -> Arc<dyn SecretStore> {
        self.secret_store.clone()
    }

    pub fn process_runner(&self) -> Arc<dyn super::ProcessRunner> {
        self.process_runner.clone()
    }

    pub fn background_tasks(&self) -> Arc<dyn BackgroundTasks> {
        self.background_tasks.clone()
    }

    pub fn local_ipc(&self) -> Arc<dyn LocalIpc> {
        self.local_ipc.clone()
    }

    pub fn system_ui(&self) -> Arc<dyn SystemUi> {
        self.system_ui.clone()
    }
}

impl PlatformHost for WindowsPlatformHost {
    fn paths(&self) -> AppPaths {
        self.paths.clone()
    }

    fn secret_store(&self) -> Arc<dyn SecretStore> {
        self.secret_store.clone()
    }

    fn process_runner(&self) -> Arc<dyn super::ProcessRunner> {
        self.process_runner.clone()
    }

    fn background_tasks(&self) -> Arc<dyn BackgroundTasks> {
        self.background_tasks.clone()
    }

    fn local_ipc(&self) -> Arc<dyn LocalIpc> {
        self.local_ipc.clone()
    }

    fn system_ui(&self) -> Arc<dyn SystemUi> {
        self.system_ui.clone()
    }
}

fn map_pipe_error(error: PipeError) -> LocalIpcError {
    let code = match &error {
        PipeError::InvalidName => "ipc_invalid_name",
        PipeError::AlreadyRunning => "ipc_already_running",
        PipeError::SidUnavailable => "ipc_sid_unavailable",
        PipeError::SecurityDescriptorUnavailable => "ipc_security_descriptor_unavailable",
        PipeError::CreateFailed(_) => "ipc_create_failed",
        PipeError::Io(_) => "ipc_io_failed",
        PipeError::Protocol(_) => "ipc_protocol_error",
    };
    LocalIpcError::new(code, error.message())
}
