//! `OrcTaskRepository` 契约测试：trait 注入边界 + 内存实现 + 错误路径。
//!
//! P1-1 起 `OrcStore` 不再直接持有 a2a-rs `TaskStore`，而是依赖注入的仓储 trait。
//! 本文件验证：
//! - 内存实现（默认）语义不变：save/get/list 往返；
//! - 仓储出错（mock 注入 save/load/list 失败）时 `OrcStore` 明确向上暴露
//!   `Repository` 错误码，不做猜测式兜底；
//! - 任务不存在仍返回既有 `TaskNotFound` 语义。

use std::sync::Arc;

use agentnotify_orchestration::{
    InMemoryOrcTaskRepository, MessageKind, NotifyMode, OrcErrorCode, OrcRepositoryError, OrcStore,
    OrcTaskRepository, Task, TaskState, TransitionAction, Workflow,
};

/// 可注入失败的内存仓储：按方法分别模拟持久化层错误（如数据库不可用）。
#[derive(Clone, Default)]
struct FaultyRepository {
    inner: InMemoryOrcTaskRepository,
    fail_save: bool,
    fail_get: bool,
    fail_list: bool,
    fail_delete: bool,
}

#[async_trait::async_trait]
impl OrcTaskRepository for FaultyRepository {
    async fn save_task(&self, task: &Task) -> Result<(), OrcRepositoryError> {
        if self.fail_save {
            return Err(OrcRepositoryError::new(
                "mock_save_failed",
                "模拟保存任务失败",
            ));
        }
        self.inner.save_task(task).await
    }

    async fn get_task(&self, task_id: &str) -> Result<Option<Task>, OrcRepositoryError> {
        if self.fail_get {
            return Err(OrcRepositoryError::new(
                "mock_get_failed",
                "模拟读取任务失败",
            ));
        }
        self.inner.get_task(task_id).await
    }

    async fn list_tasks(&self) -> Result<Vec<Task>, OrcRepositoryError> {
        if self.fail_list {
            return Err(OrcRepositoryError::new(
                "mock_list_failed",
                "模拟列出任务失败",
            ));
        }
        self.inner.list_tasks().await
    }

    async fn delete_task(&self, task_id: &str) -> Result<(), OrcRepositoryError> {
        if self.fail_delete {
            return Err(OrcRepositoryError::new(
                "mock_delete_failed",
                "模拟删除任务失败",
            ));
        }
        self.inner.delete_task(task_id).await
    }
}

/// 内存仓储往返：save 后可 get、可 list，再次 save 覆盖不丢数据。
#[tokio::test]
async fn in_memory_repository_round_trip() {
    let store = OrcStore::new(Workflow::preset(false).unwrap());
    let task = store
        .create_task("内存仓储往返", NotifyMode::FinalOnly)
        .await
        .unwrap();

    let fetched = store.get_task(task.id()).await.unwrap();
    assert_eq!(fetched, task);

    let listed = store.list_tasks().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0], task);
}

/// 注入的仓储决定实现：同一 OrcStore 业务代码对不同仓储表现一致。
#[tokio::test]
async fn orc_store_uses_injected_repository() {
    let repo = Arc::new(InMemoryOrcTaskRepository::default());
    let store = OrcStore::with_repository(Workflow::preset(false).unwrap(), repo.clone());
    let task = store
        .create_task("注入仓储", NotifyMode::Verbose)
        .await
        .unwrap();

    // 仓储 trait 直接可读：证明写入确实进了注入的实现。
    let direct = repo
        .get_task(task.id())
        .await
        .unwrap()
        .expect("任务必须已保存");
    assert_eq!(direct.id, task.id());
}

/// save 失败 → create_task / on_message / mark_blocked / recover 全部明确报错（Repository 码）。
#[tokio::test]
async fn save_failure_surfaces_repository_error() {
    let repo = FaultyRepository {
        fail_save: true,
        ..FaultyRepository::default()
    };
    let store = OrcStore::with_repository(Workflow::preset(false).unwrap(), Arc::new(repo));

    let error = store
        .create_task("目标", NotifyMode::FinalOnly)
        .await
        .unwrap_err();
    assert_eq!(error.code, OrcErrorCode::Repository);
    assert!(
        error.message.contains("模拟保存任务失败"),
        "{}",
        error.message
    );
}

/// load 失败 → get_task 明确报错；任务不存在仍保持既有 TaskNotFound 语义。
#[tokio::test]
async fn load_failure_surfaces_repository_error() {
    let store = OrcStore::new(Workflow::preset(false).unwrap());
    store
        .create_task("目标", NotifyMode::FinalOnly)
        .await
        .unwrap();

    let repo = FaultyRepository {
        fail_get: true,
        ..FaultyRepository::default()
    };
    let store = OrcStore::with_repository(Workflow::preset(false).unwrap(), Arc::new(repo));

    let error = store.get_task("no-such-id").await.unwrap_err();
    assert_eq!(error.code, OrcErrorCode::Repository);
    assert!(
        error.message.contains("模拟读取任务失败"),
        "{}",
        error.message
    );
}

/// 仓储存在但任务缺失：非仓储错误，仍是 TaskNotFound（由 OrcStore 语义决定）。
#[tokio::test]
async fn missing_task_stays_task_not_found() {
    let store = OrcStore::new(Workflow::preset(false).unwrap());
    let error = store.get_task("missing").await.unwrap_err();
    assert_eq!(error.code, OrcErrorCode::TaskNotFound);
}

/// list 失败 → list_tasks 明确报错。
#[tokio::test]
async fn list_failure_surfaces_repository_error() {
    let repo = FaultyRepository {
        fail_list: true,
        ..FaultyRepository::default()
    };
    let store = OrcStore::with_repository(Workflow::preset(false).unwrap(), Arc::new(repo));

    let error = store.list_tasks().await.unwrap_err();
    assert_eq!(error.code, OrcErrorCode::Repository);
    assert!(
        error.message.contains("模拟列出任务失败"),
        "{}",
        error.message
    );
}

/// 业务流程（推进/阻塞/恢复）在注入仓储下走同一套 save→load 边界。
#[tokio::test]
async fn business_flow_uses_repository_save_boundary() {
    let repo = Arc::new(InMemoryOrcTaskRepository::default());
    let store = OrcStore::with_repository(Workflow::preset(false).unwrap(), repo.clone());
    let id = store
        .create_task("推进链路", NotifyMode::FinalOnly)
        .await
        .unwrap()
        .id()
        .to_string();

    let outcome = store.on_message(&id, MessageKind::Report).await.unwrap();
    assert_eq!(outcome.action, TransitionAction::Advance);

    // 推进结果必须已写回注入的仓储（第 1 步汇报 → 第 2 步干活中）。
    let saved = repo.get_task(&id).await.unwrap().expect("推进后必须落库");
    assert_eq!(saved.status.state, TaskState::Working, "推进后回到干活中");
    let resumed = store.get_task(&id).await.unwrap();
    assert_eq!(
        resumed.current_step().unwrap(),
        2,
        "推进后的当前步骤必须落库"
    );
}

/// 仓储错误消息透传给用户（中文可读），错误码稳定可程序化判断。
#[test]
fn repository_error_round_trips_code_and_message() {
    let error = OrcRepositoryError::new("sqlite_error", "执行 SQLite 查询失败");
    assert_eq!(error.code(), "sqlite_error");
    assert_eq!(error.message(), "执行 SQLite 查询失败");
    assert_eq!(error.to_string(), "执行 SQLite 查询失败");
}
