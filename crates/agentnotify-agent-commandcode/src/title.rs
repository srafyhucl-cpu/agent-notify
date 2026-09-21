use std::{
    env,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

use serde_json::Value;

/// 标题链最后一级的默认标题，与 Go 版 mod 的兜底行为一致。
pub const DEFAULT_TITLE: &str = "任务已完成";
/// 项目数据目录覆盖变量，测试与便携部署用。
const PROJECTS_DIR_ENV: &str = "AGENT_NOTIFY_COMMANDCODE_PROJECTS_DIR";
const USER_PROFILE_ENV: &str = "USERPROFILE";
const HOME_ENV: &str = "HOME";
const PROJECTS_SUBDIR: [&str; 2] = [".commandcode", "projects"];
const META_SUFFIX: &str = ".meta.json";
const TRANSCRIPT_SUFFIX: &str = ".jsonl";
/// transcript 可能很大，只读头部有界字节（与 Go 版 mod 的 2 MiB 一致）。
const TRANSCRIPT_SCAN_MAX_BYTES: u64 = 2 * 1024 * 1024;
/// 标题限长，与 Go 版 mod 的 `TITLE_MAX_CHARS` 一致。
const TITLE_MAX_CHARS: usize = 40;
/// 标题降级提示，文案与 Go 版 mod 的默认标题路径一致。
const TITLE_DEGRADATION: &str = "未能读取 CommandCode 会话标题，已使用默认标题。";

/// 标题最终命中来源；顺序即降级顺序。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TitleSource {
    /// mod 在 `session_titled` 事件里给出的会话标题。
    Payload,
    /// `<sessionId>.meta.json` 里的会话标题。
    Meta,
    /// transcript 首条用户消息。
    Transcript,
    Default,
}

impl TitleSource {
    /// 稳定诊断标识，与 Go 版标题来源同名。
    pub const fn id(self) -> &'static str {
        match self {
            Self::Payload => "session_titled",
            Self::Meta => "session_meta",
            Self::Transcript => "session_transcript",
            Self::Default => "fallback",
        }
    }
}

/// 标题解析结果：全部来源都读不到时通过 `degradation` 在正文里标记降级来源。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TitleResolution {
    pub name: String,
    pub source: TitleSource,
    pub degradation: Option<String>,
}

/// 只读 Command Code 本地会话数据（`%USERPROFILE%\.commandcode\projects`）。
#[derive(Clone, Debug)]
pub struct CommandCodeSessions {
    projects_root: Option<PathBuf>,
}

impl CommandCodeSessions {
    /// 使用显式项目数据目录；测试与自定义安装位置用。
    pub fn new(projects_root: impl Into<PathBuf>) -> Self {
        Self {
            projects_root: Some(projects_root.into()),
        }
    }

    /// `AGENT_NOTIFY_COMMANDCODE_PROJECTS_DIR` 优先，其次 `%USERPROFILE%\.commandcode\projects`。
    pub fn from_default_location() -> Self {
        Self {
            projects_root: default_projects_root(),
        }
    }

    /// 不读取任何用户目录的测试实例。
    pub fn without_backend() -> Self {
        Self {
            projects_root: None,
        }
    }

    pub fn projects_root(&self) -> Option<&Path> {
        self.projects_root.as_deref()
    }

    /// 标题链：`session_titled` 标题 → meta.json → transcript 首条用户消息 → 默认标题。
    pub fn resolve_title(&self, session_id: &str, payload_title: Option<&str>) -> TitleResolution {
        if let Some(name) = payload_title
            .map(clean_title)
            .filter(|name| !name.is_empty())
        {
            return TitleResolution {
                name,
                source: TitleSource::Payload,
                degradation: None,
            };
        }
        if let Some((name, source)) = self.lookup_local_title(session_id) {
            return TitleResolution {
                name,
                source,
                degradation: None,
            };
        }
        TitleResolution {
            name: DEFAULT_TITLE.to_owned(),
            source: TitleSource::Default,
            degradation: Some(TITLE_DEGRADATION.to_owned()),
        }
    }

