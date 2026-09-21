use std::{
    env,
    ffi::OsString,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

use serde_json::Value;

use crate::transcript::{
    collapse_whitespace, for_each_json_line, read_transcript_head, truncate_summary,
};

/// Go 版渲染层对空标题使用的默认值；Antigravity 的标题链最后一级就是它。
pub const DEFAULT_TITLE: &str = "任务已完成";

const ANNOTATIONS_DIR_ENV: &str = "AGENT_NOTIFY_ANTIGRAVITY_ANNOTATIONS_DIR";
const USER_PROFILE_ENV: &str = "USERPROFILE";
const HOME_ENV: &str = "HOME";
const GEMINI_DIR_NAME: &str = ".gemini";
const ANTIGRAVITY_DIR_NAME: &str = "antigravity";
const ANNOTATIONS_DIR_NAME: &str = "annotations";
const ANNOTATION_SUFFIX: &str = ".pbtxt";
/// 注释文件上限；超过即视为不可读，与 Go 版一样退回下一级标题来源。
const ANNOTATION_MAX_BYTES: u64 = 1 << 20;
/// 标题回退只读 transcript 头部，避免为了标题扫描整个日志。
const TRANSCRIPT_TITLE_MAX_BYTES: u64 = 256 * 1024;
/// 标题最大字符数。
const TITLE_MAX_CHARS: usize = 80;
/// 与 Go 版 `bufio.Scanner` 一致的单行上限。
const TRANSCRIPT_MAX_LINE_BYTES: usize = 1024 * 1024;
const USER_REQUEST_OPEN: &str = "<USER_REQUEST>";
const USER_REQUEST_CLOSE: &str = "</USER_REQUEST>";
const ADDITIONAL_METADATA_OPEN: &str = "<ADDITIONAL_METADATA>";

/// 标题降级提示，文案与 Go 版一致，用户能在微信里看懂。
pub const TRANSCRIPT_TITLE_WARNING: &str =
    "未能读取 Antigravity 会话标题，已使用首条用户请求作为标题。";
pub const FALLBACK_TITLE_WARNING: &str = "未能读取 Antigravity 会话标题，已使用默认标题。";

/// 标题最终命中来源；顺序即降级顺序。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TitleSource {
    Annotations,
    Transcript,
    Default,
}

impl TitleSource {
    /// 稳定诊断标识，与 Go 版标题来源同名。
    pub const fn id(self) -> &'static str {
        match self {
            Self::Annotations => "annotations",
            Self::Transcript => "transcript",
            Self::Default => "fallback",
        }
    }
}

/// 标题解析结果；降级时通过 `degradation` 在正文里标记来源。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TitleResolution {
    pub name: String,
    pub source: TitleSource,
    pub degradation: Option<String>,
}

/// `AGENT_NOTIFY_ANTIGRAVITY_ANNOTATIONS_DIR` 优先，其次 `%USERPROFILE%\.gemini\antigravity\annotations`。
pub fn default_annotations_dir() -> Option<PathBuf> {
    if let Some(configured) = non_empty_env(ANNOTATIONS_DIR_ENV) {
        return Some(PathBuf::from(configured));
    }
    let home = non_empty_env(USER_PROFILE_ENV).or_else(|| non_empty_env(HOME_ENV))?;
    Some(
        PathBuf::from(home)
            .join(GEMINI_DIR_NAME)
            .join(ANTIGRAVITY_DIR_NAME)
            .join(ANNOTATIONS_DIR_NAME),
    )
}

/// 按 `annotations/<conversationId>.pbtxt → transcript 首条用户请求 → 默认标题` 解析标题。
pub fn resolve_title(
    annotations_dir: Option<&Path>,
    conversation_id: &str,
    transcript_path: Option<&str>,
) -> TitleResolution {
    if let Some(annotations_dir) = annotations_dir {
        if let Some(name) = read_annotation_title(annotations_dir, conversation_id) {
            return TitleResolution {
                name,
                source: TitleSource::Annotations,
                degradation: None,
            };
        }
    }

    if let Some(prompt) = transcript_path
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .and_then(|path| read_transcript_prompt_title(Path::new(path)))
    {
        return TitleResolution {
            name: prompt,
            source: TitleSource::Transcript,
            degradation: Some(TRANSCRIPT_TITLE_WARNING.to_owned()),
        };
    }

    TitleResolution {
        name: DEFAULT_TITLE.to_owned(),
        source: TitleSource::Default,
        degradation: Some(FALLBACK_TITLE_WARNING.to_owned()),
    }
}

