use std::{
    collections::BTreeMap,
    fmt::Display,
    panic::AssertUnwindSafe,
    sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard},
};

use agentnotify_domain::SafeError;
use futures::FutureExt;
use tokio::{sync::watch, task::JoinHandle};

use crate::ComponentState;

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum RuntimeState {
    Starting,
    Running,
    Degraded,
    MigrationRequired,
    Stopping,
    Stopped,
    Failed,
}

impl RuntimeState {
    pub const fn is_running(self) -> bool {
        matches!(self, Self::Running | Self::Degraded)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct ComponentSnapshot {
    pub name: String,
    pub state: ComponentState,
    pub last_error: Option<SafeError>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComponentFailure {
    pub error: SafeError,
    pub fatal: bool,
}

impl ComponentFailure {
    pub fn new(code: &str, message: &str, fatal: bool) -> Self {
        Self {
            error: SafeError::new(code, message).expect("运行时组件错误常量必须有效"),
            fatal,
        }
    }
}

impl Display for ComponentFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.error.message().fmt(formatter)
    }
}

impl std::error::Error for ComponentFailure {}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ComponentRecord {
    state: ComponentState,
    last_error: Option<SafeError>,
}

#[derive(Clone)]
pub struct Supervisor {
    inner: Arc<RwLock<BTreeMap<String, ComponentRecord>>>,
    runtime_state: watch::Sender<RuntimeState>,
    fatal_error: watch::Sender<Option<SafeError>>,
}

impl Default for Supervisor {
    fn default() -> Self {
        Self::new()
    }
}

impl Supervisor {
    pub fn new() -> Self {
        let (runtime_state, _) = watch::channel(RuntimeState::Starting);
        let (fatal_error, _) = watch::channel(None);
        Self {
            inner: Arc::new(RwLock::new(BTreeMap::new())),
            runtime_state,
            fatal_error,
        }
    }

    pub fn runtime_state(&self) -> RuntimeState {
        *self.runtime_state.borrow()
    }

    pub fn subscribe_runtime_state(&self) -> watch::Receiver<RuntimeState> {
        self.runtime_state.subscribe()
    }

    pub fn subscribe_fatal_error(&self) -> watch::Receiver<Option<SafeError>> {
        self.fatal_error.subscribe()
    }

    pub fn is_running(&self) -> bool {
        self.runtime_state().is_running()
    }

    pub fn set_runtime_state(&self, state: RuntimeState) {
        self.runtime_state.send_replace(state);
    }

    /// 取组件表写锁；中毒只说明曾有线程持锁时 panic，快照表本身仍可用。
    /// 显式恢复而不是静默丢弃：丢弃会让界面显示成「没有任何后台组件」且不报错。
    fn components_write(&self) -> RwLockWriteGuard<'_, BTreeMap<String, ComponentRecord>> {
        self.inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// 取组件表读锁；中毒处理同 [`Self::components_write`]。
    fn components_read(&self) -> RwLockReadGuard<'_, BTreeMap<String, ComponentRecord>> {
        self.inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn set_state(&self, name: &str, state: ComponentState) {
        self.components_write()
            .entry(name.to_owned())
            .and_modify(|record| record.state = state)
            .or_insert(ComponentRecord {
                state,
                last_error: None,
            });
    }

    pub fn mark_running(&self, name: &str) {
        self.set_state(name, ComponentState::Running);
    }

    pub fn report_failure(&self, name: &str, failure: &ComponentFailure) {
        self.components_write().insert(
            name.to_owned(),
            ComponentRecord {
                state: ComponentState::Failed,
                last_error: Some(failure.error.clone()),
            },
        );
        if failure.fatal {
            self.fatal_error.send_replace(Some(failure.error.clone()));
            self.set_runtime_state(RuntimeState::Failed);
        } else if matches!(
            self.runtime_state(),
            RuntimeState::Starting | RuntimeState::Running
        ) {
            self.set_runtime_state(RuntimeState::Degraded);
        }
    }

    pub fn components(&self) -> Vec<ComponentSnapshot> {
        self.components_read()
            .iter()
            .map(|(name, record)| ComponentSnapshot {
                name: name.clone(),
                state: record.state,
                last_error: record.last_error.clone(),
            })
            .collect()
    }

    /// 测试用：持写锁 panic，制造锁中毒（只用于验证「恢复后组件表仍可读」，不写业务数据）。
    #[cfg(test)]
    pub(crate) fn poison_components_lock(&self) {
        let inner = Arc::clone(&self.inner);
        let poisoned = std::thread::spawn(move || {
            let _guard = inner.write().expect("首次加锁必须成功");
            panic!("测试故意持锁 panic，制造锁中毒");
        })
        .join();
        assert!(poisoned.is_err(), "测试线程必须 panic 才能制造中毒");
    }

    pub fn spawn_component<F>(&self, name: impl Into<String>, future: F) -> JoinHandle<()>
    where
        F: std::future::Future<Output = Result<(), ComponentFailure>> + Send + 'static,
    {
        let name = name.into();
        self.set_state(&name, ComponentState::Starting);
        let supervisor = self.clone();
        tokio::spawn(async move {
            supervisor.set_state(&name, ComponentState::Running);
            let result = AssertUnwindSafe(future).catch_unwind().await;
            match result {
                Ok(Ok(())) => supervisor.set_state(&name, ComponentState::Stopped),
                Ok(Err(failure)) => supervisor.report_failure(&name, &failure),
                Err(_) => supervisor.report_failure(
                    &name,
                    &ComponentFailure::new(
                        "runtime_component_panicked",
                        "后台组件发生未处理异常",
                        false,
                    ),
                ),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{ComponentState, Supervisor};

    #[test]
    fn poisoned_lock_still_reports_and_updates_components() {
        let supervisor = Supervisor::new();
        supervisor.set_state("outbox", ComponentState::Running);
        supervisor.poison_components_lock();

        // 中毒后组件表不能被静默清空，且必须仍可写入新状态。
        assert_eq!(supervisor.components().len(), 1);
        supervisor.set_state("outbox", ComponentState::Stopped);
        let snapshot = supervisor.components();
        assert_eq!(snapshot.len(), 1);
        assert_eq!(snapshot[0].state, ComponentState::Stopped);
    }
}
