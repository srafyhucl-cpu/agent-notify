//! 编排会话通知过滤器（§5「只听项目经理」）：编排会话的原始完成事件一律不推微信。
//!
//! 判定：事件 `sessionId` 能解析为编排会话 `task-<id>-step-<n>` **且**任务确实存在于
//! 编排任务表（`orc_tasks`）→ 抑制通知，只走汇报回注与产出记录；其它一律放行
//! （普通 OpenCode 会话行为完全不变）。
//!
//! 实现取舍：`AgentEventFilter` 是同步接口，而任务表在 SQLite 中——本过滤器持有一条
//! **只读**连接做单行主键查询（WAL 模式下读不阻塞写；代理事件是低频事件，单次查询微秒级）。
//! 任何读取失败（数据库缺失/损坏/不可读）按「放行」处理并告警：不能证明是编排会话时，
//! 宁可不抑制（普通会话的通知优先），也不误伤用户。

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use agentnotify_agent_sdk::AgentEventEnvelope;
use agentnotify_runtime::AgentEventFilter;
use rusqlite::{Connection, OpenFlags};

use super::orc_report_observer::parse_orc_session;

/// 只读连接的忙等上限。
const FILTER_BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// 编排会话通知过滤器：持有编排数据库路径，按需建立只读连接。
pub struct OrcEventFilter {
    db_path: PathBuf,
    connection: Mutex<Option<Connection>>,
}

impl OrcEventFilter {
    /// `db_path` = 宿主 state.db（含 `orc_tasks` 表；任务仓储与运行时共用同一文件）。
    pub fn new(db_path: impl Into<PathBuf>) -> Self {
        Self {
            db_path: db_path.into(),
            connection: Mutex::new(None),
        }
    }

    /// 任务是否存在（单行主键查询）；连接惰性建立。
    fn task_exists(&self, task_id: &str) -> Result<bool, String> {
        let mut guard = self
            .connection
            .lock()
            .map_err(|_| "只读连接锁不可用".to_string())?;
        if guard.is_none() {
            let connection = open_readonly(&self.db_path)?;
            *guard = Some(connection);
        }
        let connection = guard.as_ref().expect("连接已建立");
        QueryTaskExist::query(connection, task_id)
    }
}

/// 单行存在性查询（独立小函数便于错误信息统一）。
struct QueryTaskExist;

impl QueryTaskExist {
    fn query(connection: &Connection, task_id: &str) -> Result<bool, String> {
        let mut statement = connection
            .prepare("SELECT 1 FROM orc_tasks WHERE task_id = ?1 LIMIT 1")
            .map_err(|error| format!("准备任务查询失败：{error}"))?;
        let mut rows = statement
            .query(rusqlite::params![task_id])
            .map_err(|error| format!("查询任务失败：{error}"))?;
        let exists = rows
            .next()
            .map_err(|error| format!("读取任务行失败：{error}"))?
            .is_some();
        Ok(exists)
    }
}

fn open_readonly(path: &Path) -> Result<Connection, String> {
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|error| format!("打开编排任务库失败：{error}"))?;
    let _ = connection.busy_timeout(FILTER_BUSY_TIMEOUT);
    Ok(connection)
}

impl AgentEventFilter for OrcEventFilter {
    fn allow_notification(&self, envelope: &AgentEventEnvelope) -> bool {
        let Some(session_id) = envelope
            .payload
            .get("sessionId")
            .and_then(serde_json::Value::as_str)
        else {
            return true;
        };
        let Some((task_id, _step)) = parse_orc_session(session_id) else {
            return true;
        };
        match self.task_exists(task_id) {
            Ok(true) => {
                tracing::debug!(
                    session_id,
                    task_id,
                    "编排会话事件：跳过通知创建（只走汇报回注）"
                );
                false
            }
            Ok(false) => true,
            Err(error) => {
                // 读不到任务表（数据库缺失/损坏/不可读）→ 放行 + 告警（不误伤普通会话）。
                tracing::warn!(
                    session_id,
                    "判断编排会话失败，按放行处理（通知可能多推）：{error}"
                );
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use agentnotify_domain::{AgentId, RequestId};
    use agentnotify_orchestration::{NotifyMode, OrcStore, Workflow};
    use agentnotify_storage_sqlite::SqliteStore;
    use std::sync::Arc;

    use super::*;

    fn envelope(session_id: Option<&str>) -> AgentEventEnvelope {
        let mut payload = serde_json::json!({ "eventType": "session.completed" });
        if let Some(session_id) = session_id {
            payload["sessionId"] = serde_json::Value::String(session_id.to_string());
        }
        AgentEventEnvelope {
            request_id: RequestId::new("req-orc-filter-test").expect("有效请求标识"),
            agent_id: AgentId::new("opencode").expect("有效 Agent 标识"),
            payload,
        }
    }

    fn open_sqlite(prefix: &str) -> (tempfile::TempDir, Arc<SqliteStore>) {
        let root = tempfile::Builder::new()
            .prefix(prefix)
            .tempdir_in(agentnotify_testkit::test_temp_root())
            .expect("测试临时目录必须可创建");
        let store = Arc::new(
            SqliteStore::open(root.path().join("state.db")).expect("SQLite 数据库必须可创建"),
        );
        (root, store)
    }

    /// 编排会话（任务表存在该任务）→ 抑制；普通会话/同形但无任务 → 放行。
    #[tokio::test]
    async fn suppresses_only_known_orchestration_sessions() {
        let (root, store) = open_sqlite("agentnotify-orc-filter-known-");
        let orc_store = OrcStore::with_repository(
            Workflow::preset(false).expect("预置工作流必须有效"),
            store.clone(),
        );
        let task = orc_store
            .create_task("过滤测试", NotifyMode::FinalOnly)
            .await
            .expect("创建任务必须成功");

        let filter = OrcEventFilter::new(root.path().join("state.db"));
        assert!(
            !filter.allow_notification(&envelope(Some(&format!("task-{}-step-2", task.id())))),
            "已知编排会话必须抑制通知"
        );
        assert!(
            !filter.allow_notification(&envelope(Some(&format!("task-{}-step-9", task.id())))),
            "任务存在即抑制（步号不影响判定）"
        );
        assert!(
            filter.allow_notification(&envelope(Some("ses_not_orchestration"))),
            "普通会话必须放行"
        );
        assert!(
            filter.allow_notification(&envelope(Some(
                "task-00000000-0000-4000-8000-000000000000-step-1"
            ))),
            "同形但任务不存在必须放行"
        );
        assert!(
            filter.allow_notification(&envelope(None)),
            "无 sessionId 必须放行"
        );
    }

    /// 数据库不可读（文件缺失）→ 按放行处理，不误伤普通会话。
    #[test]
    fn missing_database_fails_open() {
        let root = tempfile::Builder::new()
            .prefix("agentnotify-orc-filter-missing-")
            .tempdir_in(agentnotify_testkit::test_temp_root())
            .expect("测试临时目录必须可创建");
        let filter = OrcEventFilter::new(root.path().join("no-such.db"));
        assert!(
            filter.allow_notification(&envelope(Some("task-some-id-step-1"))),
            "数据库缺失时不能证明是编排会话，必须放行"
        );
    }
}
