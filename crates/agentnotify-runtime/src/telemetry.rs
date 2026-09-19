use std::{
    fmt::Display,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use tracing_subscriber::fmt::MakeWriter;

const MAX_PENDING_LOG_BYTES: usize = 64 * 1024;
const SENSITIVE_KEYS: &[&str] = &[
    "token",
    "secret",
    "authorization",
    "cookie",
    "context_token",
    "body",
    "message",
    "prompt",
    "text",
];

#[derive(Clone, Debug)]
pub struct TelemetryConfig {
    pub log_path: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TelemetryError {
    code: &'static str,
    message: String,
}

impl TelemetryError {
    pub const fn code(&self) -> &'static str {
        self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl Display for TelemetryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for TelemetryError {}

pub struct TelemetryGuard;

/// 统一替换结构化日志中的敏感字段值。
pub fn redact_sensitive(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut index = 0;
    while index < input.len() {
        let Some(key_length) = sensitive_key_at(input, index) else {
            let character = input[index..].chars().next().expect("索引必须位于字符边界");
            output.push(character);
            index += character.len_utf8();
            continue;
        };

        let mut value_marker = index + key_length;
        value_marker = skip_ascii_whitespace(input, value_marker);
        if input[value_marker..].starts_with('"') {
            value_marker += 1;
            value_marker = skip_ascii_whitespace(input, value_marker);
        }
        if !matches!(input[value_marker..].chars().next(), Some(':' | '=')) {
            output.push_str(&input[index..index + key_length]);
            index += key_length;
            continue;
        }

        value_marker += 1;
        value_marker = skip_ascii_whitespace(input, value_marker);
        output.push_str(&input[index..value_marker]);
        output.push_str("\"[REDACTED]\"");
        index = sensitive_value_end(input, value_marker);
    }
    output
}

pub struct RedactingWriter<W: Write> {
    inner: W,
    pending: Vec<u8>,
}

impl<W: Write> RedactingWriter<W> {
    pub fn new(inner: W) -> Self {
        Self {
            inner,
            pending: Vec::new(),
        }
    }

    fn flush_complete_lines(&mut self) -> io::Result<()> {
        while let Some(newline) = self.pending.iter().position(|byte| *byte == b'\n') {
            let line = self.pending.drain(..=newline).collect::<Vec<_>>();
            self.write_redacted(&line)?;
        }
        if self.pending.len() > MAX_PENDING_LOG_BYTES {
            let line = std::mem::take(&mut self.pending);
            self.write_redacted(&line)?;
        }
        Ok(())
    }

    fn write_redacted(&mut self, bytes: &[u8]) -> io::Result<()> {
        let text = String::from_utf8_lossy(bytes);
        let redacted = redact_sensitive(&text);
        self.inner.write_all(redacted.as_bytes())
    }

    fn flush_pending(&mut self) -> io::Result<()> {
        if self.pending.is_empty() {
            return Ok(());
        }
        let line = std::mem::take(&mut self.pending);
        self.write_redacted(&line)
    }
}

impl<W: Write> Write for RedactingWriter<W> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.pending.extend_from_slice(buffer);
        self.flush_complete_lines()?;
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.flush_pending()?;
        self.inner.flush()
    }
}

impl<W: Write> Drop for RedactingWriter<W> {
    fn drop(&mut self) {
        let _ = self.flush();
    }
}

#[derive(Clone)]
struct SharedFile {
    file: Arc<Mutex<File>>,
}

impl Write for SharedFile {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.file
            .lock()
            .map_err(|_| io::Error::other("日志文件锁不可用"))?
            .write(buffer)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file
            .lock()
            .map_err(|_| io::Error::other("日志文件锁不可用"))?
            .flush()
    }
}

impl<'a> MakeWriter<'a> for SharedFile {
    type Writer = RedactingWriter<Self>;

    fn make_writer(&'a self) -> Self::Writer {
        RedactingWriter::new(self.clone())
    }
}

/// 初始化文件日志；现有全局 subscriber 已存在时返回错误。
pub fn init_telemetry(config: TelemetryConfig) -> Result<TelemetryGuard, TelemetryError> {
    create_log_parent(&config.log_path)?;
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&config.log_path)
        .map_err(|_| TelemetryError {
            code: "telemetry_open_failed",
            message: "打开应用日志文件失败".into(),
        })?;
    let writer = SharedFile {
        file: Arc::new(Mutex::new(file)),
    };
    tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(writer)
        .try_init()
        .map_err(|_| TelemetryError {
            code: "telemetry_init_failed",
            message: "初始化应用日志失败".into(),
        })?;
    Ok(TelemetryGuard)
}

fn create_log_parent(path: &Path) -> Result<(), TelemetryError> {
    let Some(parent) = path.parent() else {
        return Err(TelemetryError {
            code: "telemetry_path_invalid",
            message: "日志路径无效".into(),
        });
    };
    fs::create_dir_all(parent).map_err(|_| TelemetryError {
        code: "telemetry_directory_failed",
        message: "创建应用日志目录失败".into(),
    })
}

fn sensitive_key_at(input: &str, index: usize) -> Option<usize> {
    if index > 0 {
        let previous = input[..index].chars().next_back()?;
        if previous.is_ascii_alphanumeric() || previous == '_' {
            return None;
        }
    }
    for key in SENSITIVE_KEYS {
        let end = index.checked_add(key.len())?;
        let Some(candidate) = input.get(index..end) else {
            continue;
        };
        if !candidate.eq_ignore_ascii_case(key) {
            continue;
        }
        let next = input[end..].chars().next();
        if next.is_none_or(|character| {
            character.is_ascii_whitespace() || matches!(character, '"' | ':' | '=')
        }) {
            return Some(key.len());
        }
    }
    None
}

fn skip_ascii_whitespace(input: &str, mut index: usize) -> usize {
    while let Some(character) = input[index..].chars().next() {
        if !character.is_ascii_whitespace() {
            break;
        }
        index += character.len_utf8();
    }
    index
}

fn sensitive_value_end(input: &str, start: usize) -> usize {
    let Some(first) = input[start..].chars().next() else {
        return start;
    };
    match first {
        '"' => quoted_value_end(input, start),
        '{' | '[' => structured_value_end(input, start),
        _ => input[start..]
            .char_indices()
            .find_map(|(offset, character)| {
                (character.is_ascii_whitespace() || matches!(character, ',' | '}' | ']'))
                    .then_some(start + offset)
            })
            .unwrap_or(input.len()),
    }
}

fn quoted_value_end(input: &str, start: usize) -> usize {
    let mut escaped = false;
    for (offset, character) in input[start + 1..].char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' {
            escaped = true;
            continue;
        }
        if character == '"' {
            return start + 1 + offset + character.len_utf8();
        }
    }
    input.len()
}

fn structured_value_end(input: &str, start: usize) -> usize {
    let opening = input[start..].chars().next().expect("结构化值必须存在");
    let closing = if opening == '{' { '}' } else { ']' };
    let mut depth = 0_usize;
    let mut in_string = false;
    let mut escaped = false;
    for (offset, character) in input[start..].char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
            }
            continue;
        }
        match character {
            '"' => in_string = true,
            value if value == opening => depth = depth.saturating_add(1),
            value if value == closing => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return start + offset + character.len_utf8();
                }
            }
            _ => {}
        }
    }
    input.len()
}
