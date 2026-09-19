pub fn build_test_app() -> tauri::Builder<tauri::Wry> {
    tauri::Builder::default()
}

pub fn run() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("AgentNotify 桌面宿主启动失败");
}
