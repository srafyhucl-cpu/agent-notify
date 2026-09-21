use std::{
    env,
    ffi::OsString,
    fs::File,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    time::SystemTime,
};

use rusqlite::{Connection, OpenFlags};

/// Codex 状态库里按优先级排列的标题列，与 Go 版标题链保持一致。
const TITLE_COLUMNS: [(&str, TitleSource); 3] = [
    ("name", TitleSource::ThreadName),
    ("title", TitleSource::ThreadTitle),
    ("first_user_message", TitleSource::ThreadFirstUserMessage),
];
const STATE_DATABASE_PREFIX: &str = "state_";
const STATE_DATABASE_SUFFIX: &str = ".sqlite";
const SESSION_INDEX_FILE_NAME: &str = "session_index.jsonl";
/// 会话索引可能很大，只读取头部有界字节，避免标题解析拖住通知。
const SESSION_INDEX_MAX_BYTES: u64 = 4 * 1024 * 1024;
const CODEX_HOME_ENV: &str = "CODEX_HOME";
const USER_PROFILE_ENV: &str = "USERPROFILE";
const HOME_ENV: &str = "HOME";
pub const DEFAULT_TITLE: &str = "跑完了";

/// 标题最终命中来源；顺序即降级顺序。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TitleSource {
    ThreadName,
    ThreadTitle,
    ThreadFirstUserMessage,
    SessionIndex,
    Payload,
    Default,
}

impl TitleSource {
    /// 稳定诊断标识，与 Go 版标题来源同名。
    pub const fn id(self) -> &'static str {
        match self {
            Self::ThreadName => "threads.name",
            Self::ThreadTitle => "threads.title",
            Self::ThreadFirstUserMessage => "threads.first_user_message",
            Self::SessionIndex => "session_index.jsonl",
            Self::Payload => "codex_payload",
            Self::Default => "fallback",
        }
    }

    /// 用户可见的降级说明只描述来源，不包含线程 ID 或文件路径。
    const fn label(self) -> &'static str {
        match self {
            Self::ThreadName => "会话名",
            Self::ThreadTitle => "线程标题",
            Self::ThreadFirstUserMessage => "首条消息",
            Self::SessionIndex => "会话索引",
            Self::Payload => "任务摘要",
            Self::Default => "默认标题",
        }
    }
}

/// 标题解析结果：状态库读取失败时通过 `degradation` 在正文里标记降级来源。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TitleResolution {
    pub name: String,
    pub source: TitleSource,
    pub degradation: Option<String>,
}

/// `%CODEX_HOME%` 优先，其次 `%USERPROFILE%\.codex`；都取不到时返回 None，标题链降级。
pub fn default_codex_home() -> Option<PathBuf> {
    if let Some(configured) = non_empty_env(CODEX_HOME_ENV) {
        return Some(PathBuf::from(configured));
    }
    let home = non_empty_env(USER_PROFILE_ENV).or_else(|| non_empty_env(HOME_ENV))?;
    Some(PathBuf::from(home).join(".codex"))
}

/// 按 `threads.name → threads.title → threads.first_user_message → session_index.jsonl
/// → payload 首条消息 → 默认标题` 的顺序解析标题。
pub fn resolve_title(
    codex_home: Option<&Path>,
    thread_id: Option<&str>,
    payload_title: Option<&str>,
) -> TitleResolution {
    let payload_title = clean_title(payload_title.unwrap_or_default());
    let Some(thread_id) = thread_id.map(str::trim).filter(|value| !value.is_empty()) else {
        return payload_or_default(payload_title, None);
    };
    let Some(codex_home) = codex_home else {
        return payload_or_default(payload_title, Some("无法确定 Codex 主目录"));
    };

    let (database_title, database_failure) = lookup_database_title(codex_home, thread_id);
    if let Some((name, source)) = database_title {
        return TitleResolution {
            name,
            source,
            degradation: None,
        };
    }
    if let Ok(Some(name)) = lookup_session_index_title(codex_home, thread_id) {
        return finish(name, TitleSource::SessionIndex, database_failure);
    }
    payload_or_default(payload_title, database_failure)
}

fn payload_or_default(
    payload_title: Option<String>,
    failure: Option<&'static str>,
) -> TitleResolution {
    match payload_title {
        Some(name) => finish(name, TitleSource::Payload, failure),
        None => finish(DEFAULT_TITLE.to_owned(), TitleSource::Default, failure),
    }
}

fn finish(name: String, source: TitleSource, failure: Option<&'static str>) -> TitleResolution {
    TitleResolution {
        name,
        source,
        degradation: failure
            .map(|reason| format!("标题读取失败：{reason}，已回退为{}。", source.label())),
    }
}

