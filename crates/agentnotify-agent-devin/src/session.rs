use std::{
    env,
    path::{Path, PathBuf},
    time::Duration,
};

use agentnotify_agent_sdk::AgentError;
use agentnotify_domain::SafeError;
use rusqlite::{Connection, OpenFlags, OptionalExtension};

/// CLI 会话库路径覆盖变量，测试与便携部署用。
const SESSIONS_DB_ENV: &str = "AGENT_NOTIFY_DEVIN_SESSIONS_DB";
/// 桌面端状态库路径覆盖变量，测试与便携部署用。
const DESKTOP_DB_ENV: &str = "AGENT_NOTIFY_DEVIN_DESKTOP_DB";
const APP_DATA_ENV: &str = "APPDATA";
const CLI_DATABASE_SUBDIR: [&str; 3] = ["devin", "cli", "sessions.db"];
const DESKTOP_DATABASE_SUBDIR: [&str; 4] = ["devin", "User", "globalStorage", "state.vscdb"];
const SESSIONS_QUERY: &str = "SELECT id, title FROM sessions WHERE id = ?1 LIMIT 1";
const DESKTOP_SESSION_QUERY: &str = "SELECT value FROM ItemTable WHERE key = ?1 LIMIT 1";
/// 桌面端把 ACP 会话登记为 `acp/devin-cli/<会话号>`，精确回复命令只认这个标识。
const DESKTOP_SESSION_KEY_PREFIX: &str = "windsurf.acp.sessioninfo.session.acp/devin-cli/";
/// 桌面端可能正持有数据库锁，读操作等待一小段时间再报错。
const SQLITE_BUSY_TIMEOUT: Duration = Duration::from_millis(500);
/// 会话标题限长，避免异常元数据把通知标题撑爆。
const TITLE_MAX_CHARS: usize = 80;

/// Devin CLI 会话元数据（只读 `%APPDATA%\devin\cli\sessions.db`）。
#[derive(Clone, Debug)]
pub struct DevinSessions {
    database: Option<PathBuf>,
}

impl DevinSessions {
    /// 使用显式会话库路径；测试与自定义安装位置用。
    pub fn new(database: impl Into<PathBuf>) -> Self {
        Self {
            database: Some(database.into()),
        }
    }

    /// `AGENT_NOTIFY_DEVIN_SESSIONS_DB` 优先，其次 `%APPDATA%\devin\cli\sessions.db`。
    pub fn from_default_location() -> Self {
        Self {
            database: default_database(SESSIONS_DB_ENV, &CLI_DATABASE_SUBDIR),
        }
    }

    /// 不读取任何用户目录的测试实例。
    pub fn without_database() -> Self {
        Self { database: None }
    }

    /// 按完整会话号读取标题；不接受模糊匹配，失败只降级标题、不影响推送。
    pub fn lookup_title(&self, session_id: &str) -> Result<String, AgentError> {
        let session_id = session_id.trim();
        if session_id.is_empty() {
            return Err(failed(
                "devin_session_lookup_failed",
                "Devin 会话号为空，无法读取会话标题",
            ));
        }
        let path = self.database.as_deref().ok_or_else(|| {
            failed(
                "devin_session_lookup_failed",
                "无法确定 Devin 会话库路径，请检查 APPDATA",
            )
        })?;
        let connection = open_read_only(path)?;
        let row: Option<(String, Option<String>)> = connection
            .query_row(SESSIONS_QUERY, [session_id], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .optional()
            .map_err(|_| {
                failed(
                    "devin_session_lookup_failed",
                    "无法读取 Devin 会话元数据，请确认 Devin 已关闭或稍后重试",
                )
            })?;
        let Some((found_id, title)) = row else {
            return Err(failed(
                "devin_session_not_found",
                "Devin 会话库中没有该会话，已使用默认标题",
            ));
        };
        // 会话号对不上说明存储结构已变化，宁可降级标题也不能用别的会话顶替。
        if found_id.trim() != session_id {
            return Err(failed(
                "devin_session_lookup_failed",
                "Devin 会话库返回了不匹配的会话，已使用默认标题",
            ));
        }
        let title = clean_title(title.as_deref().unwrap_or_default());
        if title.is_empty() {
            return Err(failed(
                "devin_session_lookup_failed",
                "Devin 会话没有可用标题，已使用默认标题",
            ));
        }
        Ok(title)
    }
}

/// Devin 桌面端状态库（只读 `%APPDATA%\devin\User\globalStorage\state.vscdb`）。
#[derive(Clone, Debug)]
pub struct DevinDesktopSessions {
    database: Option<PathBuf>,
}

impl DevinDesktopSessions {
    /// 使用显式状态库路径；测试与自定义安装位置用。
    pub fn new(database: impl Into<PathBuf>) -> Self {
        Self {
            database: Some(database.into()),
        }
    }

    /// `AGENT_NOTIFY_DEVIN_DESKTOP_DB` 优先，其次 `%APPDATA%\devin\User\globalStorage\state.vscdb`。
    pub fn from_default_location() -> Self {
        Self {
            database: default_database(DESKTOP_DB_ENV, &DESKTOP_DATABASE_SUBDIR),
        }
    }

