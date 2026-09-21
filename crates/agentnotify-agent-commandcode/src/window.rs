use std::{env, path::PathBuf};

/// 回复窗口秒数上限，与 Go 版 `commandCodeReplyWindowSec` 的 1–600 一致。
pub const MAX_REPLY_WINDOW_SEC: u64 = 600;
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
