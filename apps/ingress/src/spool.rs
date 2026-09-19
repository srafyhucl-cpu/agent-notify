use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use agentnotify_agent_sdk::AgentEventEnvelope;
use uuid::Uuid;

use crate::protocol::{IngressError, IngressEvent, encode_envelope};

const DEFAULT_MAX_EVENTS: usize = 10_000;
const DEFAULT_MAX_BYTES: u64 = 64 * 1024 * 1024;
const DEFAULT_MAX_EVENT_BYTES: u64 = 256 * 1024;
const QUARANTINE_DIR: &str = "quarantine";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SpoolLimits {
    pub max_events: usize,
    pub max_bytes: u64,
    pub max_event_bytes: u64,
}

impl Default for SpoolLimits {
    fn default() -> Self {
        Self {
            max_events: DEFAULT_MAX_EVENTS,
            max_bytes: DEFAULT_MAX_BYTES,
            max_event_bytes: DEFAULT_MAX_EVENT_BYTES,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SpoolError {
    #[error("spool 路径无效")]
    InvalidPath,
    #[error("spool 容量限制无效")]
    InvalidLimits,
    #[error("spool 队列已达到容量上限")]
    CapacityExceeded,
    #[error("入口事件无法写入 spool")]
    WriteFailed(#[source] io::Error),
    #[error("入口事件无法从 spool 读取")]
    ReadFailed(#[source] io::Error),
    #[error("入口事件无法序列化")]
    EncodeFailed(#[source] serde_json::Error),
    #[error("找不到对应的 spool 文件")]
    NotFound,
}

impl SpoolError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::InvalidPath => "spool_path_invalid",
            Self::InvalidLimits => "spool_limits_invalid",
            Self::CapacityExceeded => "spool_capacity_exceeded",
            Self::WriteFailed(_) => "spool_write_failed",
            Self::ReadFailed(_) => "spool_read_failed",
            Self::EncodeFailed(_) => "spool_encode_failed",
            Self::NotFound => "spool_not_found",
        }
    }

    pub const fn message(&self) -> &'static str {
        match self {
            Self::InvalidPath => "spool 路径无效",
            Self::InvalidLimits => "spool 容量限制无效",
            Self::CapacityExceeded => "spool 队列已达到容量上限",
            Self::WriteFailed(_) => "入口事件无法写入 spool",
            Self::ReadFailed(_) => "入口事件无法从 spool 读取",
            Self::EncodeFailed(_) => "入口事件无法序列化",
            Self::NotFound => "找不到对应的 spool 文件",
        }
    }
}

#[derive(Debug)]
pub struct SpoolEntry {
    pub path: PathBuf,
    pub event: Result<AgentEventEnvelope, IngressError>,
}

pub struct Spool {
    root: PathBuf,
    limits: SpoolLimits,
    write_lock: Mutex<()>,
}

impl Spool {
    pub fn open(root: impl Into<PathBuf>, limits: SpoolLimits) -> Result<Self, SpoolError> {
        if limits.max_events == 0 || limits.max_bytes == 0 || limits.max_event_bytes == 0 {
            return Err(SpoolError::InvalidLimits);
        }
        let root = root.into();
        if root.as_os_str().is_empty() {
            return Err(SpoolError::InvalidPath);
        }
        fs::create_dir_all(&root).map_err(SpoolError::WriteFailed)?;
        Ok(Self {
            root,
            limits,
            write_lock: Mutex::new(()),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn write_event(&self, envelope: &AgentEventEnvelope) -> Result<PathBuf, SpoolError> {
        let _guard = self
            .write_lock
            .lock()
            .map_err(|_| SpoolError::WriteFailed(io::Error::other("spool 写入锁不可用")))?;
        let request_id = envelope.request_id.as_str();
        if let Some(path) = self.find_path(request_id)? {
            return Ok(path);
        }

        let bytes = encode_envelope(envelope).map_err(SpoolError::EncodeFailed)?;
        if bytes.len() as u64 > self.limits.max_event_bytes {
            return Err(SpoolError::CapacityExceeded);
        }
        let queued = self.queued_usage()?;
        if queued.count >= self.limits.max_events
            || queued.bytes.saturating_add(bytes.len() as u64) > self.limits.max_bytes
        {
            return Err(SpoolError::CapacityExceeded);
        }

        let timestamp = unix_millis();
        let final_path = self.root.join(format!("{timestamp:013}-{request_id}.json"));
        let temporary_path = self
            .root
            .join(format!(".{request_id}-{}.json.tmp", Uuid::new_v4()));
        let write_result = (|| -> Result<(), SpoolError> {
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temporary_path)
                .map_err(SpoolError::WriteFailed)?;
            file.write_all(&bytes).map_err(SpoolError::WriteFailed)?;
            file.sync_all().map_err(SpoolError::WriteFailed)?;
            match fs::rename(&temporary_path, &final_path) {
                Ok(()) => Ok(()),
                Err(_) if final_path.exists() => {
                    let _ = fs::remove_file(&temporary_path);
                    Ok(())
                }
                Err(error) => Err(SpoolError::WriteFailed(error)),
            }
        })();
        if write_result.is_err() {
            let _ = fs::remove_file(&temporary_path);
        }
        write_result?;
        Ok(final_path)
    }

    pub fn drain_batch(&self, limit: usize) -> Result<Vec<SpoolEntry>, SpoolError> {
        let limit = limit.max(1);
        let mut paths = self.json_paths()?;
        paths.sort();
        paths.truncate(limit);
        let mut entries = Vec::with_capacity(paths.len());
        for path in paths {
            let bytes = fs::read(&path).map_err(SpoolError::ReadFailed)?;
            entries.push(SpoolEntry {
                path,
                event: IngressEvent::parse(&bytes),
            });
        }
        Ok(entries)
    }

    pub fn path_for(&self, request_id: &str) -> Result<PathBuf, SpoolError> {
        self.find_path(request_id)?.ok_or(SpoolError::NotFound)
    }

    pub fn queued_count(&self) -> Result<usize, SpoolError> {
        Ok(self.json_paths()?.len())
    }

    pub fn ack(&self, entry: &SpoolEntry) -> Result<(), SpoolError> {
        match fs::remove_file(&entry.path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(SpoolError::WriteFailed(error)),
        }
    }

    pub fn quarantine(&self, entry: &SpoolEntry, reason_code: &str) -> Result<(), SpoolError> {
        let quarantine = self.root.join(QUARANTINE_DIR);
        fs::create_dir_all(&quarantine).map_err(SpoolError::WriteFailed)?;
        let file_name = entry
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or(SpoolError::InvalidPath)?;
        let mut target = quarantine.join(file_name);
        if target.exists() {
            target = quarantine.join(format!("{}-{file_name}", Uuid::new_v4()));
        }
        fs::rename(&entry.path, &target).map_err(SpoolError::WriteFailed)?;

        let reason_path = target.with_extension("error");
        let reason = serde_json::to_vec(&serde_json::json!({ "code": reason_code }))
            .map_err(SpoolError::EncodeFailed)?;
        fs::write(reason_path, reason).map_err(SpoolError::WriteFailed)
    }

    pub fn cleanup_expired(&self, max_age: Duration) -> Result<usize, SpoolError> {
        let threshold = SystemTime::now()
            .checked_sub(max_age)
            .unwrap_or(SystemTime::UNIX_EPOCH);
        let mut removed = 0;
        for path in self.json_paths()? {
            let metadata = fs::metadata(&path).map_err(SpoolError::ReadFailed)?;
            let modified = metadata.modified().map_err(SpoolError::ReadFailed)?;
            if modified < threshold {
                fs::remove_file(path).map_err(SpoolError::WriteFailed)?;
                removed += 1;
            }
        }
        Ok(removed)
    }

    fn find_path(&self, request_id: &str) -> Result<Option<PathBuf>, SpoolError> {
        let suffix = format!("-{request_id}.json");
        Ok(self
            .json_paths()?
            .into_iter()
            .find(|path| path.to_string_lossy().ends_with(&suffix)))
    }

    fn json_paths(&self) -> Result<Vec<PathBuf>, SpoolError> {
        let entries = fs::read_dir(&self.root).map_err(SpoolError::ReadFailed)?;
        let mut paths = Vec::new();
        for entry in entries {
            let entry = entry.map_err(SpoolError::ReadFailed)?;
            let path = entry.path();
            if path
                .extension()
                .is_some_and(|extension| extension == "json")
            {
                paths.push(path);
            }
        }
        Ok(paths)
    }

    fn queued_usage(&self) -> Result<SpoolUsage, SpoolError> {
        let mut usage = SpoolUsage::default();
        for path in self.json_paths()? {
            usage.count += 1;
            usage.bytes = usage
                .bytes
                .saturating_add(fs::metadata(path).map_err(SpoolError::ReadFailed)?.len());
        }
        Ok(usage)
    }
}

#[derive(Default)]
struct SpoolUsage {
    count: usize,
    bytes: u64,
}

pub fn default_spool_dir() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("AGENT_NOTIFY_SPOOL_DIR") {
        if !path.is_empty() {
            return Some(PathBuf::from(path));
        }
    }
    std::env::var_os("LOCALAPPDATA")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .map(|root| root.join("AgentNotify").join("spool"))
}

pub fn write_default_spool(envelope: &AgentEventEnvelope) -> Result<PathBuf, SpoolError> {
    let root = default_spool_dir().ok_or(SpoolError::InvalidPath)?;
    Spool::open(root, SpoolLimits::default())?.write_event(envelope)
}

fn unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