    /// 不读取任何用户目录的测试实例。
    pub fn without_database() -> Self {
        Self { database: None }
    }

    /// 把 CLI 会话号解析成桌面端 ACP Cascade 标识；未登记时明确失败，绝不回退到最近会话。
    pub fn resolve_cascade(&self, session_id: &str) -> Result<String, AgentError> {
        let session_id = session_id.trim();
        if session_id.is_empty() {
            return Err(AgentError::InvalidInput);
        }
        let path = self.database.as_deref().ok_or_else(|| {
            failed(
                "devin_desktop_db_missing",
                "无法确定 Devin 桌面端状态库路径，请检查 APPDATA；回复不会改投到最近会话",
            )
        })?;
        if !path.is_file() {
            return Err(failed(
                "devin_desktop_db_missing",
                "未找到 Devin 桌面端会话状态库：请先在 Devin 中打开该会话再引用回复",
            ));
        }
        let connection = open_read_only(path)?;
        let key = format!("{DESKTOP_SESSION_KEY_PREFIX}{session_id}");
        let value: Option<String> = connection
            .query_row(DESKTOP_SESSION_QUERY, [key], |row| row.get(0))
            .optional()
            .map_err(|_| {
                failed(
                    "devin_desktop_db_unreadable",
                    "无法读取 Devin 桌面端会话状态库：请确认 Devin 桌面端仍在运行后重试",
                )
            })?;
        let Some(value) = value else {
            return Err(failed(
                "devin_session_not_found",
                "Devin 桌面端尚未登记该会话：请先在 Devin 中打开该会话后再引用回复",
            ));
        };
        let cascade_id = parse_desktop_session_id(&value).ok_or_else(|| {
            failed(
                "devin_desktop_session_invalid",
                "Devin 桌面端会话元数据无效，请重启 Devin 后重试",
            )
        })?;
        // 会话号对不上说明桌面端存储结构已变化，宁可直接报错也不能把回复投给别的会话。
        if !cascade_id.ends_with(&format!("/{session_id}")) {
            return Err(failed(
                "devin_desktop_session_mismatch",
                "Devin 桌面端登记的会话与通知不一致，已拒绝回复以免投错会话",
            ));
        }
        Ok(cascade_id)
    }
}

/// 只解析精确回复需要的 `info.sessionId` 字段。
fn parse_desktop_session_id(value: &str) -> Option<String> {
    let root: serde_json::Value = serde_json::from_str(value).ok()?;
    let session_id = root
        .get("info")?
        .get("sessionId")?
        .as_str()?
        .trim()
        .to_owned();
    (!session_id.is_empty()).then_some(session_id)
}

fn default_database(env_name: &str, subdir: &[&str]) -> Option<PathBuf> {
    if let Some(configured) = env::var_os(env_name).filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(configured));
    }
    let app_data = env::var_os(APP_DATA_ENV).filter(|value| !value.is_empty())?;
    let mut path = PathBuf::from(app_data);
    for part in subdir {
        path.push(part);
    }
    Some(path)
}

/// 只读打开，绝不改动 Devin 自己的数据。
fn open_read_only(path: &Path) -> Result<Connection, AgentError> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|_| {
        failed(
            "devin_database_unreadable",
            "无法只读打开 Devin 数据库：请确认 Devin 已安装且当前用户有读取权限",
        )
    })?;
    connection.busy_timeout(SQLITE_BUSY_TIMEOUT).map_err(|_| {
        failed(
            "devin_database_unreadable",
            "无法读取 Devin 数据库，请稍后重试",
        )
    })?;
    Ok(connection)
}

/// 与 Go 版 `cleanTitle` 一致：折叠空白并限长。
fn clean_title(value: &str) -> String {
    let collapsed = value.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.chars().take(TITLE_MAX_CHARS).collect()
}

fn failed(code: &str, message: &str) -> AgentError {
    AgentError::Failed(safe_error(code, message))
}

fn safe_error(code: &str, message: &str) -> SafeError {
    SafeError::new(code, message).expect("Devin 错误常量必须是有效安全错误")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_session_id_requires_the_info_field() {
        assert_eq!(
            parse_desktop_session_id(r#"{"info":{"sessionId":"acp/devin-cli/session-1"}}"#),
            Some("acp/devin-cli/session-1".to_owned())
        );
        for invalid in [
            "",
            "null",
            "{}",
            r#"{"info":{}}"#,
            r#"{"info":{"sessionId":"   "}}"#,
            r#"{"sessionId":"acp/devin-cli/session-1"}"#,
        ] {
            assert_eq!(parse_desktop_session_id(invalid), None, "{invalid}");
        }
    }

    #[test]
    fn title_cleaning_collapses_whitespace_and_bounds_length() {
        assert_eq!(clean_title("  Devin\t 会话  "), "Devin 会话");
        assert_eq!(
            clean_title("字".repeat(TITLE_MAX_CHARS + 10).as_str())
                .chars()
                .count(),
            TITLE_MAX_CHARS
        );
    }
}
