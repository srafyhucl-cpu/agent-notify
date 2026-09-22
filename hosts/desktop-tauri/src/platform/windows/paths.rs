use std::{env, path::PathBuf};

use super::AppPaths;

const CONFIG_DIR_ENV: &str = "AGENT_NOTIFY_CONFIG_DIR";
const DATA_DIR_ENV: &str = "AGENT_NOTIFY_DATA_DIR";
const LOG_DIR_ENV: &str = "AGENT_NOTIFY_LOG_DIR";
const SPOOL_DIR_ENV: &str = "AGENT_NOTIFY_SPOOL_DIR";
const TEMP_DIR_ENV: &str = "AGENT_NOTIFY_TEMP_DIR";
const USER_PROFILE_ENV: &str = "USERPROFILE";
const LOCAL_APP_DATA_ENV: &str = "LOCALAPPDATA";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppPathsError {
    code: &'static str,
    message: String,
}

impl AppPathsError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub const fn code(&self) -> &'static str {
        self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl std::fmt::Display for AppPathsError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.message.fmt(formatter)
    }
}

impl std::error::Error for AppPathsError {}

impl AppPaths {
    pub fn from_environment() -> Result<Self, AppPathsError> {
        Ok(Self {
            config_dir: environment_override_or(CONFIG_DIR_ENV, || {
                Ok(required_environment_path(USER_PROFILE_ENV)?
                    .join(".config")
                    .join("agent-notify"))
            })?,
            data_dir: environment_override_or(DATA_DIR_ENV, || {
                Ok(local_app_data()?.join("AgentNotify").join("data"))
            })?,
            log_dir: environment_override_or(LOG_DIR_ENV, || {
                Ok(local_app_data()?.join("AgentNotify").join("logs"))
            })?,
            spool_dir: environment_override_or(SPOOL_DIR_ENV, || {
                Ok(local_app_data()?.join("AgentNotify").join("spool"))
            })?,
            // 更新包下载与解压有自己的临时目录，避免与用户 `%TEMP%` 下的其它内容混在一起；
            // 隔离环境可用 AGENT_NOTIFY_TEMP_DIR 覆盖（与 Go 版同名）。
            temp_dir: environment_override_or(TEMP_DIR_ENV, || {
                Ok(local_app_data()?.join("AgentNotify").join("temp"))
            })?,
        })
    }

    pub fn ensure(&self) -> Result<(), AppPathsError> {
        for directory in [
            &self.config_dir,
            &self.data_dir,
            &self.log_dir,
            &self.spool_dir,
            &self.temp_dir,
        ] {
            std::fs::create_dir_all(directory).map_err(|error| {
                AppPathsError::new(
                    "app_paths_create_failed",
                    format!("无法创建应用目录 {}：{error}", directory.display()),
                )
            })?;
        }
        Ok(())
    }
}

fn required_environment_path(name: &str) -> Result<PathBuf, AppPathsError> {
    let value = env::var_os(name).ok_or_else(|| {
        AppPathsError::new(
            "app_paths_environment_missing",
            format!("缺少必要的 Windows 环境变量 {name}"),
        )
    })?;
    absolute_path(name, PathBuf::from(value))
}

fn local_app_data() -> Result<PathBuf, AppPathsError> {
    required_environment_path(LOCAL_APP_DATA_ENV)
}

fn environment_override_or(
    name: &str,
    default: impl FnOnce() -> Result<PathBuf, AppPathsError>,
) -> Result<PathBuf, AppPathsError> {
    match env::var_os(name) {
        Some(value) => absolute_path(name, PathBuf::from(value)),
        None => default(),
    }
}

fn absolute_path(name: &str, path: PathBuf) -> Result<PathBuf, AppPathsError> {
    if !path.is_absolute() {
        return Err(AppPathsError::new(
            "app_paths_relative",
            format!("环境变量 {name} 必须配置绝对路径，不能使用当前工作目录兜底"),
        ));
    }
    Ok(path)
}
