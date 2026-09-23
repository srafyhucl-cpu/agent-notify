use std::{path::PathBuf, sync::Arc, time::Duration};

use agentnotify_agent_sdk::{AgentEventEnvelope, AgentRegistry};
use agentnotify_application::{
    Clock, DeliveryService, DeliveryTarget, EventSink, IdGenerator, IngestResult, IngestService,
    NotificationPolicy, ReplyConfig, ReplyService, ReplyTarget, StatusOverview, StatusService,
    StatusStore,
};
use agentnotify_channel_sdk::ChannelRegistry;
use agentnotify_domain::{InboundMessage, SafeError};
use agentnotify_storage_sqlite::SqliteStore;
use tokio::{
    sync::{mpsc, watch},
    task::JoinHandle,
};

use crate::migration::{MigrationConfig, MigrationFailure, MigrationSnapshot, prepare_migration};
use crate::{ComponentSnapshot, EventBus, RuntimeEvent, RuntimeState, Supervisor};
use crate::{TelemetryConfig, TelemetryGuard, init_telemetry};
use time::Duration as TimeDuration;

mod error;
mod workers;

pub use error::RuntimeError;
use workers::{
    build_snapshot, enabled_accounts, map_status_error, run_channel, run_fatal_monitor,
    run_inbound_consumer, run_outbox_worker, run_status_refresher,
};

const DEFAULT_INBOUND_CAPACITY: usize = 256;
const DEFAULT_WORKER_IDLE_DELAY: Duration = Duration::from_millis(250);
const DEFAULT_STATUS_REFRESH_INTERVAL: Duration = Duration::from_secs(2);
const DEFAULT_CHANNEL_POLL_INTERVAL: Duration = Duration::from_millis(100);
const DEFAULT_REPLY_ROUTE_TTL_SECONDS: i64 = 24 * 60 * 60;

struct SnapshotMetadata {
    app_version: String,
    platform: String,
    migration: Arc<MigrationSnapshot>,
}

/// 运行时装配参数。所有适配器必须先在注册表中显式注册。
#[derive(Clone)]
pub struct RuntimeConfig {
    pub database_path: PathBuf,
    pub migration: Option<MigrationConfig>,
    pub agents: Arc<AgentRegistry>,
    pub channels: Arc<ChannelRegistry>,
    pub clock: Arc<dyn Clock>,
    pub id_generator: Arc<dyn IdGenerator>,
    pub notification_policy: NotificationPolicy,
    pub delivery_targets: Vec<DeliveryTarget>,
    pub reply_targets: Vec<ReplyTarget>,
    pub reply_config: ReplyConfig,
    pub target_provider: Option<Arc<dyn RuntimeTargetProvider>>,
    pub app_version: String,
    pub platform: String,
    pub ingress_spool_dir: Option<PathBuf>,
    pub ingress_pipe_enabled: bool,
    pub telemetry: Option<TelemetryConfig>,
    pub inbound_capacity: usize,
    pub worker_idle_delay: Duration,
    pub status_refresh_interval: Duration,
    pub channel_poll_interval: Duration,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedRuntimeTargets {
    pub notification_policy: NotificationPolicy,
    pub delivery_targets: Vec<DeliveryTarget>,
    pub reply_targets: Vec<ReplyTarget>,
    pub reply_config: ReplyConfig,
    pub reply_route_ttl: TimeDuration,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeTargetError {
    code: String,
    message: String,
}

impl RuntimeTargetError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn code(&self) -> &str {
        &self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl std::fmt::Display for RuntimeTargetError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.message())
    }
}

impl std::error::Error for RuntimeTargetError {}

#[async_trait::async_trait]
pub trait RuntimeTargetProvider: Send + Sync {
    /// 在迁移完成后解析启动目标，避免旧账号导入与新运行时启动竞争。
    async fn resolve(&self) -> Result<ResolvedRuntimeTargets, RuntimeTargetError>;
}

impl RuntimeConfig {
    fn inbound_capacity(&self) -> usize {
        if self.inbound_capacity == 0 {
            DEFAULT_INBOUND_CAPACITY
        } else {
            self.inbound_capacity
        }
    }

    fn worker_idle_delay(&self) -> Duration {
        if self.worker_idle_delay.is_zero() {
            DEFAULT_WORKER_IDLE_DELAY
        } else {
            self.worker_idle_delay
        }
    }

    fn status_refresh_interval(&self) -> Duration {
        if self.status_refresh_interval.is_zero() {
            DEFAULT_STATUS_REFRESH_INTERVAL
        } else {
            self.status_refresh_interval
        }
    }

