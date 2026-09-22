use std::sync::{Arc, RwLock};
use std::time::Duration;

use agentnotify_agent_sdk::AgentRegistry;
use agentnotify_application::{Clock, IdGenerator, NotificationPolicy, ReplyConfig, SecretStore};
use agentnotify_channel_clawbot::ClawBotLoginAdapter;
use agentnotify_channel_sdk::ChannelRegistry;
use agentnotify_domain::Timestamp;
use agentnotify_runtime::{
    AppRuntime, MigrationConfig, RuntimeConfig, RuntimeError, RuntimeHandle, RuntimeSnapshot,
    RuntimeState as CoreRuntimeState, TelemetryConfig, start_migration_diagnostics,
};
use agentnotify_storage_sqlite::{AgentConfigRecord, LegacyPaths, SqliteStore};
use tokio::sync::Mutex;

use super::agents::{build_agent_registry, load_agent_configs, sync_commandcode_reply_window};
use super::settings::ProductionSettingsStore;
use super::targets::ProductionTargetProvider;
use crate::bridge::error::CommandError;
use crate::lifecycle::LifecycleError;
use crate::lifecycle::pause::RuntimeControl;
use crate::lifecycle::window::RuntimeState;
use crate::platform::AppPaths;

const RUNTIME_LOG_FILE_NAME: &str = "runtime.log";

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        Timestamp::now_utc()
    }
}

pub struct UuidGenerator;

impl IdGenerator for UuidGenerator {
    fn next_id(&self) -> String {
        uuid::Uuid::new_v4().to_string()
    }
}

#[derive(Clone)]
pub struct ProductionRuntimeCoordinator {
    inner: Arc<Mutex<Option<RuntimeHandle>>>,
    restart_lock: Arc<Mutex<()>>,
    paths: AppPaths,
    store: Arc<SqliteStore>,
    settings: ProductionSettingsStore,
    secret_store: Arc<dyn SecretStore>,
    /// 当前生效的 Agent 注册表；`update_agent_config` 后按最新配置整体替换。
    agent_registry: Arc<RwLock<Arc<AgentRegistry>>>,
    channel_registry: Arc<ChannelRegistry>,
    login_adapter: Arc<ClawBotLoginAdapter>,
    target_provider: Arc<ProductionTargetProvider>,
    app_version: String,
    platform: String,
    ingress_pipe_enabled: bool,
}