/// 读取 `annotations/<conversationId>.pbtxt` 的 title 字段；任何失败都返回 None 由调用方降级。
fn read_annotation_title(annotations_dir: &Path, conversation_id: &str) -> Option<String> {
    if !is_safe_conversation_file_name(conversation_id) {
        return None;
    }
    let path = annotations_dir.join(format!("{conversation_id}{ANNOTATION_SUFFIX}"));
    let file = File::open(path).ok()?;
    let mut data = Vec::new();
    file.take(ANNOTATION_MAX_BYTES + 1)
        .read_to_end(&mut data)
        .ok()?;
    if data.len() as u64 > ANNOTATION_MAX_BYTES {
        return None;
    }
    parse_annotation_title(&data)
}

/// 会话 ID 只能作为单个文件名使用：出现路径分隔或 `.` / `..` 时拒绝读取。
fn is_safe_conversation_file_name(conversation_id: &str) -> bool {
    let conversation_id = conversation_id.trim();
    if conversation_id.is_empty() || conversation_id == "." || conversation_id.contains('\0') {
        return false;
    }
    Path::new(conversation_id)
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name == conversation_id)
}

/// 解析 protobuf 文本里的 `title:"..."`；格式不符时返回 None。
fn parse_annotation_title(data: &[u8]) -> Option<String> {
    let mut search_from = 0;
    while let Some(position) = find_bytes(data, b"title", search_from) {
        let boundary_ok =
            position == 0 || matches!(data[position - 1], b' ' | b'\t' | b'\n' | b'\x0C' | b'\r');
        if boundary_ok {
            let mut cursor = position + b"title".len();
            skip_regex_whitespace(data, &mut cursor);
            if data.get(cursor) == Some(&b':') {
                cursor += 1;
                skip_regex_whitespace(data, &mut cursor);
                if data.get(cursor) == Some(&b'"') {
                    if let Some(title) = parse_quoted_title(data, cursor) {
                        let cleaned = collapse_whitespace(&title);
                        if cleaned.is_empty() {
                            // 与 Go 版一致：命中的 title 为空即视为读取失败，继续降级。
                            return None;
                        }
                        return Some(truncate_summary(&cleaned, TITLE_MAX_CHARS));
                    }
                }
            }
        }
        search_from = position + 1;
    }
    None
}

fn find_bytes(data: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if from >= data.len() {
        return None;
    }
    data[from..]
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|offset| offset + from)
}

/// Go 版正则里的 `\s` 只包含这五个 ASCII 空白字符。
fn skip_regex_whitespace(data: &[u8], cursor: &mut usize) {
    while data
        .get(*cursor)
        .is_some_and(|byte| matches!(byte, b' ' | b'\t' | b'\n' | b'\x0C' | b'\r'))
    {
        *cursor += 1;
    }
}

/// 解析 `"..."` 并解码转义；未闭合或转义非法时返回 None。
fn parse_quoted_title(data: &[u8], quote_index: usize) -> Option<String> {
    let mut cursor = quote_index + 1;
    let mut raw = Vec::new();
    while let Some(byte) = data.get(cursor).copied() {
        match byte {
            b'"' => return decode_escaped_title(&raw),
            b'\\' => {
                raw.push(byte);
                cursor += 1;
                raw.push(data.get(cursor).copied()?);
            }
            _ => raw.push(byte),
        }
        cursor += 1;
    }
    None
}

