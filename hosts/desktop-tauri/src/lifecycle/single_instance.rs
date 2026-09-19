use tauri::{AppHandle, Runtime, plugin::TauriPlugin};

use super::window::show_main_window;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SecondInstanceAction {
    ShowExistingMain,
}

pub fn second_instance_action() -> SecondInstanceAction {
    SecondInstanceAction::ShowExistingMain
}

pub fn plugin<R: Runtime>() -> TauriPlugin<R> {
    tauri_plugin_single_instance::init(|app, _args, _cwd| {
        focus_existing_instance(app);
    })
}

fn focus_existing_instance<R: Runtime>(app: &AppHandle<R>) {
    if second_instance_action() != SecondInstanceAction::ShowExistingMain {
        return;
    }
    if let Err(error) = show_main_window(app) {
        tracing::error!(
            code = error.code(),
            message = error.message(),
            "唤起已有主窗口失败"
        );
    }
}
