mod agents;
pub mod events;
pub mod runtime;
pub mod service;
pub mod settings;
pub mod targets;

use std::sync::Arc;

use agentnotify_application::SecretStore;
use agentnotify_channel_clawbot::{ClawBotChannel, ClawBotHttpClient, ClawBotLoginAdapter};
use agentnotify_channel_sdk::ChannelRegistry;
use agentnotify_storage_sqlite::SqliteStore;
use tauri::{AppHandle, Wry};

use crate::bridge::error::CommandError;
use crate::platform::AppPaths;

use agents::{build_agent_registry, seed_disabled_agent_configs};

pub use events::EventForwarder;
pub use runtime::ProductionRuntimeCoordinator;
pub use service::ProductionHostCommandService;
pub use settings::ProductionSettingsStore;
pub use targets::ProductionTargetProvider;

pub async fn bootstrap_headless(
    paths: AppPaths,
    secret_store: Arc<dyn SecretStore>,
) -> Result<
    (
        Arc<ProductionRuntimeCoordinator>,
        Arc<ProductionHostCommandService>,
    ),
    CommandError,
> {
    bootstrap_internal(None, paths, secret_store).await
}

pub async fn bootstrap_production(
    app: AppHandle<Wry>,
    paths: AppPaths,
    secret_store: Arc<dyn SecretStore>,
) -> Result<
    (
        Arc<ProductionRuntimeCoordinator>,
        Arc<ProductionHostCommandService>,
    ),
    CommandError,
> {
    let (coordinator, service) = bootstrap_internal(Some(app.clone()), paths, secret_store).await?;

    // 启动事件转发任务
    let forwarder = EventForwarder::new(app, coordinator.clone());
    forwarder.start();

    Ok((coordinator, service))
}

async fn bootstrap_internal(
    app: Option<AppHandle<Wry>>,
    paths: AppPaths,
    secret_store: Arc<dyn SecretStore>,
) -> Result<
    (
        Arc<ProductionRuntimeCoordinator>,
        Arc<ProductionHostCommandService>,
    ),
    CommandError,
> {
    paths
        .ensure()
        .map_err(|e| CommandError::new("paths_ensure_failed", e.to_string()))?;

    let db_path = paths.data_dir.join("state.db");
    let store =
        Arc::new(SqliteStore::open(&db_path).map_err(|e| {
            CommandError::new("database_open_failed", format!("打开数据库失败：{e}"))
        })?);

    let settings = ProductionSettingsStore::new(store.clone(), &paths.config_dir);

    // 注册全部 Agent；应用自身的收件箱走 AppPaths，外部 Agent 目录由适配器解析真实安装位置
    let agent_registry = build_agent_registry(&paths)?;
    // 新接入的适配器先补一条“默认关闭”的配置行，用户在界面启用后才会推送
    seed_disabled_agent_configs(&store, &agent_registry).await?;
    let agent_registry = Arc::new(agent_registry);

    // 注册 ClawBot Channel
    let clawbot_channel =
        ClawBotChannel::new(secret_store.clone()).with_account_store(store.clone());
    let mut channel_registry = ChannelRegistry::default();
    channel_registry
        .register(Arc::new(clawbot_channel))
        .map_err(|e| CommandError::new("channel_register_failed", e.to_string()))?;
    let channel_registry = Arc::new(channel_registry);

    // 注册 ClawBot 登录适配器
    let http_client =
        Arc::new(ClawBotHttpClient::new().map_err(|e| {
            CommandError::new(e.code(), format!("创建 ClawBot HTTP 客户端失败：{e}"))
        })?);
    let login_adapter = Arc::new(ClawBotLoginAdapter::new(
        http_client,
        secret_store.clone(),
        store.clone(),
    ));

    let target_provider = Arc::new(ProductionTargetProvider::new(
        store.clone(),
        settings.clone(),
        secret_store.clone(),
        agent_registry.clone(),
        channel_registry.clone(),
    ));

    let ingress_pipe_enabled = app.is_some();
    let coordinator = Arc::new(ProductionRuntimeCoordinator::with_ingress_pipe(
        paths,
        store.clone(),
        settings.clone(),
        secret_store,
        agent_registry,
        channel_registry,
        login_adapter,
        target_provider,
        env!("CARGO_PKG_VERSION"),
        "windows",
        ingress_pipe_enabled,
    ));

    let service = Arc::new(ProductionHostCommandService::new(
        app,
        coordinator.clone(),
        store,
        settings,
    ));

    // 启动生产运行时
    let _ = coordinator.start_or_restart().await?;

    Ok((coordinator, service))
}
