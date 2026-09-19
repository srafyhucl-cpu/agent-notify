pub mod bridge;
pub mod lifecycle;
pub mod platform;

use lifecycle::LifecycleController;

pub fn build_test_app() -> tauri::Builder<tauri::Wry> {
    build_app_with_lifecycle(LifecycleController::new())
}

pub fn build_app_with_lifecycle(lifecycle: LifecycleController) -> tauri::Builder<tauri::Wry> {
    let specta = bridge::specta_builder();
    let builder = tauri::Builder::default()
        .plugin(lifecycle::single_instance::plugin())
        .plugin(lifecycle::autostart::plugin())
        .plugin(tauri_plugin_notification::init())
        .manage(lifecycle)
        .invoke_handler(specta.invoke_handler())
        .setup(move |app| {
            specta.mount_events(app);
            lifecycle::tray::install(app.handle())?;
            Ok(())
        });

    lifecycle::window::install(builder)
}

pub fn run() {
    build_test_app()
        .run(tauri::generate_context!())
        .expect("AgentNotify 桌面宿主启动失败");
}