impl ProductionRuntimeCoordinator {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        paths: AppPaths,
        store: Arc<SqliteStore>,
        settings: ProductionSettingsStore,
        secret_store: Arc<dyn SecretStore>,
        agent_registry: Arc<AgentRegistry>,
        channel_registry: Arc<ChannelRegistry>,
        login_adapter: Arc<ClawBotLoginAdapter>,
        target_provider: Arc<ProductionTargetProvider>,
        app_version: impl Into<String>,
        platform: impl Into<String>,
    ) -> Self {
        Self::with_ingress_pipe(
            paths,
            store,
            settings,
            secret_store,
            agent_registry,
            channel_registry,
            login_adapter,
            target_provider,
            app_version,
            platform,
            true,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn with_ingress_pipe(
        paths: AppPaths,
        store: Arc<SqliteStore>,
        settings: ProductionSettingsStore,
        secret_store: Arc<dyn SecretStore>,
        agent_registry: Arc<AgentRegistry>,
        channel_registry: Arc<ChannelRegistry>,
        login_adapter: Arc<ClawBotLoginAdapter>,
        target_provider: Arc<ProductionTargetProvider>,
        app_version: impl Into<String>,
        platform: impl Into<String>,
        ingress_pipe_enabled: bool,
    ) -> Self {
        Self {
            inner: Arc::new(Mutex::new(None)),
            restart_lock: Arc::new(Mutex::new(())),
            paths,
            store,
            settings,
            secret_store,
            agent_registry: Arc::new(RwLock::new(agent_registry)),
            channel_registry,
            login_adapter,
            target_provider,
            app_version: app_version.into(),
            platform: platform.into(),
            ingress_pipe_enabled,
        }
    }

    pub fn paths(&self) -> AppPaths {
        self.paths.clone()
    }

    pub fn store(&self) -> Arc<SqliteStore> {
        self.store.clone()
    }

    pub fn settings(&self) -> ProductionSettingsStore {
        self.settings.clone()
    }

    pub fn agent_registry(&self) -> Arc<AgentRegistry> {
        self.agent_registry
            .read()
            .map(|registry| registry.clone())
            .unwrap_or_else(|poisoned| poisoned.into_inner().clone())
    }

    /// 更新单个 Agent 配置：先按合并后的配置校验能否构建注册表，无效配置在这里直接失败，
    /// 不写库、不替换内存注册表——否则带着无效行重启会让宿主起不来。
    /// 校验通过后才落库，并用最新配置整体替换注册表、复用既有重启流程让配置生效。
    pub async fn update_agent_config(
        &self,
        agent_id: &str,
        enabled: bool,
        config: &serde_json::Value,
    ) -> Result<RuntimeSnapshot, CommandError> {
        let mut candidate = load_agent_configs(&self.store).await?;
        let mut record = candidate
            .get(agent_id)
            .cloned()
            .unwrap_or(AgentConfigRecord {
                enabled: true,
                config: serde_json::json!({}),
                updated_at: Timestamp::now_utc(),
            });
        record.enabled = enabled;
        record.config = config.clone();
        candidate.insert(agent_id.to_owned(), record);

        let registry = build_agent_registry(&self.paths, &candidate)?;

        self.store
            .upsert_agent_config(agent_id, enabled, config)
            .await
            .map_err(|error| CommandError::new("agent_config_save_failed", error.to_string()))?;

        // 保存成功后把同一个值写给 Command Code mod；写不出去就明确报错，
        // 不替换内存注册表、也不重启运行时（数据库里的值下次启动仍会生效）。
        sync_commandcode_reply_window(&self.paths, &candidate)?;

        *self.agent_registry.write().map_err(|_| {
            CommandError::new("agent_registry_unavailable", "Agent 注册表暂不可用，请重试")
        })? = Arc::new(registry);
        self.start_or_restart().await
    }

    pub fn channel_registry(&self) -> Arc<ChannelRegistry> {
        self.channel_registry.clone()
    }

    pub fn login_adapter(&self) -> Arc<ClawBotLoginAdapter> {
        self.login_adapter.clone()
    }

    pub fn secret_store(&self) -> Arc<dyn SecretStore> {
        self.secret_store.clone()
    }

    fn build_runtime_config(&self) -> RuntimeConfig {
        let database_path = self.paths.data_dir.join("state.db");
        let lock_path = self.paths.data_dir.join("AgentNotify.runtime.lock");
        let temp_root = std::env::temp_dir();
        let activity_path = temp_root.join("agent-notify").join("widget-alive.txt");
        let legacy_paths =
            LegacyPaths::new(&self.paths.config_dir, &temp_root, &self.paths.data_dir);

        let migration = MigrationConfig::new(legacy_paths, self.secret_store.clone(), lock_path)
            .watch_legacy_activity([activity_path]);

        RuntimeConfig {
            database_path,
            migration: Some(migration),
            agents: self.agent_registry(),
            channels: self.channel_registry.clone(),
            clock: Arc::new(SystemClock),
            id_generator: Arc::new(UuidGenerator),
            notification_policy: NotificationPolicy::default(),
            delivery_targets: Vec::new(),
            reply_targets: Vec::new(),
            reply_config: ReplyConfig::default(),
            target_provider: Some(self.target_provider.clone()),
            app_version: self.app_version.clone(),
            platform: self.platform.clone(),
            ingress_spool_dir: Some(self.paths.spool_dir.clone()),
            ingress_pipe_enabled: self.ingress_pipe_enabled,
            telemetry: Some(TelemetryConfig {
                log_path: self.paths.log_dir.join(RUNTIME_LOG_FILE_NAME),
            }),
            inbound_capacity: 256,
            worker_idle_delay: Duration::from_millis(250),
            status_refresh_interval: Duration::from_millis(100),
            channel_poll_interval: Duration::from_millis(100),
        }
    }

    pub async fn start_or_restart(&self) -> Result<RuntimeSnapshot, CommandError> {
        let _guard = self.restart_lock.lock().await;

        // 1. 先安全停用旧的运行时实例，释放独占锁与正在运行的任务
        let old_handle = {
            let mut slot = self.inner.lock().await;
            slot.take()
        };
        if let Some(mut old) = old_handle {
            if let Err(error) = old.shutdown().await {
                tracing::warn!(%error, "停用旧运行时实例时出现警告");
            }
        }

        // 2. 重新加载最新配置并启动新运行时
        let config = self.build_runtime_config();
        let start_result = AppRuntime::start(config.clone()).await;

        let handle = match start_result {
            Ok(handle) => handle,
            Err(RuntimeError::Migration(failure)) => {
                // 如果是运行时锁被占用，说明外部实例仍存活，直接报错暴露，不降级为迁移模式
                if failure.code == "runtime_lock_unavailable" {
                    return Err(CommandError::new(
                        failure.code,
                        "运行时锁已被其他实例占用，无法启动",
                    ));
                }
                tracing::warn!(
                    code = %failure.code,
                    message = %failure.message,
                    "旧数据迁移失败，启动迁移诊断模式"
                );
                start_migration_diagnostics(config, *failure)
                    .await
                    .map_err(|error| {
                        CommandError::new(error.code(), format!("启动迁移诊断模式失败：{error}"))
                    })?
            }
            Err(error) => {
                return Err(CommandError::new(
                    error.code(),
                    format!("启动桌面运行时失败：{error}"),
                ));
            }
        };

        // 继承当前的暂停状态
        if let Ok(settings_dto) = self.settings.load_settings().await {
            handle.set_outbox_paused(settings_dto.notifications_paused);
        }

        let snapshot = handle.snapshot();

        let mut slot = self.inner.lock().await;
        *slot = Some(handle);

        Ok(snapshot)
    }

    pub async fn current_snapshot(&self) -> Option<RuntimeSnapshot> {
        let slot = self.inner.lock().await;
        slot.as_ref().map(|h| h.snapshot())
    }

    pub async fn is_outbox_paused(&self) -> bool {
        let slot = self.inner.lock().await;
        slot.as_ref().map(|h| h.outbox_paused()).unwrap_or(false)
    }

    pub async fn set_outbox_paused(&self, paused: bool) -> Result<(), CommandError> {
        let slot = self.inner.lock().await;
        if let Some(handle) = slot.as_ref() {
            handle.set_outbox_paused(paused);
            Ok(())
        } else {
            Err(CommandError::new("runtime_unavailable", "运行时尚未启动"))
        }
    }

    pub async fn ingest(
        &self,
        envelope: agentnotify_agent_sdk::AgentEventEnvelope,
    ) -> Result<agentnotify_application::IngestResult, CommandError> {
        let slot = self.inner.lock().await;
        let handle = slot
            .as_ref()
            .ok_or_else(|| CommandError::new("runtime_unavailable", "运行时尚未启动"))?;
        handle
            .ingest(envelope)
            .await
            .map_err(|error| CommandError::new(error.code(), format!("事件入站失败：{error}")))
    }

    pub async fn subscribe_events(
        &self,
    ) -> Option<tokio::sync::broadcast::Receiver<agentnotify_runtime::RuntimeEvent>> {
        let slot = self.inner.lock().await;
        slot.as_ref().map(|h| h.subscribe_events())
    }

    pub async fn retry_legacy_migration(&self) -> Result<RuntimeSnapshot, CommandError> {
        self.start_or_restart().await
    }

    pub async fn shutdown_runtime(&self) -> Result<(), CommandError> {
        let _guard = self.restart_lock.lock().await;
        let mut slot = self.inner.lock().await;
        if let Some(mut handle) = slot.take() {
            handle
                .shutdown()
                .await
                .map_err(|error| CommandError::new(error.code(), error.to_string()))?;
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl RuntimeControl for ProductionRuntimeCoordinator {
    fn state(&self) -> RuntimeState {
        // 尝试非阻塞读取当前 slot
        if let Ok(slot) = self.inner.try_lock() {
            if let Some(handle) = slot.as_ref() {
                return match handle.runtime_state() {
                    CoreRuntimeState::Starting => RuntimeState::Starting,
                    CoreRuntimeState::Running => {
                        if handle.outbox_paused() {
                            RuntimeState::Paused
                        } else {
                            RuntimeState::Running
                        }
                    }
                    CoreRuntimeState::Degraded => {
                        if handle.outbox_paused() {
                            RuntimeState::Paused
                        } else {
                            RuntimeState::Running
                        }
                    }
                    CoreRuntimeState::MigrationRequired => RuntimeState::Starting,
                    CoreRuntimeState::Stopping => RuntimeState::Stopping,
                    CoreRuntimeState::Stopped => RuntimeState::Stopped,
                    CoreRuntimeState::Failed => RuntimeState::Failed,
                };
            }
        }
        RuntimeState::Starting
    }

    async fn set_outbox_paused(&self, paused: bool) -> Result<(), LifecycleError> {
        self.set_outbox_paused(paused)
            .await
            .map_err(|error| LifecycleError::new(error.code(), error.message()))
    }

    async fn shutdown_runtime(&self) -> Result<(), LifecycleError> {
        self.shutdown_runtime()
            .await
            .map_err(|error| LifecycleError::new(error.code, error.message))
    }
}
