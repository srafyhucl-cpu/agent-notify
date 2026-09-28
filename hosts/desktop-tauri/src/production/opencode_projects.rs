//! OpenCode 已知项目读取（§3.1 工作目录下拉）：只读访问 OpenCode 本地数据库。
//!
//! 数据源：`~/.local/share/opencode/opencode.db` 的 `project` 表
//! （`worktree` = 目录、`name`、`time_active`），按最近活跃倒序。
//! 失败一律明确报错（不猜、不静默），由界面退回手动输入路径；只读打开，不修改 OpenCode 数据。

use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};

use crate::bridge::dto::OpencodeProjectDto;
use crate::bridge::error::CommandError;

/// 相对用户目录的 OpenCode 数据库路径（Windows 也按 XDG 约定）。
pub const OPENCODE_DB_RELATIVE: &str = ".local/share/opencode/opencode.db";
/// 找不到 OpenCode 数据库文件（界面应退回手动输入）。
pub const OPENCODE_DB_NOT_FOUND: &str = "opencode_db_not_found";
/// 打开/查询 OpenCode 数据库失败。
pub const OPENCODE_DB_READ_FAILED: &str = "opencode_db_read_failed";
/// 只读连接的忙等上限（OpenCode 自身可能在写）。
const READ_BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// 默认数据库路径：`<用户目录>/.local/share/opencode/opencode.db`。
pub fn default_db_path() -> Result<PathBuf, CommandError> {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .ok_or_else(|| {
            CommandError::new(
                "opencode_home_missing",
                "无法确定用户目录，不能读取 OpenCode 项目列表",
            )
        })?;
    Ok(PathBuf::from(home).join(OPENCODE_DB_RELATIVE))
}

/// 读取默认位置的 OpenCode 项目列表（异步入口：SQLite 为阻塞调用，放专用阻塞线程）。
pub async fn list_projects() -> Result<Vec<OpencodeProjectDto>, CommandError> {
    let path = default_db_path()?;
    tokio::task::spawn_blocking(move || list_projects_from_db(&path))
        .await
        .map_err(|_| {
            CommandError::new(
                OPENCODE_DB_READ_FAILED,
                "读取 OpenCode 项目列表任务异常，请重试",
            )
        })?
}

/// 从指定数据库读取项目列表（可单测）：只读打开，失败带中文原因。
pub fn list_projects_from_db(path: &Path) -> Result<Vec<OpencodeProjectDto>, CommandError> {
    if !path.is_file() {
        return Err(CommandError::new(
            OPENCODE_DB_NOT_FOUND,
            format!(
                "未找到 OpenCode 项目数据库：{}（可直接手动输入工作目录）",
                path.display()
            ),
        ));
    }
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|error| read_failed(format!("打开数据库失败：{error}")))?;
    let _ = connection.busy_timeout(READ_BUSY_TIMEOUT);

    let mut statement = connection
        .prepare("SELECT worktree, name, time_active FROM project ORDER BY time_active DESC")
        .map_err(|error| read_failed(format!("项目表不可读：{error}")))?;
    let mut rows = statement
        .query([])
        .map_err(|error| read_failed(format!("查询项目表失败：{error}")))?;

    let mut projects = Vec::new();
    while let Some(row) = rows
        .next()
        .map_err(|error| read_failed(format!("读取项目行失败：{error}")))?
    {
        let directory: String = row
            .get(0)
            .map_err(|error| read_failed(format!("工作目录字段不可读：{error}")))?;
        let directory = directory.trim().to_string();
        if directory.is_empty() {
            // 无目录的项目行对「选择工作目录」没有意义，跳过（不影响其它行）。
            continue;
        }
        let name: Option<String> = row
            .get(1)
            .map_err(|error| read_failed(format!("项目名字段不可读：{error}")))?;
        let last_active_at: Option<i64> = row
            .get(2)
            .map_err(|error| read_failed(format!("活跃时间字段不可读：{error}")))?;
        projects.push(OpencodeProjectDto {
            directory,
            name: name
                .map(|value| value.trim().to_string())
                .filter(|v| !v.is_empty()),
            last_active_at,
        });
    }
    Ok(projects)
}

fn read_failed(detail: String) -> CommandError {
    CommandError::new(
        OPENCODE_DB_READ_FAILED,
        format!("读取 OpenCode 项目列表失败：{detail}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 造一个最小 OpenCode 项目库：project(worktree, name, time_active)。
    fn write_project_db(path: &Path, rows: &[(&str, Option<&str>, Option<i64>)]) {
        let connection = Connection::open(path).expect("创建测试库必须成功");
        connection
            .execute_batch("CREATE TABLE project (worktree TEXT, name TEXT, time_active INTEGER);")
            .expect("建表必须成功");
        for (worktree, name, time_active) in rows {
            connection
                .execute(
                    "INSERT INTO project(worktree, name, time_active) VALUES (?1, ?2, ?3)",
                    rusqlite::params![worktree, name, time_active],
                )
                .expect("插入项目行必须成功");
        }
    }

    /// 正常读取：按 time_active 倒序，name 去空白，空目录行跳过。
    #[test]
    fn list_projects_orders_by_activity_and_skips_empty_dirs() {
        let dir = tempfile::tempdir().expect("测试临时目录必须可创建");
        let db = dir.path().join("opencode.db");
        write_project_db(
            &db,
            &[
                ("D:/Project/old", Some("旧项目"), Some(100)),
                ("D:/Project/new", Some("  新项目  "), Some(300)),
                ("  ", Some("空目录"), Some(400)),
                ("D:/Project/mid", None, Some(200)),
            ],
        );

        let projects = list_projects_from_db(&db).expect("读取项目列表必须成功");
        let dirs: Vec<&str> = projects.iter().map(|p| p.directory.as_str()).collect();
        assert_eq!(
            dirs,
            vec!["D:/Project/new", "D:/Project/mid", "D:/Project/old"],
            "必须按最近活跃倒序且跳过空目录"
        );
        assert_eq!(projects[0].name.as_deref(), Some("新项目"));
        assert_eq!(projects[0].last_active_at, Some(300));
        assert_eq!(projects[1].name, None);
    }

    /// 数据库缺失 → 明确错误（界面据此退回手动输入），不静默空列表。
    #[test]
    fn missing_database_reports_clear_error() {
        let dir = tempfile::tempdir().expect("测试临时目录必须可创建");
        let error =
            list_projects_from_db(&dir.path().join("missing.db")).expect_err("数据库缺失必须报错");
        assert_eq!(error.code(), OPENCODE_DB_NOT_FOUND);
        assert!(error.message().contains("未找到 OpenCode 项目数据库"));
        assert!(error.message().contains("手动输入"));
    }

    /// 表结构不符（损坏/版本不兼容）→ 明确读取错误，不猜测兜底。
    #[test]
    fn broken_schema_reports_read_failure() {
        let dir = tempfile::tempdir().expect("测试临时目录必须可创建");
        let db = dir.path().join("opencode.db");
        let connection = Connection::open(&db).expect("创建测试库必须成功");
        connection
            .execute_batch("CREATE TABLE unrelated (id INTEGER);")
            .expect("建表必须成功");
        drop(connection);

        let error = list_projects_from_db(&db).expect_err("表结构不符必须报错");
        assert_eq!(error.code(), OPENCODE_DB_READ_FAILED);
        assert!(error.message().contains("读取 OpenCode 项目列表失败"));
    }
}
