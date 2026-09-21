use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    path::Path,
};

use serde_json::Value;

/// 只读取 transcript 尾部，避免超大日志把停止事件拖住（与 Go 版一致）。
pub const TRANSCRIPT_TAIL_BYTES: u64 = 512 * 1024;
/// 摘要上限；超出后截断并追加省略号。
pub const SUMMARY_MAX_CHARS: usize = 12000;
/// 与 Go 版 Scanner 一致的单行上限：超过即停止扫描，不再解析后续行。
const MAX_LINE_BYTES: usize = 1024 * 1024;

/// 正文候选字段及权重，顺序与 Go 版保持字段优先级一致。
const TEXT_FIELDS: [(&str, u32); 7] = [
    ("last_assistant_message", 500),
    ("assistant_message", 500),
    ("response", 80),
    ("output", 70),
    ("content", 60),
    ("message", 50),
    ("text", 40),
];
/// 命中的字段处在 assistant 上下文时加分，保证助手正文优先于用户内容。
const ASSISTANT_CONTEXT_BONUS: u32 = 200;

/// 从 transcript 尾部提取摘要；文件缺失或不可解析时返回空串，绝不阻塞 Stop。
pub fn read_transcript_summary(path: Option<&Path>) -> String {
    let Some(path) = path else {
        return String::new();
    };
    match read_transcript_tail(path, TRANSCRIPT_TAIL_BYTES) {
        Ok(data) => extract_transcript_summary(&data),
        Err(_) => String::new(),
    }
}

/// 在给定字节上提取摘要；Antigravity transcript 是私有格式，无法识别时返回空串。
pub fn extract_transcript_summary(data: &[u8]) -> String {
    let mut best = TranscriptCandidate::default();
    for_each_json_line(data, MAX_LINE_BYTES, |line| {
        let Ok(node) = serde_json::from_slice::<Value>(line) else {
            return;
        };
        let candidate = candidate_from_node(&node, "");
        if !candidate.text.is_empty() && candidate.score >= best.score {
            best = candidate;
        }
    });
    truncate_summary(&best.text, SUMMARY_MAX_CHARS)
}

/// 读取文件头部有界字节，供标题解析使用。
pub(crate) fn read_transcript_head(path: &Path, limit: u64) -> io::Result<Vec<u8>> {
    let mut file = File::open(path)?;
    read_bounded(&mut file, limit)
}

/// 读取文件尾部的有界字节；文件小于上限时读取全部。
fn read_transcript_tail(path: &Path, limit: u64) -> io::Result<Vec<u8>> {
    let mut file = File::open(path)?;
    let length = file.metadata()?.len();
    file.seek(SeekFrom::Start(length.saturating_sub(limit)))?;
    read_bounded(&mut file, limit)
}

fn read_bounded(file: &mut File, limit: u64) -> io::Result<Vec<u8>> {
    let mut buffer = Vec::new();
    file.take(limit).read_to_end(&mut buffer)?;
    Ok(buffer)
}

/// 逐行扫描并按 Go 版 `bufio.Scanner` 语义处理：单行超限时停止（不是跳过）。
pub(crate) fn for_each_json_line(data: &[u8], max_line_bytes: usize, mut visit: impl FnMut(&[u8])) {
    for line in data.split(|byte| *byte == b'\n') {
        if line.len() > max_line_bytes {
            return;
        }
        let line = trim_ascii(line);
        if line.is_empty() {
            continue;
        }
        visit(line);
    }
}

fn trim_ascii(mut value: &[u8]) -> &[u8] {
    while value.first().is_some_and(u8::is_ascii_whitespace) {
        value = &value[1..];
    }
    while value.last().is_some_and(u8::is_ascii_whitespace) {
        value = &value[..value.len() - 1];
    }
    value
}

/// 摘要截断：按字符计数，超限时截断并追加省略号。
pub(crate) fn truncate_summary(text: &str, limit: usize) -> String {
    let text = text.trim();
    if limit == 0 {
        return text.to_owned();
    }
    if text.chars().count() <= limit {
        return text.to_owned();
    }
    let truncated: String = text.chars().take(limit).collect();
    format!("{}…", truncated.trim())
}

