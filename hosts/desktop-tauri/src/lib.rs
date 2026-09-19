pub mod bridge;
pub mod platform;

pub fn build_test_app() -> tauri::Builder<tauri::Wry> {
    let specta = bridge::specta_builder();
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .invoke_handler(specta.invoke_handler())
        .setup(move |app| {
            specta.mount_events(app);
            Ok(())
        })
}

pub fn run() {
    build_test_app()
        .run(tauri::generate_context!())
        .expect("AgentNotify 桌面宿主启动失败");
}
