use tauri::{AppHandle, Manager, Runtime, WindowEvent};

use super::{LifecycleController, LifecycleError};

pub const MAIN_WINDOW_LABEL: &str = "main";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeState {
    Starting,
    Running,
    Paused,
    Stopping,
    Stopped,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowAction {
    Hide,
    AllowClose,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LifecycleAction {
    ShutdownRuntime,
    Exit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeReadyAction {
    KeepHidden,
    ShowMain,
}

pub fn window_action_for_close(_runtime_state: RuntimeState, tray_available: bool) -> WindowAction {
    if tray_available {
        WindowAction::Hide
    } else {
        WindowAction::AllowClose
    }
}

pub fn replacement_for_quit(_runtime_state: RuntimeState) -> LifecycleAction {
    LifecycleAction::ShutdownRuntime
}

pub fn runtime_ready_action(runtime_ready: bool) -> RuntimeReadyAction {
    if runtime_ready {
        RuntimeReadyAction::ShowMain
    } else {
        RuntimeReadyAction::KeepHidden
    }
}

pub fn determine_runtime_ready_action(start_hidden: bool, autostart: bool) -> RuntimeReadyAction {
    if start_hidden || autostart {
        RuntimeReadyAction::KeepHidden
    } else {
        RuntimeReadyAction::ShowMain
    }
}

pub fn show_main_window<R: Runtime>(app: &AppHandle<R>) -> Result<(), LifecycleError> {
    let window = app
        .get_webview_window(MAIN_WINDOW_LABEL)
        .ok_or_else(|| LifecycleError::new("main_window_missing", "AgentNotify 主窗口不存在"))?;
    let _ = window.unminimize();
    window.show().map_err(|error| {
        LifecycleError::new(
            "main_window_show_failed",
            format!("显示主窗口失败：{error}"),
        )
    })?;
    let _ = window.set_focus();
    Ok(())
}

pub fn show_main_window_when_ready<R: Runtime>(
    app: &AppHandle<R>,
    controller: &LifecycleController,
) -> Result<(), LifecycleError> {
    match runtime_ready_action(controller.is_runtime_ready()) {
        RuntimeReadyAction::KeepHidden => Ok(()),
        RuntimeReadyAction::ShowMain => show_main_window(app),
    }
}

pub fn install<R: Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
    builder.on_window_event(|window, event| {
        if window.label() != MAIN_WINDOW_LABEL {
            return;
        }
        let WindowEvent::CloseRequested { api, .. } = event else {
            return;
        };

        let controller = window.state::<LifecycleController>().inner().clone();
        let action = if controller.is_quitting() {
            WindowAction::AllowClose
        } else {
            window_action_for_close(controller.runtime_state(), true)
        };
        if action == WindowAction::Hide {
            api.prevent_close();
            if let Err(error) = window.hide() {
                tracing::error!(code = "main_window_hide_failed", %error, "关闭主窗口时隐藏失败");
            }
        }
    })
}