    fn channel_poll_interval(&self) -> Duration {
        if self.channel_poll_interval.is_zero() {
            DEFAULT_CHANNEL_POLL_INTERVAL
        } else {
            self.channel_poll_interval
        }
    }
}

/// runtime 对外暴露的脱敏快照。
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct RuntimeSnapshot {
    pub app_version: String,
    pub platform: String,
    pub state: RuntimeState,
    pub overview: StatusOverview,
    pub components: Vec<ComponentSnapshot>,
    pub diagnostics: Vec<DiagnosticItem>,
    pub migration: MigrationSnapshot,
}

impl RuntimeSnapshot {
    pub fn unhealthy_channel_count(&self) -> usize {
        self.overview.unhealthy_channel_count()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum DiagnosticLevel {
    Ok,
    Warning,
    Error,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct DiagnosticItem {
    pub code: String,
    pub level: DiagnosticLevel,
    pub message: String,
}

pub struct AppRuntime;

impl AppRuntime {
    pub async fn start(config: RuntimeConfig) -> Result<RuntimeHandle, RuntimeError> {
        if config.app_version.trim().is_empty() {
            return Err(RuntimeError::InvalidConfiguration {
                field: "app_version",
            });
        }
        if config.platform.trim().is_empty() {
            return Err(RuntimeError::InvalidConfiguration { field: "platform" });
        }

        let telemetry = config.telemetry.clone().map(init_telemetry).transpose()?;
        tracing::info_span!(
            "host",
            platform = %config.platform,
            host_version = %config.app_version,
            ipc_status = if config.ingress_pipe_enabled { "enabled" } else { "disabled" },
            task_status = "starting",
        )
        .in_scope(|| tracing::info!("启动桌面运行时"));

        let store = Arc::new(SqliteStore::open(&config.database_path)?);
        if !store.integrity_check().await? {
            return Err(RuntimeError::IntegrityCheckFailed);
        }

        let migration = prepare_migration(config.migration.as_ref(), store.clone()).await?;
        let runtime_lock = migration.lock;
        let snapshot_metadata = SnapshotMetadata {
            app_version: config.app_version.clone(),
            platform: config.platform.clone(),
            migration: Arc::new(migration.snapshot),
        };

        let recovery = store.recover_interrupted_work(config.clock.now()).await?;
        if recovery.interrupted_outbox > 0 || recovery.interrupted_claims > 0 {
            tracing::warn!(
                interrupted_outbox = recovery.interrupted_outbox,
                interrupted_claims = recovery.interrupted_claims,
                "启动时收敛了中断的投递与回复，记录不会自动重放"
            );
            let message = format!(
                "上次运行中断，已标记 {} 个未确认投递和 {} 个未确认回复，未自动重试",
                recovery.interrupted_outbox, recovery.interrupted_claims
            );
            store
                .record_error(
                    SafeError::new("runtime_recovered_interrupted_work", message)
                        .expect("启动恢复错误常量必须有效"),
                )
                .await?;
        }

        let event_bus = Arc::new(EventBus::new());
        let resolved_targets = match config.target_provider.as_ref() {
            Some(provider) => provider
                .resolve()
                .await
                .map_err(RuntimeError::TargetProvider)?,
            None => ResolvedRuntimeTargets {
                notification_policy: config.notification_policy.clone(),
                delivery_targets: config.delivery_targets.clone(),
                reply_targets: config.reply_targets.clone(),
                reply_config: config.reply_config.clone(),
                reply_route_ttl: TimeDuration::seconds(DEFAULT_REPLY_ROUTE_TTL_SECONDS),
            },
        };
        let ingest = Arc::new(IngestService::new(
            config.agents.clone(),
            store.clone(),
            event_bus.clone(),
            config.clock.clone(),
            config.id_generator.clone(),
            resolved_targets.notification_policy.clone(),
        ));
        let delivery = Arc::new(
            DeliveryService::new(
                store.clone(),
                config.channels.clone(),
                resolved_targets.delivery_targets.clone(),
                config.clock.clone(),
                config.id_generator.clone(),
                event_bus.clone(),
                Default::default(),
            )
            .with_route_ttl(resolved_targets.reply_route_ttl)
            .with_agent_registry(config.agents.clone()),
        );
        let reply = Arc::new(ReplyService::new(
            store.clone(),
            store.clone(),
            config.channels.clone(),
            config.agents.clone(),
            config.clock.clone(),
            event_bus.clone(),
            resolved_targets.reply_targets.clone(),
            resolved_targets.reply_config.clone(),
            Some(store.clone()),
        )?);
        let status = Arc::new(StatusService::new(
            config.agents.clone(),
            config.channels.clone(),
            store.clone(),
            store.clone(),
        ));

        crate::ingress::drain_before_start(config.ingress_spool_dir.as_deref(), ingest.clone())
            .await?;

        let accounts = enabled_accounts(store.clone(), config.channels.clone()).await?;
        let initial_overview = status.snapshot().await.map_err(map_status_error)?;
        let supervisor = Supervisor::new();
        let (cancel_sender, cancel_receiver) = watch::channel(false);
        let (outbox_pause_sender, outbox_pause_receiver) = watch::channel(false);
        let (inbound_sender, inbound_receiver) =
            mpsc::channel::<InboundMessage>(config.inbound_capacity());
        let (status_sender, status_receiver) = watch::channel(build_snapshot(
            &snapshot_metadata.app_version,
            &snapshot_metadata.platform,
            RuntimeState::Starting,
            initial_overview,
            &supervisor,
            &snapshot_metadata.migration,
        ));

        supervisor.set_runtime_state(RuntimeState::Running);

        let mut tasks = Vec::new();
        for (adapter, account) in accounts {
            let name = format!("channel:{}", account.id);
            let channel_cancel = cancel_receiver.clone();
            let channel_emitter = inbound_sender.clone();
            let channel_supervisor = supervisor.clone();
            let poll_interval = config.channel_poll_interval();
            tasks.push(
                channel_supervisor.spawn_component(name.clone(), async move {
                    run_channel(
                        name,
                        adapter,
                        account,
                        channel_emitter,
                        channel_cancel,
                        poll_interval,
                    )
                    .await
                }),
            );
        }
        drop(inbound_sender);

        if config.ingress_pipe_enabled {
            let ingress_cancel = cancel_receiver.clone();
            tasks.push(supervisor.clone().spawn_component(
                "ingress.pipe",
                crate::platform::run_ingress_server(ingest.clone(), ingress_cancel),
            ));
        }

        let reply_cancel = cancel_receiver.clone();
        tasks.push(supervisor.clone().spawn_component(
            "reply.inbound",
            run_inbound_consumer(inbound_receiver, reply, reply_cancel),
        ));
        let delivery_cancel = cancel_receiver.clone();
        tasks.push(supervisor.clone().spawn_component(
            "delivery.outbox",
            run_outbox_worker(
                delivery,
                outbox_pause_receiver,
                delivery_cancel,
                config.worker_idle_delay(),
            ),
        ));
        let status_cancel = cancel_receiver.clone();
        tasks.push(supervisor.clone().spawn_component(
            "status.refresher",
            run_status_refresher(
                status,
                supervisor.clone(),
                snapshot_metadata,
                status_sender,
                status_cancel,
                config.status_refresh_interval(),
            ),
        ));
        let fatal_cancel = cancel_receiver.clone();
        tasks.push(supervisor.clone().spawn_component(
            "runtime.fatal",
            run_fatal_monitor(supervisor.clone(), fatal_cancel, cancel_sender.clone()),
        ));

        Ok(RuntimeHandle {
            supervisor,
            event_bus,
            status: status_receiver,
            store,
            ingest,
            cancel_sender,
            outbox_pause: outbox_pause_sender,
            tasks,
            stopped: false,
            host_platform: config.platform.clone(),
            host_version: config.app_version.clone(),
            ingress_pipe_enabled: config.ingress_pipe_enabled,
            migration_required: false,
            runtime_lock,
            _telemetry: telemetry,
        })
    }
}

/// 迁移失败时只构建可读取诊断的 runtime，不启动渠道、Outbox 或 ingress。
pub async fn start_migration_diagnostics(
    config: RuntimeConfig,
    failure: MigrationFailure,
) -> Result<RuntimeHandle, RuntimeError> {
    if config.app_version.trim().is_empty() {
        return Err(RuntimeError::InvalidConfiguration {
            field: "app_version",
        });
    }
    if config.platform.trim().is_empty() {
        return Err(RuntimeError::InvalidConfiguration { field: "platform" });
    }

    let telemetry = config.telemetry.clone().map(init_telemetry).transpose()?;
    let store = Arc::new(SqliteStore::open(&config.database_path)?);
    if !store.integrity_check().await? {
        return Err(RuntimeError::IntegrityCheckFailed);
    }

    let event_bus = Arc::new(EventBus::new());
    let ingest = Arc::new(IngestService::new(
        config.agents.clone(),
        store.clone(),
        event_bus.clone(),
        config.clock.clone(),
        config.id_generator.clone(),
        config.notification_policy.clone(),
    ));
    let status = Arc::new(StatusService::new(
        config.agents.clone(),
        config.channels.clone(),
        store.clone(),
        store.clone(),
    ));
    let overview = status.snapshot().await.map_err(map_status_error)?;
    let supervisor = Supervisor::new();
    supervisor.set_runtime_state(RuntimeState::MigrationRequired);
    let (cancel_sender, _cancel_receiver) = watch::channel(false);
    let (outbox_pause, _outbox_pause_receiver) = watch::channel(false);
    let (status_sender, status_receiver) = watch::channel(build_snapshot(
        &config.app_version,
        &config.platform,
        RuntimeState::MigrationRequired,
        overview,
        &supervisor,
        &failure.snapshot,
    ));
    drop(status_sender);

    Ok(RuntimeHandle {
        supervisor,
        event_bus,
        status: status_receiver,
        store,
        ingest,
        cancel_sender,
        outbox_pause,
        tasks: Vec::new(),
        stopped: false,
        host_platform: config.platform.clone(),
        host_version: config.app_version.clone(),
        ingress_pipe_enabled: config.ingress_pipe_enabled,
        migration_required: true,
        runtime_lock: None,
        _telemetry: telemetry,
    })
}

pub struct RuntimeHandle {
    supervisor: Supervisor,
    event_bus: Arc<EventBus>,
    status: watch::Receiver<RuntimeSnapshot>,
    store: Arc<SqliteStore>,
    ingest: Arc<IngestService>,
    cancel_sender: watch::Sender<bool>,
    outbox_pause: watch::Sender<bool>,
    tasks: Vec<JoinHandle<()>>,
    stopped: bool,
    host_platform: String,
    host_version: String,
    ingress_pipe_enabled: bool,
    migration_required: bool,
    runtime_lock: Option<crate::migration::RuntimeLock>,
    _telemetry: Option<TelemetryGuard>,
}

impl RuntimeHandle {
    pub fn is_running(&self) -> bool {
        self.supervisor.is_running() && !self.stopped
    }

    pub fn runtime_state(&self) -> RuntimeState {
        self.supervisor.runtime_state()
    }

    pub fn snapshot(&self) -> RuntimeSnapshot {
        self.status.borrow().clone()
    }

    pub fn subscribe_events(&self) -> tokio::sync::broadcast::Receiver<RuntimeEvent> {
        self.event_bus.subscribe()
    }

    pub fn store(&self) -> Arc<SqliteStore> {
        self.store.clone()
    }

    pub fn outbox_paused(&self) -> bool {
        *self.outbox_pause.borrow()
    }

    /// 只暂停新 Outbox 领取；渠道入站与本地接收仍保持工作。
    pub fn set_outbox_paused(&self, paused: bool) {
        self.outbox_pause.send_replace(paused);
    }

    pub async fn ingest(&self, envelope: AgentEventEnvelope) -> Result<IngestResult, RuntimeError> {
        if self.migration_required {
            return Err(RuntimeError::Migration(Box::new(MigrationFailure {
                code: "legacy_import_failed".into(),
                message: "旧数据迁移未完成，当前运行时拒绝接收新事件".into(),
                snapshot: Box::new(self.snapshot().migration),
            })));
        }
        Ok(self.ingest.ingest(envelope).await?)
    }

    pub async fn shutdown(&mut self) -> Result<(), RuntimeError> {
        if self.stopped {
            return Ok(());
        }
        self.stopped = true;
        self.supervisor.set_runtime_state(RuntimeState::Stopping);
        tracing::info_span!(
            "host",
            platform = %self.host_platform,
            host_version = %self.host_version,
            ipc_status = if self.ingress_pipe_enabled { "enabled" } else { "disabled" },
            task_status = "stopping",
        )
        .in_scope(|| tracing::info!("停止桌面运行时"));
        self.runtime_lock.take();
        let _ = self.cancel_sender.send(true);
        for task in self.tasks.drain(..) {
            let _ = task.await;
        }
        self.store.wal_checkpoint_truncate().await?;
        self.event_bus.runtime_stopped().await;
        self.supervisor.set_runtime_state(RuntimeState::Stopped);
        Ok(())
    }
}
