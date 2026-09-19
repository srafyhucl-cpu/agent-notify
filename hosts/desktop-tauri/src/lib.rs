pub mod bridge;
pub mod lifecycle;
pub mod platform;
pub mod update;

use lifecycle::LifecycleController;
use tauri::Manager;

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
            #[cfg(windows)]
            {
                let platform_host = platform::windows::WindowsPlatformHost::from_environment()?;
                app.manage(platform_host);
            }
            specta.mount_events(app);
            lifecycle::tray::install(app.handle())?;
            schedule_smoke_exit(app.handle());
            Ok(())
        });

    lifecycle::window::install(builder)
}

pub fn run() {
    build_test_app()
        .run(tauri::generate_context!())
        .expect("AgentNotify 桌面宿主启动失败");
}

fn schedule_smoke_exit(app: &tauri::AppHandle) {
    let Ok(raw) = std::env::var("AGENT_NOTIFY_SMOKE_EXIT_AFTER_MS") else {
        return;
    };
    let Ok(delay_ms) = raw.parse::<u64>() else {
        tracing::warn!(
            code = "smoke_exit_delay_invalid",
            value = %raw,
            "忽略无效的桌面 smoke 退出时间"
        );
        return;
    };
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
        app.exit(0);
    });
}
