use std::{
    collections::BTreeMap,
    panic::AssertUnwindSafe,
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

use futures::FutureExt as _;
use tokio::{sync::watch, task::JoinSet};

use super::{
    BackgroundTaskError, BackgroundTaskFactory, BackgroundTaskSnapshot, BackgroundTasks,
    TaskCancellation, TaskState,
};

const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

struct TaskRecord {
    name: String,
    state: TaskState,
}

pub struct WindowsBackgroundTasks {
    tasks: Mutex<Option<JoinSet<()>>>,
    records: Arc<RwLock<BTreeMap<u64, TaskRecord>>>,
    cancel_sender: watch::Sender<bool>,
    next_id: AtomicU64,
    shutting_down: AtomicBool,
}

impl WindowsBackgroundTasks {
    pub fn new() -> Self {
        let (cancel_sender, _) = watch::channel(false);
        Self {
            tasks: Mutex::new(Some(JoinSet::new())),
            records: Arc::new(RwLock::new(BTreeMap::new())),
            cancel_sender,
            next_id: AtomicU64::new(1),
            shutting_down: AtomicBool::new(false),
        }
    }

    pub fn spawn<F>(&self, name: &str, task: F) -> Result<(), BackgroundTaskError>
    where
        F: FnOnce(TaskCancellation) -> super::BackgroundTaskFuture + Send + 'static,
    {
        <Self as BackgroundTasks>::spawn(self, name, Box::new(task))
    }

    fn update_state(&self, id: u64, state: TaskState) {
        update_record_state(&self.records, id, state);
    }

    fn mark_running_unknown(&self) {
        if let Ok(mut records) = self.records.write() {
            for record in records.values_mut() {
                if record.state == TaskState::Running {
                    record.state = TaskState::Unknown;
                }
            }
        }
    }

    fn snapshots_inner(&self) -> Vec<BackgroundTaskSnapshot> {
        self.records
            .read()
            .map(|records| {
                records
                    .values()
                    .map(|record| BackgroundTaskSnapshot {
                        name: record.name.clone(),
                        state: record.state,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

impl Default for WindowsBackgroundTasks {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl BackgroundTasks for WindowsBackgroundTasks {
    fn spawn(&self, name: &str, task: BackgroundTaskFactory) -> Result<(), BackgroundTaskError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(BackgroundTaskError::new(
                "background_task_name_empty",
                "后台任务名称不能为空",
            ));
        }
        if self.shutting_down.load(Ordering::Acquire) {
            return Err(BackgroundTaskError::new(
                "background_tasks_shutting_down",
                "后台任务正在关闭，不能再启动新任务",
            ));
        }

        {
            let records = self.records.read().map_err(|_| {
                BackgroundTaskError::new("background_task_state_unavailable", "后台任务状态不可用")
            })?;
            if records
                .values()
                .any(|record| record.name == name && record.state == TaskState::Running)
            {
                return Err(BackgroundTaskError::new(
                    "background_task_name_duplicate",
                    format!("后台任务名称重复：{name}"),
                ));
            }
        }

        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.records
            .write()
            .map_err(|_| {
                BackgroundTaskError::new("background_task_state_unavailable", "后台任务状态不可用")
            })?
            .insert(
                id,
                TaskRecord {
                    name: name.to_owned(),
                    state: TaskState::Running,
                },
            );

        let state_receiver = self.cancel_sender.subscribe();
        let cancellation = TaskCancellation::new(self.cancel_sender.subscribe());
        let future = task(cancellation);
        let records = self.records.clone();

        let mut tasks = self.tasks.lock().map_err(|_| {
            BackgroundTaskError::new("background_task_state_unavailable", "后台任务集合不可用")
        })?;
        let Some(join_set) = tasks.as_mut() else {
            self.update_state(id, TaskState::Failed);
            return Err(BackgroundTaskError::new(
                "background_tasks_shutting_down",
                "后台任务正在关闭，不能再启动新任务",
            ));
        };
        join_set.spawn(async move {
            let result = AssertUnwindSafe(future).catch_unwind().await;
            let state = if result.is_err() {
                TaskState::Failed
            } else if *state_receiver.borrow() {
                TaskState::Cancelled
            } else {
                TaskState::Completed
            };
            update_record_state(&records, id, state);
        });

        Ok(())
    }

    async fn shutdown(&self) -> Vec<BackgroundTaskSnapshot> {
        if self.shutting_down.swap(true, Ordering::AcqRel) {
            return self.snapshots_inner();
        }
        let _ = self.cancel_sender.send(true);

        let mut join_set = match self.tasks.lock() {
            Ok(mut tasks) => tasks.take(),
            Err(_) => None,
        };
        let Some(mut join_set) = join_set.take() else {
            self.mark_running_unknown();
            return self.snapshots_inner();
        };

        let completed = tokio::time::timeout(SHUTDOWN_TIMEOUT, async {
            while join_set.join_next().await.is_some() {}
        })
        .await
        .is_ok();

        if !completed {
            join_set.abort_all();
            self.mark_running_unknown();
        }
        self.snapshots_inner()
    }

    fn snapshots(&self) -> Vec<BackgroundTaskSnapshot> {
        self.snapshots_inner()
    }
}

fn update_record_state(records: &RwLock<BTreeMap<u64, TaskRecord>>, id: u64, state: TaskState) {
    if let Ok(mut records) = records.write()
        && let Some(record) = records.get_mut(&id)
    {
        record.state = state;
    }
}
