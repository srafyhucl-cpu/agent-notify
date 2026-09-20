use tauri::{
    AppHandle, Emitter, Manager, Wry,
    menu::{MenuBuilder, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};
use tauri_plugin_notification::NotificationExt;

use super::{LifecycleController, LifecycleError, window::show_main_window};

pub const TRAY_ID: &str = "agentnotify-tray";
pub const TRAY_SHOW_ID: &str = "tray.show";
pub const TRAY_PAUSE_ID: &str = "tray.pause";
pub const TRAY_QUIT_ID: &str = "tray.quit";
pub const TRAY_STATE_EVENT: &str = "tray.state.changed";

const SHOW_LABEL: &str = "显示 AgentNotify";
const PAUSE_LABEL: &str = "暂停通知";
const RESUME_LABEL: &str = "恢复通知";
const QUIT_LABEL: &str = "退出";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrayMenuAction {
    Show,
    SetPaused(bool),
    Quit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TrayMenuEntry {
    id: &'static str,
    label: &'static str,
    action: TrayMenuAction,
}

impl TrayMenuEntry {
    pub fn id(&self) -> &'static str {
        self.id
    }

    pub fn label(&self) -> &'static str {
        self.label
    }

    pub fn action(&self) -> TrayMenuAction {
        self.action
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TrayMenu {
    pub show: TrayMenuEntry,
    pub pause: TrayMenuEntry,
    pub quit: TrayMenuEntry,
}

pub fn tray_menu(paused: bool) -> TrayMenu {
    TrayMenu {
        show: TrayMenuEntry {
            id: TRAY_SHOW_ID,
            label: SHOW_LABEL,
            action: TrayMenuAction::Show,
        },
        pause: TrayMenuEntry {
            id: TRAY_PAUSE_ID,
            label: if paused { RESUME_LABEL } else { PAUSE_LABEL },
            action: TrayMenuAction::SetPaused(!paused),
        },
        quit: TrayMenuEntry {
            id: TRAY_QUIT_ID,
            label: QUIT_LABEL,
            action: TrayMenuAction::Quit,
        },
    }
}

struct TrayMenuState {
    pause_item: MenuItem<Wry>,
}

impl TrayMenuState {
    fn new(pause_item: MenuItem<Wry>) -> Self {
        Self { pause_item }
    }

    fn apply_paused(&self, paused: bool) -> Result<(), LifecycleError> {
        let label = tray_menu(paused).pause.label();
        self.pause_item.set_text(label).map_err(|error| {
            LifecycleError::new(
                "tray_pause_label_failed",
                format!("更新托盘暂停菜单失败：{error}"),
            )
        })
    }
}

pub fn sync_tray_paused(app: &AppHandle<Wry>, paused: bool) -> Result<(), LifecycleError> {
    if let Some(state) = app.try_state::<TrayMenuState>() {
        state.apply_paused(paused)?;
    }
    let _ = app.emit(TRAY_STATE_EVENT, if paused { "paused" } else { "running" });
    Ok(())
}

pub fn install(app: &AppHandle<Wry>) -> Result<(), LifecycleError> {
    let paused = app.state::<LifecycleController>().inner().is_paused();
    let model = tray_menu(paused);
    let show_item = MenuItem::with_id(app, model.show.id(), model.show.label(), true, None::<&str>)
        .map_err(map_menu_error)?;
    let pause_item = MenuItem::with_id(
        app,
        model.pause.id(),
        model.pause.label(),
        true,
        None::<&str>,
    )
    .map_err(map_menu_error)?;
    let quit_item = MenuItem::with_id(app, model.quit.id(), model.quit.label(), true, None::<&str>)
        .map_err(map_menu_error)?;
    let menu = MenuBuilder::new(app)
        .item(&show_item)
        .separator()
        .item(&pause_item)
        .separator()
        .item(&quit_item)
        .build()
        .map_err(map_menu_error)?;

    app.manage(TrayMenuState::new(pause_item));

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .tooltip("AgentNotify")
        .show_menu_on_left_click(false)
        .on_menu_event(handle_menu_event)
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                if let Err(error) = show_main_window(tray.app_handle()) {
                    tracing::error!(
                        code = error.code(),
                        message = error.message(),
                        "托盘唤起窗口失败"
                    );
                }
            }
        });

    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }
    builder.build(app).map_err(|error| {
        LifecycleError::new("tray_create_failed", format!("创建系统托盘失败：{error}"))
    })?;
    Ok(())
}

fn handle_menu_event(app: &AppHandle<Wry>, event: tauri::menu::MenuEvent) {
    match event.id().0.as_str() {
        TRAY_SHOW_ID => {
            if let Err(error) = show_main_window(app) {
                notify_error(app, &error);
            }
        }
        TRAY_PAUSE_ID => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let controller = app.state::<LifecycleController>().inner().clone();
                let next = !controller.is_paused();
                match controller.set_paused(next).await {
                    Ok(paused) => {
                        if let Some(state) = app.try_state::<TrayMenuState>() {
                            if let Err(error) = state.apply_paused(paused) {
                                notify_error(&app, &error);
                            }
                        }
                        let _ = app.emit(
                            TRAY_STATE_EVENT,
                            if controller.is_paused() {
                                "paused"
                            } else {
                                "running"
                            },
                        );
                    }
                    Err(error) => notify_error(&app, &error),
                }
            });
        }
        TRAY_QUIT_ID => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let controller = app.state::<LifecycleController>().inner().clone();
                match controller.shutdown_for_quit().await {
                    Ok(()) => app.exit(0),
                    Err(error) => notify_error(&app, &error),
                }
            });
        }
        _ => {}
    }
}

fn notify_error(app: &AppHandle<Wry>, error: &LifecycleError) {
    let _ = app
        .notification()
        .builder()
        .title("AgentNotify")
        .body(error.message())
        .show();
}

fn map_menu_error(error: tauri::Error) -> LifecycleError {
    LifecycleError::new(
        "tray_menu_create_failed",
        format!("创建托盘菜单失败：{error}"),
    )
}
