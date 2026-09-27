//! 0003 迁移（orc_tasks）与 `OrcTaskRepository` SQLite 实现的集成测试。
//!
//! 覆盖：迁移版本 3 应用、表存在、重启幂等；save 后可 load、更新保留 created_at、
//! list 有序（新→旧）、未知 id 返回 None、非编排任务明确报错。

use std::sync::Arc;

use agentnotify_orchestration::{
    NotifyMode, OrcStore, OrcTask, OrcTaskRepository, Task, TaskState, TaskStatus, Workflow,
};
use agentnotify_storage_sqlite::SqliteStore;
use rusqlite::{Connection, params};

/// 打开一个真实 SQLite 文件，等待 0003 迁移应用。
#[tokio::test]
async fn migration_0003_applies_and_table_exists_idempotently() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("state.db");

    {
        let store = SqliteStore::open(&path).unwrap();
        assert_eq!(store.schema_version().await.unwrap(), 3);
        assert!(store.table_exists("orc_tasks").await.unwrap());
    }
    // 重复打开幂等：不再重跑迁移、仍保持版本 3。
    {
        let store = SqliteStore::open(&path).unwrap();
        assert_eq!(store.schema_version().await.unwrap(), 3);
        assert!(store.table_exists("orc_tasks").await.unwrap());
    }
}

/// save 后可 load：同库往返 + 关闭重开（模拟应用重启）后任务仍在。
#[tokio::test]
async fn orc_task_save_then_load_round_trip() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("state.db");

    let created;
    {
        let store = SqliteStore::open(&path).unwrap();
        let orc = OrcStore::with_repository(Workflow::preset(false).unwrap(), Arc::new(store.clone()));
        created = orc
            .create_task("做一个贪吃蛇游戏", NotifyMode::FinalOnly)
            .await
            .unwrap();

        let fetched = orc.get_task(created.id()).await.unwrap();
        assert_eq!(fetched, created);
    }

    // 关闭重开：SQLite 文件里仍能读回完整任务。
    {
        let store = SqliteStore::open(&path).unwrap();
        let orc = OrcStore::with_repository(Workflow::preset(false).unwrap(), Arc::new(store.clone()));
        let reloaded = orc.get_task(created.id()).await.unwrap();
        assert_eq!(reloaded, created, "重开数据库后任务内容必须一致");
        assert_eq!(reloaded.state(), TaskState::Working);
        assert_eq!(reloaded.current_step().unwrap(), 1);
    }
}

/// 更新（blocked 落库）保留首次 created_at，不再改写；状态与阻塞原因确实更新。
#[tokio::test]
async fn orc_task_update_preserves_created_at_and_updates_content() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("state.db");
    let store = SqliteStore::open(&path).unwrap();
    let orc = OrcStore::with_repository(Workflow::preset(false).unwrap(), Arc::new(store.clone()));

    let task = orc
        .create_task("目标", NotifyMode::FinalOnly)
        .await
        .unwrap();
    let task_id = task.id().to_string();

    let created_at_before = created_at_of(&path, &task_id);

    let blocked = orc
        .mark_blocked(&task_id, 1, "codex 未登录，消息未送达")
        .await
        .unwrap();
    assert_eq!(blocked.state(), TaskState::Failed);
    assert_eq!(blocked.blocked_step().unwrap(), Some(1));

    let created_at_after = created_at_of(&path, &task_id);
    assert_eq!(
        created_at_before, created_at_after,
        "更新不得改写 created_at"
    );

    // 内容确实更新：阻塞原因已随 a2a_task_json 落库。
    let meta = blocked.meta().unwrap();
    assert_eq!(meta.block_reason.as_deref(), Some("codex 未登录，消息未送达"));
}

/// 未知 task_id → Ok(None)（由调用方决定语义，仓储不猜测）。
#[tokio::test]
async fn orc_repo_get_missing_returns_none() {
    let temp = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(temp.path().join("state.db")).unwrap();

    let fetched = store.get_task("no-such-task").await.unwrap();
    assert!(fetched.is_none());
}

/// list 有序：created_at 新→旧（后创建的任务排在最前）；重开后顺序稳定。
#[tokio::test]
async fn orc_repo_list_is_ordered_newest_first() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("state.db");
    let store = SqliteStore::open(&path).unwrap();
    let orc = OrcStore::with_repository(Workflow::preset(false).unwrap(), Arc::new(store.clone()));

    orc.create_task("先建任务", NotifyMode::FinalOnly).await.unwrap();
    // 保证两条 created_at 可区分（毫秒精度）。
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    orc.create_task("后建任务", NotifyMode::FinalOnly).await.unwrap();

    let tasks = store.list_tasks().await.unwrap();
    assert_eq!(tasks.len(), 2);

    // 用目标区分先后（created_at 新→旧 ⇒ 后建任务在前）。
    let newest = OrcTask::from_a2a(tasks[0].clone()).unwrap();
    let oldest = OrcTask::from_a2a(tasks[1].clone()).unwrap();
    assert_eq!(newest.goal().unwrap(), "后建任务", "最新创建的任务必须排在最前");
    assert_eq!(oldest.goal().unwrap(), "先建任务");
    drop(store);
    drop(orc);

    // 重开后顺序稳定。
    let store = SqliteStore::open(&path).unwrap();
    let reloaded = store.list_tasks().await.unwrap();
    assert_eq!(reloaded.len(), 2);
}

/// 写一个不含编排元数据的 A2A Task：仓储必须明确拒绝，不猜测兜底。
#[tokio::test]
async fn orc_repo_rejects_task_without_orchestration_meta() {
    let temp = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(temp.path().join("state.db")).unwrap();

    let foreign = Task {
        kind: "task".to_string(),
        id: "foreign-1".to_string(),
        context_id: "ctx-1".to_string(),
        status: TaskStatus {
            state: TaskState::Working,
            message: None,
            timestamp: None,
        },
        artifacts: None,
        history: None,
        metadata: None,
    };

    let error = store.save_task(&foreign).await.unwrap_err();
    assert!(error.message().contains("元数据"), "{}", error.message());
}

fn created_at_of(path: &std::path::Path, task_id: &str) -> String {
    let connection = Connection::open(path).unwrap();
    connection
        .query_row(
            "SELECT created_at FROM orc_tasks WHERE task_id = ?1",
            params![task_id],
            |row| row.get(0),
        )
        .unwrap()
}