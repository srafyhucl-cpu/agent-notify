use std::{
    path::{Path, PathBuf},
    ptr::{null, null_mut},
};

use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;
use windows_sys::Win32::{UI::Shell::ShellExecuteW, UI::WindowsAndMessaging::SW_SHOWNORMAL};

use super::{AppPaths, SystemUiError, TrayState};

pub const MAIN_WINDOW_LABEL: &str = "main";
pub const TRAY_STATE_EVENT: &str = "tray.state.changed";

pub struct WindowsSystemUi {
    app: Option<AppHandle>,
    paths: AppPaths,
}

impl WindowsSystemUi {
    pub fn new(app: AppHandle, paths: AppPaths) -> Self {
        Self {
            app: Some(app),
            paths,
        }
    }

    pub fn unavailable(paths: AppPaths) -> Self {
        Self { app: None, paths }
    }

    fn app(&self) -> Result<&AppHandle, SystemUiError> {
        self.app.as_ref().ok_or_else(|| {
            SystemUiError::new(
                "system_ui_unavailable",
                "桌面窗口尚未初始化，暂时无法执行系统界面操作",
            )
        })
    }
}

#[async_trait::async_trait]
impl super::SystemUi for WindowsSystemUi {
    async fn show_main_window(&self) -> Result<(), SystemUiError> {
        let window = self
            .app()?
            .get_webview_window(MAIN_WINDOW_LABEL)
            .ok_or_else(|| SystemUiError::new("system_ui_window_missing", "主窗口不存在"))?;
        window
            .show()
            .map_err(|error| map_tauri_error("显示主窗口失败", error))?;
        window
            .set_focus()
            .map_err(|error| map_tauri_error("聚焦主窗口失败", error))
    }

    async fn hide_main_window(&self) -> Result<(), SystemUiError> {
        self.app()?
            .get_webview_window(MAIN_WINDOW_LABEL)
            .ok_or_else(|| SystemUiError::new("system_ui_window_missing", "主窗口不存在"))?
            .hide()
            .map_err(|error| map_tauri_error("隐藏主窗口失败", error))
    }

    async fn set_tray_state(&self, state: TrayState) -> Result<(), SystemUiError> {
        self.app()?
            .emit(TRAY_STATE_EVENT, state)
            .map_err(|error| map_tauri_error("更新托盘状态失败", error))
    }

    async fn show_system_notification(&self, title: &str, body: &str) -> Result<(), SystemUiError> {
        if title.trim().is_empty() || body.trim().is_empty() {
            return Err(SystemUiError::new(
                "system_ui_notification_empty",
                "系统通知标题和正文不能为空",
            ));
        }
        self.app()?
            .notification()
            .builder()
            .title(title)
            .body(body)
            .show()
            .map_err(|error| map_tauri_error("显示系统通知失败", error))
    }

    async fn open_log_dir(&self) -> Result<(), SystemUiError> {
        let directory = validate_existing_directory(&self.paths, &self.paths.log_dir)?;
        open_directory(&directory)
    }
}

pub fn validate_existing_directory(
    paths: &AppPaths,
    candidate: &Path,
) -> Result<PathBuf, SystemUiError> {
    let canonical_candidate = std::fs::canonicalize(candidate).map_err(|error| {
        SystemUiError::new(
            "system_ui_path_invalid",
            format!("无法解析目标目录 {}：{error}", candidate.display()),
        )
    })?;
    for allowed in [
        &paths.config_dir,
        &paths.data_dir,
        &paths.log_dir,
        &paths.spool_dir,
    ] {
        let canonical_allowed = std::fs::canonicalize(allowed).map_err(|error| {
            SystemUiError::new(
                "system_ui_path_invalid",
                format!("无法解析应用目录 {}：{error}", allowed.display()),
            )
        })?;
        if canonical_candidate.starts_with(canonical_allowed) {
            return Ok(canonical_candidate);
        }
    }

    Err(SystemUiError::new(
        "system_ui_path_outside_app_paths",
        format!("目录 {} 不在 AgentNotify 应用目录内", candidate.display()),
    ))
}

fn open_directory(directory: &Path) -> Result<(), SystemUiError> {
    let operation = wide_string("open");
    let directory = wide_string(&directory.to_string_lossy());
    let result = unsafe {
        ShellExecuteW(
            null_mut(),
            operation.as_ptr(),
            directory.as_ptr(),
            null(),
            null(),
            SW_SHOWNORMAL,
        )
    };
    if result as isize <= 32 {
        return Err(SystemUiError::new(
            "system_ui_open_log_dir_failed",
            "无法打开 AgentNotify 日志目录",
        ));
    }
    Ok(())
}

fn map_tauri_error(prefix: &str, error: impl std::fmt::Display) -> SystemUiError {
    SystemUiError::new("system_ui_failed", format!("{prefix}：{error}"))
}

fn wide_string(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
