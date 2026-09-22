use std::{
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

/// 回复窗口秒数上限，与 Go 版 `commandCodeReplyWindowSec` 的 1–600 一致。
pub const MAX_REPLY_WINDOW_SEC: u64 = 600;
/// 应用写给 mod 的窗口文件名；放在应用自己的回复收件箱根目录里。
pub const WINDOW_FILE_NAME: &str = "window.json";
/// 窗口秒数的环境覆盖变量，与 Go 版 `AGENT_NOTIFY_COMMANDCODE_WINDOW_SEC` 同名。
const WINDOW_ENV: &str = "AGENT_NOTIFY_COMMANDCODE_WINDOW_SEC";
const CONFIG_FILE_ENV: &str = "AGENT_NOTIFY_CONFIG_FILE";
const CONFIG_DIR_ENV: &str = "AGENT_NOTIFY_CONFIG_DIR";
const USER_PROFILE_ENV: &str = "USERPROFILE";
const HOME_ENV: &str = "HOME";
const CONFIG_SUBDIR: [&str; 2] = [".config", "agent-notify"];
const CONFIG_FILE_NAME: &str = "config.json";
const WINDOW_KEY: &str = "commandCodeReplyWindowSec";

/// 解析回复窗口秒数：环境变量 > AgentNotify 配置文件 > 0（默认关闭，保守）。
///
/// 与 Go 版一致：环境变量只有大于 0 才覆盖配置（0 或非法值回退到配置文件），
/// 超过上限收敛到 600 秒。
pub fn resolve_reply_window_sec() -> u64 {
    if let Some(value) = env_number(WINDOW_ENV).filter(|value| *value > 0) {
        return clamp_reply_window_sec(value);
    }
    config_number().map_or(0, clamp_reply_window_sec)
}

/// 0 表示关闭；1–600 原样；超过上限收敛到上限。
pub const fn clamp_reply_window_sec(value: u64) -> u64 {
    if value == 0 {
        0
    } else if value > MAX_REPLY_WINDOW_SEC {
        MAX_REPLY_WINDOW_SEC
    } else {
        value
    }
}

/// 把生效的窗口秒数写入 mod 能读到的 `window.json`（应用自己的回复收件箱）。
///
/// 界面里配置的值原本只落在 SQLite，mod 读不到；此文件是应用自己的数据通道。
/// 幂等：同目录临时文件 + 改名原子替换，重复调用只会覆盖同一个文件。
pub fn write_reply_window(root: &Path, sec: u64) -> io::Result<()> {
    fs::create_dir_all(root)?;
    let destination = root.join(WINDOW_FILE_NAME);
    let temporary = root.join(format!(".{WINDOW_FILE_NAME}.{}.tmp", uuid::Uuid::new_v4()));
    let bytes = serde_json::to_vec(&serde_json::json!({
        WINDOW_KEY: clamp_reply_window_sec(sec),
    }))
    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;

    let result = write_atomic(&temporary, &destination, &bytes);
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn write_atomic(temporary: &Path, destination: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = fs::File::create(temporary)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(temporary, destination)
}

fn env_number(name: &str) -> Option<u64> {
    env::var(name).ok()?.trim().parse::<u64>().ok()
}

fn config_number() -> Option<u64> {
    let content = std::fs::read_to_string(config_path()?).ok()?;
    let root: serde_json::Value = serde_json::from_str(&content).ok()?;
    root.get(WINDOW_KEY)?.as_u64()
}

fn config_path() -> Option<PathBuf> {
    if let Some(configured) = non_empty_env(CONFIG_FILE_ENV) {
        return Some(PathBuf::from(configured));
    }
    let directory = match non_empty_env(CONFIG_DIR_ENV) {
        Some(configured) => PathBuf::from(configured),
        None => {
            let home = non_empty_env(USER_PROFILE_ENV).or_else(|| non_empty_env(HOME_ENV))?;
            let mut path = PathBuf::from(home);
            for part in CONFIG_SUBDIR {
                path.push(part);
            }
            path
        }
    };
    Some(directory.join(CONFIG_FILE_NAME))
}

fn non_empty_env(name: &str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.trim().is_empty())
}
