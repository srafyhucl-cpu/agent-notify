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
use crate::update::{UpdateService, UpdateTransport};

use agents::{
    assemble_agents, legacy_installation_detected, load_agent_configs, seed_disabled_agent_configs,
};

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
    let updates = production_update_service(&paths)?;
    bootstrap_internal(None, paths, secret_store, updates).await
}

/// 测试用装配：注入假更新传输，命令层不会访问真实网络。
pub async fn bootstrap_headless_with_update_transport(
    paths: AppPaths,
    secret_store: Arc<dyn SecretStore>,
    transport: Arc<dyn UpdateTransport>,
) -> Result<
    (
        Arc<ProductionRuntimeCoordinator>,
        Arc<ProductionHostCommandService>,
    ),
    CommandError,
> {
    let updates = Arc::new(UpdateService::new(
        paths.temp_dir.clone(),
        crate::update::UpdateConfig::from_environment(),
        transport,
    ));
    bootstrap_internal(None, paths, secret_store, updates).await
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
    let updates = production_update_service(&paths)?;
    let (coordinator, service) =
        bootstrap_internal(Some(app.clone()), paths, secret_store, updates).await?;

    // 启动事件转发任务
    let forwarder = EventForwarder::new(app.clone(), coordinator.clone());
    forwarder.start();

    // 升级后首启可能因旧版心跳仍在判定窗口内落到迁移诊断模式，后台自愈并在恢复后通知界面。
    coordinator.spawn_migration_autoretry(Some(app));

    Ok((coordinator, service))
}

fn production_update_service(paths: &AppPaths) -> Result<Arc<UpdateService>, CommandError> {
    UpdateService::from_environment(paths)
        .map(Arc::new)
        .map_err(|error| CommandError::new(error.code(), error.message().to_owned()))
}

async fn bootstrap_internal(
    app: Option<AppHandle<Wry>>,
    paths: AppPaths,
    secret_store: Arc<dyn SecretStore>,
    updates: Arc<UpdateService>,
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

    // 注册全部 Agent；先读已保存配置，界面里配置的路径必须作用到适配器。
    // 应用自身的收件箱走 AppPaths，外部 Agent 目录由适配器解析真实安装位置。
    // 装配同时把 Command Code 回复窗口写给 mod（应用自己的 window.json）。
    let agent_configs = load_agent_configs(&store).await?;
    let agent_registry = assemble_agents(&paths, &agent_configs)?;
    // 新接入的适配器先补一条“默认关闭”的配置行，用户在界面启用后才会推送；
    // 旧版（Go 版）遗留存在时旧 Agent 的开关由迁移继承，这里不抢写默认关闭行。
    seed_disabled_agent_configs(
        &store,
        &agent_registry,
        legacy_installation_detected(&paths),
    )
    .await?;
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
        updates,
    ));

    // 启动生产运行时
    let _ = coordinator.start_or_restart().await?;

    Ok((coordinator, service))
}