/// 解码 Go `strconv.Unquote` 支持的转义；非法转义按读取失败处理（与 Go 一致）。
fn decode_escaped_title(raw: &[u8]) -> Option<String> {
    let mut bytes = Vec::with_capacity(raw.len());
    let mut cursor = 0;
    while cursor < raw.len() {
        let byte = raw[cursor];
        if byte != b'\\' {
            bytes.push(byte);
            cursor += 1;
            continue;
        }
        cursor += 1;
        let escape = *raw.get(cursor)?;
        cursor += 1;
        match escape {
            b'a' => bytes.push(0x07),
            b'b' => bytes.push(0x08),
            b'f' => bytes.push(0x0C),
            b'n' => bytes.push(b'\n'),
            b'r' => bytes.push(b'\r'),
            b't' => bytes.push(b'\t'),
            b'v' => bytes.push(0x0B),
            b'\\' => bytes.push(b'\\'),
            b'"' => bytes.push(b'"'),
            b'\'' => bytes.push(b'\''),
            b'x' => bytes.push(read_hex(raw, &mut cursor, 2)? as u8),
            b'u' => bytes.extend_from_slice(
                char::from_u32(read_hex(raw, &mut cursor, 4)?)?
                    .to_string()
                    .as_bytes(),
            ),
            b'U' => bytes.extend_from_slice(
                char::from_u32(read_hex(raw, &mut cursor, 8)?)?
                    .to_string()
                    .as_bytes(),
            ),
            b'0'..=b'7' => {
                let mut value = u32::from(escape - b'0');
                let mut digits = 1;
                while digits < 3 {
                    let Some(digit @ b'0'..=b'7') = raw.get(cursor).copied() else {
                        break;
                    };
                    value = value * 8 + u32::from(digit - b'0');
                    cursor += 1;
                    digits += 1;
                }
                if value > u32::from(u8::MAX) {
                    return None;
                }
                bytes.push(value as u8);
            }
            _ => return None,
        }
    }
    String::from_utf8(bytes).ok()
}

fn read_hex(raw: &[u8], cursor: &mut usize, digits: usize) -> Option<u32> {
    let mut value = 0u32;
    for _ in 0..digits {
        let digit = char::from(*raw.get(*cursor)?).to_digit(16)?;
        value = value * 16 + digit;
        *cursor += 1;
    }
    Some(value)
}

/// 从 transcript 头部提取首条 `USER_INPUT` 的标题；解析失败返回 None。
fn read_transcript_prompt_title(path: &Path) -> Option<String> {
    let data = read_transcript_head(path, TRANSCRIPT_TITLE_MAX_BYTES).ok()?;
    let mut found = None;
    for_each_json_line(&data, TRANSCRIPT_MAX_LINE_BYTES, |line| {
        if found.is_some() {
            return;
        }
        let Ok(node) = serde_json::from_slice::<Value>(line) else {
            return;
        };
        let Some(object) = node.as_object() else {
            return;
        };
        let kind = first_string_field(object, &["type", "kind", "event"]);
        if !kind.trim().eq_ignore_ascii_case("USER_INPUT") {
            return;
        }
        // 命中 USER_INPUT 但清洗后为空时继续尝试后续行，与 Go 版一致。
        found = clean_prompt_title(&first_string_field(object, &["content", "text", "message"]));
    });
    found
}

/// 去掉 Antigravity 的 `<USER_REQUEST>` 包装与附加元数据，再限长。
fn clean_prompt_title(content: &str) -> Option<String> {
    let mut content = content.trim();
    if let Some(index) = content.find(USER_REQUEST_OPEN) {
        content = &content[index + USER_REQUEST_OPEN.len()..];
    }
    if let Some(index) = content.find(USER_REQUEST_CLOSE) {
        content = &content[..index];
    }
    if let Some(index) = content.find(ADDITIONAL_METADATA_OPEN) {
        content = &content[..index];
    }
    let cleaned = collapse_whitespace(content);
    if cleaned.is_empty() {
        None
    } else {
        Some(truncate_summary(&cleaned, TITLE_MAX_CHARS))
    }
}

fn first_string_field(object: &serde_json::Map<String, Value>, keys: &[&str]) -> String {
    for key in keys {
        if let Some(Value::String(text)) = object.get(*key) {
            if !text.trim().is_empty() {
                return text.clone();
            }
        }
    }
    String::new()
}

fn non_empty_env(name: &str) -> Option<OsString> {
    env::var_os(name).filter(|value| !value.is_empty())
}
