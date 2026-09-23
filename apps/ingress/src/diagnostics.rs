//! 只读自检报告（`--doctor` / `--ping`）：只观察本机状态，不提交事件、不写任何文件。
//!
//! 判定口径：命名管道已被桌面端监听，且 spool 无积压、无错误，才算正常。

use std::path::Path;

use serde::Serialize;

use crate::protocol::PROTOCOL_VERSION;
use crate::spool::QUARANTINE_DIR;

/// ingress 自身版本（来自 Cargo workspace 版本，由 tools/sync-version.ps1 与产品版本对齐）。
pub const SELF_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Debug, Serialize)]
pub struct PipeStatus {
    /// 当前用户命名管道全名；解析失败时为 None 并附 error。
    pub name: Option<String>,
    /// 桌面端是否已创建该管道（只读枚举系统管道表，不连接、不写入）。
    pub listening: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct SpoolStatus {
    pub dir: Option<String>,
    pub exists: bool,
    /// 待补投事件数（核心离线时积压）。
    pub pending: usize,
    /// 隔离事件数（无法解析或超限被隔离）。
    pub quarantined: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct DoctorReport {
    /// 正常 = 管道在监听、spool 目录存在且无积压无错误。
    pub ok: bool,
    pub version: &'static str,
    pub protocol_version: u8,
    pub pipe: PipeStatus,
    pub spool: SpoolStatus,
}

impl SpoolStatus {
    /// 只读检查 spool 目录：不创建目录、不写文件。读失败时原样暴露，不猜数字。
    pub fn inspect(dir: &Path) -> Self {
        let dir_text = Some(dir.to_string_lossy().into_owned());
        if !dir.is_dir() {
            return Self {
                dir: dir_text,
                exists: false,
                pending: 0,
                quarantined: 0,
                error: None,
            };
        }
        let pending = match count_events(dir) {
            Ok(count) => count,
            Err(error) => {
                return Self {
                    dir: dir_text,
                    exists: true,
                    pending: 0,
                    quarantined: 0,
                    error: Some(error.to_string()),
                };
            }
        };
        // 隔离目录还没建过就是 0 条隔离；读不出来时整体报错而不是默认成 0。
        match count_events(&dir.join(QUARANTINE_DIR)) {
            Ok(quarantined) => Self {
                dir: dir_text,
                exists: true,
                pending,
                quarantined,
                error: None,
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Self {
                dir: dir_text,
                exists: true,
                pending,
                quarantined: 0,
                error: None,
            },
            Err(error) => Self {
                dir: dir_text,
                exists: true,
                pending,
                quarantined: 0,
                error: Some(error.to_string()),
            },
        }
    }
}

/// 只数事件文件（`.json`，与 spool 队列文件命名一致）。
fn count_events(dir: &Path) -> std::io::Result<usize> {
    let mut count = 0;
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if entry.path().extension().is_some_and(|ext| ext == "json") {
            count += 1;
        }
    }
    Ok(count)
}

impl DoctorReport {
    pub fn assemble(pipe: PipeStatus, spool: SpoolStatus) -> Self {
        let ok = pipe.error.is_none()
            && pipe.name.is_some()
            && pipe.listening
            && spool.error.is_none()
            && spool.exists
            && spool.pending == 0;
        Self {
            ok,
            version: SELF_VERSION,
            protocol_version: PROTOCOL_VERSION,
            pipe,
            spool,
        }
    }

    /// 生产入口：读取本机命名管道与 spool 状态。
    pub fn collect() -> Self {
        let pipe = current_pipe_status();
        let spool = match crate::spool::default_spool_dir() {
            Some(dir) => SpoolStatus::inspect(&dir),
            None => SpoolStatus {
                dir: None,
                exists: false,
                pending: 0,
                quarantined: 0,
                error: Some("无法解析 spool 目录（LOCALAPPDATA 未设置）".to_owned()),
            },
        };
        Self::assemble(pipe, spool)
    }

    /// 一行可读结论（`--ping` 用）。
    pub fn summary(&self) -> String {
        if self.ok {
            return format!(
                "AgentNotify ingress 正常（v{}，事件协议 v{}）",
                self.version, self.protocol_version
            );
        }
        let mut reasons = Vec::new();
        if self.pipe.error.is_some() {
            reasons.push("命名管道无法探测".to_owned());
        } else if self.pipe.name.is_none() {
            reasons.push("无法解析当前用户命名管道".to_owned());
        } else if !self.pipe.listening {
            reasons.push("桌面端未在监听（AgentNotify 没在运行？）".to_owned());
        }
        match &self.spool.error {
            Some(error) => reasons.push(format!("spool 不可读：{error}")),
            None if !self.spool.exists => reasons.push("spool 目录不存在".to_owned()),
            None => {}
        }
        if self.spool.pending > 0 {
            reasons.push(format!("spool 积压 {} 条待补投事件", self.spool.pending));
        }
        format!("AgentNotify ingress 异常：{}", reasons.join("；"))
    }
}

fn current_pipe_status() -> PipeStatus {
    #[cfg(windows)]
    {
        let name = match crate::windows_pipe::pipe_name() {
            Ok(name) => name,
            Err(error) => {
                return PipeStatus {
                    name: None,
                    listening: false,
                    error: Some(format!("{}：{}", error.code(), error.message())),
                };
            }
        };
        match crate::windows_pipe::named_pipe_exists(&name) {
            Ok(listening) => PipeStatus {
                name: Some(name),
                listening,
                error: None,
            },
            Err(error) => PipeStatus {
                name: Some(name),
                listening: false,
                error: Some(error.to_string()),
            },
        }
    }
    #[cfg(not(windows))]
    {
        PipeStatus {
            name: None,
            listening: false,
            error: Some("仅 Windows 提供命名管道探测".to_owned()),
        }
    }
}
