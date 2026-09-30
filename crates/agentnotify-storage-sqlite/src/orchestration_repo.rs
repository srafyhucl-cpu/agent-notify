//! 编排任务仓储的 SQLite 实现（P1-1，B 方案核心）：
//! `SqliteStore` 实现 `agentnotify-orchestration` 的 [`OrcTaskRepository`] 契约。
//!
//! 与 channel_account_store.rs 同模式：自跑 `self.run(|conn| ...)`、`storage_error`、
//! `row_codec` 时间戳、`sqlite_helpers::query_optional`。
//!
//! 存储模型：`orc_tasks` 表一行一个任务，`a2a_task_json` 保存 A2A Task 完整 JSON
//! （唯一事实源），`task_id / workflow_id / goal / created_at` 是索引列。写入幂等
//! （按 task_id 覆盖），`created_at` 仅在首次插入时记录、更新不覆盖。

use agentnotify_application::StoreError;
use agentnotify_orchestration::{OrcRepositoryError, OrcTask, OrcTaskRepository, Task};
use async_trait::async_trait;
use rusqlite::params;

use crate::SqliteStore;
use crate::migrations::storage_error;
use crate::row_codec::{timestamp_column, timestamp_to_db};
use crate::sqlite_helpers::query_optional;

const ORC_TASK_COLUMNS: &str = "task_id, workflow_id, goal, a2a_task_json, created_at";

/// 把持久化层错误映射为仓储契约错误：保留稳定错误码与中文用户消息。
fn repository_error(error: StoreError) -> OrcRepositoryError {
    OrcRepositoryError::new(error.code(), error.message())
}

#[async_trait]
impl OrcTaskRepository for SqliteStore {
    async fn save_task(&self, task: &Task) -> Result<(), OrcRepositoryError> {
        let json = serde_json::to_string(task).map_err(|e| {
            OrcRepositoryError::new("orc_serialize_failed", format!("序列化编排任务失败：{e}"))
        })?;
        let meta = OrcTask::from_a2a(task.clone())
            .and_then(|orc| orc.meta())
            .map_err(|e| {
                OrcRepositoryError::new(
                    "orc_meta_invalid",
                    format!("编排任务元数据缺失或损坏：{}", e.message),
                )
            })?;
        let task_id = task.id.clone();
        let workflow_id = meta.workflow_id;
        let goal = meta.goal;
        let now = timestamp_to_db(agentnotify_domain::Timestamp::now_utc());
        self.run(move |connection| {
            connection
                .execute(
                    "INSERT INTO orc_tasks(task_id, workflow_id, goal, a2a_task_json, created_at) \
                     VALUES (?1, ?2, ?3, ?4, ?5) \
                     ON CONFLICT(task_id) DO UPDATE SET \
                        workflow_id = excluded.workflow_id, \
                        goal = excluded.goal, \
                        a2a_task_json = excluded.a2a_task_json",
                    params![task_id, workflow_id, goal, json, now],
                )
                .map_err(|error| storage_error("保存编排任务失败", error))?;
            Ok(())
        })
        .await
        .map_err(repository_error)
    }

    async fn get_task(&self, task_id: &str) -> Result<Option<Task>, OrcRepositoryError> {
        let task_id = task_id.to_owned();
        self.run(move |connection| {
            query_optional(
                connection,
                &format!("SELECT {ORC_TASK_COLUMNS} FROM orc_tasks WHERE task_id = ?1"),
                params![task_id],
                task_from_row,
            )
        })
        .await
        .map_err(repository_error)
    }

    async fn list_tasks(&self) -> Result<Vec<Task>, OrcRepositoryError> {
        self.run(|connection| {
            let mut statement = connection
                .prepare(&format!(
                    "SELECT {ORC_TASK_COLUMNS} FROM orc_tasks ORDER BY created_at DESC, task_id"
                ))
                .map_err(|error| storage_error("准备编排任务查询失败", error))?;
            let mut rows = statement
                .query([])
                .map_err(|error| storage_error("查询编排任务失败", error))?;
            let mut tasks = Vec::new();
            while let Some(row) = rows
                .next()
                .map_err(|error| storage_error("读取编排任务行失败", error))?
            {
                tasks.push(task_from_row(row)?);
            }
            Ok(tasks)
        })
        .await
        .map_err(repository_error)
    }

    async fn delete_task(&self, task_id: &str) -> Result<(), OrcRepositoryError> {
        let task_id = task_id.to_owned();
        self.run(move |connection| {
            connection
                .execute("DELETE FROM orc_tasks WHERE task_id = ?1", params![task_id])
                .map_err(|error| storage_error("删除编排任务失败", error))?;
            Ok(())
        })
        .await
        .map_err(repository_error)
    }
}

/// 从 orc_tasks 行还原 A2A Task；JSON 损坏或内部 id 与主键不一致 → 明确报错（不猜测兜底）。
/// 旧任务（升级前创建）meta 缺 createdAt：用表里的 created_at 回填，保证创建时间可见。
fn task_from_row(row: &rusqlite::Row<'_>) -> Result<Task, StoreError> {
    let task_id: String = crate::row_codec::column(row, "task_id")?;
    let json: String = crate::row_codec::column(row, "a2a_task_json")?;
    let mut task: Task = serde_json::from_str(&json)
        .map_err(|_| StoreError::corrupted("编排任务 JSON 损坏，无法读取任务记录"))?;
    if task.id != task_id {
        return Err(StoreError::corrupted("编排任务主键与任务内容不一致"));
    }
    let created_at = timestamp_column(row, "created_at")?;
    backfill_created_at(&mut task, &created_at.to_rfc3339());
    Ok(task)
}

/// 旧任务 meta 缺 createdAt 时回填（只补展示用字段；已有值不覆盖）。
fn backfill_created_at(task: &mut Task, created_at: &str) {
    let Some(metadata) = task
        .metadata
        .as_mut()
        .and_then(serde_json::Value::as_object_mut)
    else {
        return;
    };
    let Some(orc) = metadata
        .get_mut("orc")
        .and_then(serde_json::Value::as_object_mut)
    else {
        return;
    };
    if orc.contains_key("createdAt") {
        return;
    }
    orc.insert(
        "createdAt".to_string(),
        serde_json::Value::String(created_at.to_string()),
    );
}