/// 折叠空白，避免把换行带进标题与摘要首行。
pub(crate) fn collapse_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[derive(Clone, Debug, Default)]
struct TranscriptCandidate {
    text: String,
    score: u32,
}

/// 递归打分：顶层数组取最高分候选，映射先看自身文本字段，再深入非文本字段。
fn candidate_from_node(node: &Value, inherited_role: &str) -> TranscriptCandidate {
    match node {
        Value::Array(items) => {
            let mut best = TranscriptCandidate::default();
            for item in items {
                let candidate = candidate_from_node(item, inherited_role);
                if !candidate.text.is_empty() && candidate.score >= best.score {
                    best = candidate;
                }
            }
            best
        }
        Value::Object(object) => {
            let role = transcript_role(&first_string_field(
                object,
                &["role", "speaker", "author", "source"],
            ));
            let kind = transcript_role(&first_string_field(object, &["type", "kind", "event"]));
            let context_role = if role.is_empty() {
                inherited_role
            } else {
                &role
            };
            if transcript_role_excluded(context_role) || transcript_role_excluded(&kind) {
                return TranscriptCandidate::default();
            }
            let assistant_context =
                transcript_role_assistant(context_role) || transcript_role_assistant(&kind);

            let mut best = TranscriptCandidate::default();
            for (field, field_score) in TEXT_FIELDS {
                let Some(raw) = object.get(field) else {
                    continue;
                };
                let text = flatten_transcript_text(raw);
                if text.is_empty() {
                    continue;
                }
                let score = field_score
                    + if assistant_context {
                        ASSISTANT_CONTEXT_BONUS
                    } else {
                        0
                    };
                if score >= best.score {
                    best = TranscriptCandidate { text, score };
                }
            }

            for (key, raw) in object {
                if is_transcript_text_key(key) {
                    continue;
                }
                let candidate = candidate_from_node(raw, context_role);
                if !candidate.text.is_empty() && candidate.score > best.score {
                    best = candidate;
                }
            }
            best
        }
        _ => TranscriptCandidate::default(),
    }
}

/// 只拼接明确的文本容器；`thought`/`tool_calls` 等推理与工具内容一律跳过。
fn flatten_transcript_text(value: &Value) -> String {
    let mut parts: Vec<&str> = Vec::new();
    collect_transcript_text(value, &mut parts);
    parts.join("\n").trim().to_owned()
}

fn collect_transcript_text<'a>(value: &'a Value, parts: &mut Vec<&'a str>) {
    match value {
        Value::String(text) => {
            let text = text.trim();
            if !text.is_empty() {
                parts.push(text);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_transcript_text(item, parts);
            }
        }
        Value::Object(object) => {
            for (key, item) in object {
                let key = key.trim().to_lowercase();
                match key.as_str() {
                    "text" | "content" | "parts" | "value" | "message" | "response" | "output" => {
                        collect_transcript_text(item, parts);
                    }
                    // 与 Go 版相同：未列出的键不递归，避免把推理或工具参数当正文。
                    _ => {}
                }
            }
        }
        _ => {}
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

fn transcript_role(value: &str) -> String {
    value
        .trim()
        .to_lowercase()
        .chars()
        .filter(|character| !matches!(character, '-' | '_' | ' '))
        .collect()
}

fn transcript_role_assistant(role: &str) -> bool {
    role.contains("assistant")
        || role == "model"
        || role.contains("plannerresponse")
        || role.contains("agentresponse")
}

fn transcript_role_excluded(role: &str) -> bool {
    if role.is_empty() {
        return false;
    }
    role.contains("user")
        || role.contains("human")
        || role.contains("tool")
        || role.contains("function")
        || role.contains("system")
}

fn is_transcript_text_key(key: &str) -> bool {
    let key = key.trim().to_lowercase();
    TEXT_FIELDS.iter().any(|(field, _)| *field == key)
}