    /// 本地标题链按会话号精确定位，绝不回退到最近会话。
    fn lookup_local_title(&self, session_id: &str) -> Option<(String, TitleSource)> {
        let root = self.projects_root.as_deref()?;
        let session_id = session_id.trim();
        if session_id.is_empty() {
            return None;
        }
        if let Some(name) = meta_title(root, session_id) {
            return Some((name, TitleSource::Meta));
        }
        transcript_title(root, session_id).map(|name| (name, TitleSource::Transcript))
    }
}

/// Command Code 把会话存在 `<projects>/<cwd slug>/<sessionId>.*`；会话号唯一，
/// 因此无需知道 slug 算法，直接扫描各项目目录定位。
fn project_directories(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.path())
        .collect()
}

/// 读 `<sessionId>.meta.json` 的 title；拿不到返回 None。
fn meta_title(root: &Path, session_id: &str) -> Option<String> {
    for directory in project_directories(root) {
        let meta = directory.join(format!("{session_id}{META_SUFFIX}"));
        let Ok(content) = std::fs::read_to_string(&meta) else {
            continue;
        };
        let Ok(parsed) = serde_json::from_str::<Value>(&content) else {
            continue;
        };
        let Some(title) = parsed.get("title").and_then(Value::as_str) else {
            continue;
        };
        let title = clean_title(title);
        if !title.is_empty() {
            return Some(title);
        }
    }
    None
}

/// 会话标题尚未生成时，用 transcript 首条用户消息兜底。
fn transcript_title(root: &Path, session_id: &str) -> Option<String> {
    for directory in project_directories(root) {
        let transcript = directory.join(format!("{session_id}{TRANSCRIPT_SUFFIX}"));
        let Ok(metadata) = std::fs::metadata(&transcript) else {
            continue;
        };
        if metadata.len() > TRANSCRIPT_SCAN_MAX_BYTES {
            continue;
        }
        let Some(content) = read_head(&transcript, TRANSCRIPT_SCAN_MAX_BYTES) else {
            continue;
        };
        if let Some(title) = first_user_message_text(&content) {
            return Some(title);
        }
    }
    None
}

/// 逐行找第一条 `type=message` 且 `message.role=user` 的文本，只取首行。
fn first_user_message_text(transcript: &str) -> Option<String> {
    for line in transcript.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let Some(text) = user_message_text(&value) else {
            continue;
        };
        let title = clean_title(text.lines().next().unwrap_or_default());
        if !title.is_empty() {
            return Some(title);
        }
    }
    None
}

fn user_message_text(value: &Value) -> Option<&str> {
    if value.get("type").and_then(Value::as_str) != Some("message") {
        return None;
    }
    let message = value.get("message")?;
    if message.get("role").and_then(Value::as_str) != Some("user") {
        return None;
    }
    for part in message.get("content")?.as_array()? {
        if part.get("type").and_then(Value::as_str) != Some("text") {
            continue;
        }
        let Some(text) = part.get("text").and_then(Value::as_str) else {
            continue;
        };
        if !text.trim().is_empty() {
            return Some(text);
        }
    }
    None
}

fn read_head(path: &Path, max_bytes: u64) -> Option<String> {
    let file = File::open(path).ok()?;
    let mut buffer = Vec::new();
    file.take(max_bytes).read_to_end(&mut buffer).ok()?;
    Some(String::from_utf8_lossy(&buffer).into_owned())
}

/// 与 Go 版 `cleanTitle` 一致：折叠空白并限长。
fn clean_title(value: &str) -> String {
    let collapsed = value.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.chars().take(TITLE_MAX_CHARS).collect()
}

fn default_projects_root() -> Option<PathBuf> {
    if let Some(configured) = env::var_os(PROJECTS_DIR_ENV).filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(configured));
    }
    let home = env::var_os(USER_PROFILE_ENV)
        .or_else(|| env::var_os(HOME_ENV))
        .filter(|value| !value.is_empty())?;
    let mut path = PathBuf::from(home);
    for part in PROJECTS_SUBDIR {
        path.push(part);
    }
    Some(path)
}
