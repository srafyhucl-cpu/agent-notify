use std::fs;
use std::path::Path;

use specta_typescript::Typescript;
use tauri_specta::{Builder, collect_commands, collect_events};

pub mod commands;
pub mod dto;
pub mod error;
pub mod events;

pub use commands::{BridgeState, HostCommandService};
pub use dto::*;
pub use error::CommandError;
pub use events::{ChannelLoginChangedEvent, DeliveryChangedEvent, SnapshotChangedEvent};

pub const BUSINESS_COMMAND_NAMES: [&str; 19] = [
    "get_snapshot",
    "list_agents",
    "update_agent_config",
    "list_channel_accounts",
    "begin_channel_login",
    "submit_channel_login_code",
    "logout_channel_account",
    "enable_channel_account",
    "disable_channel_account",
    "send_test_notification",
    "list_notifications",
    "get_notification_detail",
    "retry_delivery",
    "get_diagnostics",
    "get_settings",
    "update_settings",
    "set_runtime_paused",
    "quit_app",
    "get_update_status",
];

pub fn specta_builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new()
        .typ::<dto::HostEvent>()
        .commands(collect_commands![
            commands::get_snapshot,
            commands::list_agents,
            commands::update_agent_config,
            commands::list_channel_accounts,
            commands::begin_channel_login,
            commands::submit_channel_login_code,
            commands::logout_channel_account,
            commands::enable_channel_account,
            commands::disable_channel_account,
            commands::send_test_notification,
            commands::list_notifications,
            commands::get_notification_detail,
            commands::retry_delivery,
            commands::get_diagnostics,
            commands::get_settings,
            commands::update_settings,
            commands::set_runtime_paused,
            commands::quit_app,
            commands::get_update_status,
        ])
        .events(collect_events![
            events::SnapshotChangedEvent,
            events::DeliveryChangedEvent,
            events::ChannelLoginChangedEvent,
        ])
        .dangerously_cast_bigints_to_number()
        .error_handling(tauri_specta::ErrorHandlingMode::Throw)
}

pub fn export_typescript_bindings(path: impl AsRef<Path>) -> Result<(), specta_typescript::Error> {
    let path = path.as_ref();
    specta_builder().export(
        Typescript::default().header("// 此文件由 Rust 生成，禁止手改。\n"),
        path,
    )?;

    let source = fs::read_to_string(path)?;
    fs::write(path, format!("{}\n", source.trim_end()))?;
    Ok(())
}