/// 状态库读取结果；第二个值只在“没有任何可查询的状态库”时为降级原因。
fn lookup_database_title(
    codex_home: &Path,
    thread_id: &str,
) -> (Option<(String, TitleSource)>, Option<&'static str>) {
    let databases = discover_state_databases(codex_home);
    if databases.is_empty() {
        return (None, Some("未找到 Codex 会话数据库"));
    }

    let mut read_failed = false;
    for database in databases {
        match query_thread_title(&database, thread_id) {
            Ok(Some(hit)) => return (Some(hit), None),
            Ok(None) => {}
            Err(_) => read_failed = true,
        }
    }
    if read_failed {
        return (None, Some("无法读取 Codex 会话数据库"));
    }
    (None, None)
}

/// 按版本号从新到旧发现 `state_*.sqlite`；版本号无法解析的排在最后。
fn discover_state_databases(codex_home: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(codex_home) else {
        return Vec::new();
    };
    let mut candidates: Vec<StateDatabase> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name();
            let name = name.to_str()?;
            let version = name
                .strip_prefix(STATE_DATABASE_PREFIX)?
                .strip_suffix(STATE_DATABASE_SUFFIX)
                .and_then(|text| text.parse::<u64>().ok());
            let metadata = entry.metadata().ok()?;
            if !metadata.is_file() {
                return None;
            }
            Some(StateDatabase {
                path: entry.path(),
                version,
                modified: metadata.modified().ok(),
            })
        })
        .collect();
    candidates.sort_by(|left, right| {
        match (left.version, right.version) {
            (Some(left_version), Some(right_version)) if left_version != right_version => {
                return right_version.cmp(&left_version);
            }
            (Some(_), None) => return std::cmp::Ordering::Less,
            (None, Some(_)) => return std::cmp::Ordering::Greater,
            _ => {}
        }
        match (left.modified, right.modified) {
            (Some(left_time), Some(right_time)) if left_time != right_time => {
                return right_time.cmp(&left_time);
            }
            _ => {}
        }
        left.path.cmp(&right.path)
    });
    candidates
        .into_iter()
        .map(|candidate| candidate.path)
        .collect()
}

struct StateDatabase {
    path: PathBuf,
    version: Option<u64>,
    modified: Option<SystemTime>,
}

/// 只读查询一个状态库；缺少标题列只影响该列，只有所有列都查不动才算数据库不可读。
fn query_thread_title(
    database: &Path,
    thread_id: &str,
) -> Result<Option<(String, TitleSource)>, rusqlite::Error> {
    let connection = Connection::open_with_flags(database, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut queryable = false;
    let mut last_error = None;
    for (column, source) in TITLE_COLUMNS {
        let sql = format!("SELECT {column} FROM threads WHERE id = ?1 LIMIT 1");
        match connection.query_row(&sql, [thread_id], |row| row.get::<_, Option<String>>(0)) {
            Ok(Some(value)) => {
                queryable = true;
                if let Some(name) = clean_title(&value) {
                    return Ok(Some((name, source)));
                }
            }
            Ok(None) => queryable = true,
            Err(rusqlite::Error::QueryReturnedNoRows) => queryable = true,
            Err(error) => last_error = Some(error),
        }
    }
    if queryable {
        return Ok(None);
    }
    match last_error {
        Some(error) => Err(error),
        None => Ok(None),
    }
}

fn lookup_session_index_title(
    codex_home: &Path,
    thread_id: &str,
) -> std::io::Result<Option<String>> {
    let path = codex_home.join(SESSION_INDEX_FILE_NAME);
    let file = match File::open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let reader = BufReader::new(file.take(SESSION_INDEX_MAX_BYTES));
    let mut name = None;
    for line in reader.lines() {
        let Ok(line) = line else { break };
        let Ok(entry) = serde_json::from_str::<SessionIndexEntry>(&line) else {
            continue;
        };
        if entry.id == thread_id {
            if let Some(candidate) = clean_title(entry.thread_name.as_deref().unwrap_or_default()) {
                // 同一线程可能被追加多条索引，取最后一条与 Go 版一致。
                name = Some(candidate);
            }
        }
    }
    Ok(name)
}

#[derive(serde::Deserialize)]
struct SessionIndexEntry {
    id: String,
    #[serde(default)]
    thread_name: Option<String>,
}

/// 折叠空白，避免把换行带进通知标题。
fn clean_title(value: &str) -> Option<String> {
    let cleaned = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

fn non_empty_env(name: &str) -> Option<OsString> {
    env::var_os(name).filter(|value| !value.is_empty())
}
