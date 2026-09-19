use tauri::{AppHandle, Runtime, plugin::TauriPlugin};
use tauri_plugin_autostart::ManagerExt;

use super::LifecycleError;

/// 测试和设置页使用的自启动动作；接口只存在当前用户动作，不暴露 HKLM/系统级入口。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AutostartAction {
    EnableCurrentUser,
    DisableCurrentUser,
}

pub trait CurrentUserAutostart: Send + Sync {
    fn set_current_user_enabled(&self, enabled: bool) -> Result<(), LifecycleError>;
}

pub fn set_autostart(
    adapter: &dyn CurrentUserAutostart,
    enabled: bool,
) -> Result<(), LifecycleError> {
    adapter.set_current_user_enabled(enabled).map(|_| ())
}

/// Tauri 插件在 Windows 上写入当前用户启动项，不在应用内直接操作注册表。
pub fn plugin<R: Runtime>() -> TauriPlugin<R> {
    tauri_plugin_autostart::Builder::new()
        .app_name("AgentNotify")
        .arg("--autostart")
        .build()
}

pub struct TauriCurrentUserAutostart<R: Runtime> {
    app: AppHandle<R>,
}

impl<R: Runtime> TauriCurrentUserAutostart<R> {
    pub fn new(app: AppHandle<R>) -> Self {
        Self { app }
    }
}

impl<R: Runtime> CurrentUserAutostart for TauriCurrentUserAutostart<R> {
    fn set_current_user_enabled(&self, enabled: bool) -> Result<(), LifecycleError> {
        let manager = self.app.autolaunch();
        let result = if enabled {
            manager.enable()
        } else {
            manager.disable()
        };
        result.map_err(|error| {
            LifecycleError::new(
                "autostart_update_failed",
                format!("更新当前用户登录自启动失败：{error}"),
            )
        })
    }
}
