pub mod autostart;
pub mod pause;
pub mod single_instance;
pub mod tray;
pub mod window;

use std::{
    fmt,
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, Ordering},
    },
};

use crate::lifecycle::pause::{PauseCoordinator, PauseSettingsStore, RuntimeControl};
use crate::lifecycle::window::{RuntimeReadyAction, RuntimeState};

/// 生命周期层的稳定错误，供托盘和宿主上层直接展示。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LifecycleError {
    code: String,
    message: String,
}

impl LifecycleError {
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

impl fmt::Display for LifecycleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message())
    }
}

impl std::error::Error for LifecycleError {}

/// 生命周期控制器只持有 runtime 的显式端口，不假设核心已经支持暂停。
#[derive(Clone, Default)]
pub struct LifecycleController {
    inner: Arc<LifecycleControllerInner>,
}

#[derive(Default)]
struct LifecycleControllerInner {
    runtime: RwLock<Option<Arc<dyn RuntimeControl>>>,
    pause: RwLock<Option<Arc<PauseCoordinator>>>,
    quitting: AtomicBool,
    runtime_ready: AtomicBool,
}

impl LifecycleController {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn attach_runtime(
        &self,
        runtime: Arc<dyn RuntimeControl>,
        settings: Arc<dyn PauseSettingsStore>,
    ) -> Result<RuntimeReadyAction, LifecycleError> {
        self.attach_runtime_with_action(runtime, settings, RuntimeReadyAction::ShowMain)
            .await
    }

    pub async fn attach_runtime_with_action(
        &self,
        runtime: Arc<dyn RuntimeControl>,
        settings: Arc<dyn PauseSettingsStore>,
        action: RuntimeReadyAction,
    ) -> Result<RuntimeReadyAction, LifecycleError> {
        let pause = Arc::new(PauseCoordinator::initialize(runtime.clone(), settings).await?);
        let mut runtime_slot = self.inner.runtime.write().map_err(|_| {
            LifecycleError::new("lifecycle_state_unavailable", "生命周期状态不可用")
        })?;
        let mut pause_slot = self.inner.pause.write().map_err(|_| {
            LifecycleError::new("lifecycle_state_unavailable", "生命周期状态不可用")
        })?;
        *runtime_slot = Some(runtime);
        *pause_slot = Some(pause);
        self.inner.runtime_ready.store(true, Ordering::Release);
        Ok(action)
    }

    pub fn runtime_state(&self) -> RuntimeState {
        self.inner
            .runtime
            .read()
            .ok()
            .and_then(|runtime| runtime.as_ref().map(|runtime| runtime.state()))
            .unwrap_or(RuntimeState::Starting)
    }

    pub fn is_runtime_ready(&self) -> bool {
        self.inner.runtime_ready.load(Ordering::Acquire)
    }

    pub fn is_paused(&self) -> bool {
        self.inner
            .pause
            .read()
            .ok()
            .and_then(|pause| pause.as_ref().map(|pause| pause.is_paused()))
            .unwrap_or(false)
    }

    pub async fn set_paused(&self, paused: bool) -> Result<bool, LifecycleError> {
        let pause = self
            .inner
            .pause
            .read()
            .map_err(|_| LifecycleError::new("lifecycle_state_unavailable", "生命周期状态不可用"))?
            .clone()
            .ok_or_else(|| {
                LifecycleError::new(
                    "runtime_not_connected",
                    "提醒运行时尚未连接，暂时不能暂停或恢复通知",
                )
            })?;
        pause.set_paused(paused).await?;
        Ok(paused)
    }

    pub fn begin_quit(&self) {
        self.inner.quitting.store(true, Ordering::Release);
    }

    pub fn is_quitting(&self) -> bool {
        self.inner.quitting.load(Ordering::Acquire)
    }

    pub async fn shutdown_for_quit(&self) -> Result<(), LifecycleError> {
        self.begin_quit();
        let runtime = self
            .inner
            .runtime
            .read()
            .map_err(|_| LifecycleError::new("lifecycle_state_unavailable", "生命周期状态不可用"))?
            .clone();
        let result = match runtime {
            Some(runtime) => runtime.shutdown_runtime().await,
            None => Ok(()),
        };
        if result.is_err() {
            self.inner.quitting.store(false, Ordering::Release);
        }
        result
    }
}
