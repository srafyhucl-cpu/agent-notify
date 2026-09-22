pub mod bridge;
pub mod lifecycle;
pub mod platform;
pub mod production;
pub mod update;

use std::sync::Arc;

use lifecycle::{LifecycleController, window::determine_runtime_ready_action};
use tauri::Manager;

pub fn build_test_app() -> tauri::Builder<tauri::Wry> {
    build_app_with_lifecycle(LifecycleController::new())
}

pub fn build_app_with_lifecycle(lifecycle: LifecycleController) -> tauri::Builder<tauri::Wry> {
    let specta = bridge::specta_builder();
    let bridge_state = bridge::BridgeState::unavailable();
    let builder = tauri::Builder::default()
        .plugin(lifecycle::single_instance::plugin())
        .plugin(lifecycle::autostart::plugin())
        .plugin(tauri_plugin_notification::init())
        .manage(lifecycle.clone())
        .manage(bridge_state.clone())
        .invoke_handler(specta.invoke_handler())
        .setup(move |app| {
            let paths = platform::AppPaths::from_environment()?;
            paths.ensure()?;

            #[cfg(windows)]
            let secret_store = {
                let system_ui = Arc::new(platform::windows::WindowsSystemUi::new(
                    app.handle().clone(),
                    paths.clone(),
                ));
                let host = platform::windows::WindowsPlatformHost::with_system_ui(
                    paths.clone(),
                    system_ui,
                );
                let secret_store = host.secret_store();
                app.manage(host);
                secret_store
            };

            #[cfg(not(windows))]
            let secret_store: Arc<dyn agentnotify_application::SecretStore> = {
                panic!("桌面端当前仅支持 Windows 平台");
            };

            specta.mount_events(app);
            lifecycle::tray::install(app.handle())?;
            schedule_smoke_exit(app.handle());

            let app_handle = app.handle().clone();
            let lifecycle_clone = lifecycle.clone();
            let bridge_state_clone = bridge_state.clone();

            tauri::async_runtime::spawn(async move {
                match production::bootstrap_production(app_handle.clone(), paths, secret_store)
                    .await
                {
                    Ok((coordinator, service)) => {
                        let start_hidden = coordinator
                            .settings()
                            .load_settings()
                            .await
                            .map(|s| s.start_hidden)
                            .unwrap_or(false);
                        let is_autostart = std::env::args().any(|arg| arg == "--autostart");
                        let action = determine_runtime_ready_action(start_hidden, is_autostart);

                        if let Err(error) = lifecycle_clone
                            .attach_runtime_with_action(
                                coordinator.clone(),
                                Arc::new(coordinator.settings()),
                                action,
                            )
                            .await
                        {
                            tracing::error!(%error, "接入生命周期控制器失败");
                        } else {
                            let _ = lifecycle::tray::sync_tray_paused(
                                &app_handle,
                                lifecycle_clone.is_paused(),
                            );
                            let _ = lifecycle::window::show_main_window_when_ready(
                                &app_handle,
                                &lifecycle_clone,
                            );
                        }

                        bridge_state_clone.replace(service).await;
                        tracing::info!("桌面生产运行时及宿主服务初始化完成");
                    }
                    Err(error) => {
                        tracing::error!(%error, "生产运行时异步初始化失败");
                        // 让等待中的命令立即拿到具体原因，而不是空等到超时
                        bridge_state_clone.fail(&error.to_string()).await;
                    }
                }
            });

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
        // smoke 退出必须与托盘退出走同一条优雅关闭路径，否则 SQLite 不会完成 checkpoint。
        let controller = app
            .try_state::<LifecycleController>()
            .map(|state| state.inner().clone());
        if let Some(controller) = controller {
            if let Err(error) = controller.shutdown_for_quit().await {
                tracing::warn!(code = error.code(), "smoke 退出时关闭运行时失败");
            }
        }
        app.exit(0);
    });
}
