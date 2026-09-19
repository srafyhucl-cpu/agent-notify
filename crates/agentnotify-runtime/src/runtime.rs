use std::{path::PathBuf, sync::Arc, time::Duration};

use agentnotify_agent_sdk::{AgentEventEnvelope, AgentRegistry};
use agentnotify_application::{
    ChannelAccountStore, Clock, DeliveryError, DeliveryService, DeliveryTarget, EventSink,
    IdGenerator, IngestError, IngestResult, IngestService, NotificationPolicy, ReplyConfig,
    ReplyError, ReplyService, ReplyTarget, StatusError, StatusOverview, StatusService, StatusStore,
    StoreError,
};
use agentnotify_channel_sdk::{ChannelAccount, ChannelError, ChannelRegistry, InboundEmitter};
use agentnotify_domain::{InboundMessage, SafeError};
use agentnotify_storage_sqlite::SqliteStore;
use tokio::{
    sync::{mpsc, watch},
    task::JoinHandle,
};

use crate::{
    ComponentFailure, ComponentSnapshot, ComponentState, EventBus, RuntimeEvent, RuntimeState,
    Supervisor,
};
use crate::{TelemetryConfig, TelemetryError, TelemetryGuard, init_telemetry};

use crate::migration::{
    MigrationConfig, MigrationFailure, MigrationSnapshot, MigrationState, prepare_migration,
};

const DEFAULT_INBOUND_CAPACITY: usize = 256;
const DEFAULT_WORKER_IDLE_DELAY: Duration = Duration::from_millis(250);
const DEFAULT_STATUS_REFRESH_INTERVAL: Duration = Duration::from_secs(2);
const DEFAULT_CHANNEL_POLL_INTERVAL: Duration = Duration::from_millis(100);

struct SnapshotMetadata {
    app_version: String,
    platform: String,
    migration: Arc<MigrationSnapshot>,
}

/// 运行时装配参数。所有适配器必须先在注册表中显式注册。
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuntimeError {
    Store(StoreError),
    Reply(ReplyError),
    IngressSpool {
        code: &'static str,
        message: &'static str,
    },
    Telemetry(TelemetryError),
    InvalidConfiguration {
        field: &'static str,
    },
    IntegrityCheckFailed,
    Ingest(IngestError),
    Migration(Box<MigrationFailure>),
}

impl RuntimeError {
    pub fn code(&self) -> &str {
        match self {
            Self::Store(error) => error.code(),
            Self::Reply(error) => error.code(),
            Self::IngressSpool { code, .. } => code,
            Self::Telemetry(error) => error.code(),
            Self::InvalidConfiguration { field } => field,
            Self::IntegrityCheckFailed => "database_integrity_failed",
            Self::Ingest(error) => error.code(),
            Self::Migration(error) => error.code.as_str(),
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Self::Store(error) => error.message(),
            Self::Reply(error) => error.message(),
            Self::IngressSpool { message, .. } => message,
            Self::Telemetry(error) => error.message(),
            Self::InvalidConfiguration { .. } => "运行时配置无效",
            Self::IntegrityCheckFailed => "数据库完整性检查失败，运行时未启动",
            Self::Ingest(error) => error.message(),
            Self::Migration(error) => error.message.as_str(),
        }
    }
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.message())
    }
}

impl std::error::Error for RuntimeError {}

impl From<StoreError> for RuntimeError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}

impl From<ReplyError> for RuntimeError {
    fn from(value: ReplyError) -> Self {
        Self::Reply(value)
    }
}

impl From<IngestError> for RuntimeError {
    fn from(value: IngestError) -> Self {
        Self::Ingest(value)
    }
}

impl From<agentnotify_ingress::SpoolError> for RuntimeError {
    fn from(value: agentnotify_ingress::SpoolError) -> Self {
        Self::IngressSpool {
            code: value.code(),
            message: value.message(),
        }
    }
}

impl From<TelemetryError> for RuntimeError {
    fn from(value: TelemetryError) -> Self {
        Self::Telemetry(value)
    }
}

impl From<MigrationFailure> for RuntimeError {
    fn from(value: MigrationFailure) -> Self {
        Self::Migration(Box::new(value))
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
            ipc_status = "disabled",
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
        let ingest = Arc::new(IngestService::new(
            config.agents.clone(),
            store.clone(),
            event_bus.clone(),
            config.clock.clone(),
            config.id_generator.clone(),
            config.notification_policy.clone(),
        ));
        let delivery = Arc::new(DeliveryService::new(
            store.clone(),
            config.channels.clone(),
            config.delivery_targets.clone(),
            config.clock.clone(),
            config.id_generator.clone(),
            event_bus.clone(),
            Default::default(),
        ));
        let reply = Arc::new(ReplyService::new(
            store.clone(),
            store.clone(),
            config.channels.clone(),
            config.agents.clone(),
            config.clock.clone(),
            event_bus.clone(),
            config.reply_targets.clone(),
            config.reply_config.clone(),
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
            run_outbox_worker(delivery, delivery_cancel, config.worker_idle_delay()),
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
            tasks,
            stopped: false,
            host_platform: config.platform.clone(),
            host_version: config.app_version.clone(),
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
        tasks: Vec::new(),
        stopped: false,
        host_platform: config.platform.clone(),
        host_version: config.app_version.clone(),
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
    tasks: Vec<JoinHandle<()>>,
    stopped: bool,
    host_platform: String,
    host_version: String,
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
            ipc_status = "disabled",
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

async fn enabled_accounts(
    store: Arc<SqliteStore>,
    channels: Arc<ChannelRegistry>,
) -> Result<
    Vec<(
        Arc<dyn agentnotify_channel_sdk::ChannelAdapter>,
        ChannelAccount,
    )>,
    RuntimeError,
> {
    let account_store: Arc<dyn ChannelAccountStore> = store;
    let mut accounts = Vec::new();
    for adapter in channels.all() {
        let descriptor = adapter.descriptor();
        for account in account_store.list(&descriptor.id).await? {
            if account.enabled {
                accounts.push((adapter.clone(), account));
            }
        }
    }
    Ok(accounts)
}

async fn run_channel(
    name: String,
    adapter: Arc<dyn agentnotify_channel_sdk::ChannelAdapter>,
    account: ChannelAccount,
    emit: InboundEmitter,
    mut cancel: watch::Receiver<bool>,
    poll_interval: Duration,
) -> Result<(), ComponentFailure> {
    let task = adapter
        .start(account, emit)
        .await
        .map_err(|error| channel_failure(error, &name))?;
    loop {
        if *cancel.borrow() {
            return task
                .shutdown()
                .await
                .map_err(|error| channel_failure(error, &name));
        }
        tokio::select! {
            changed = cancel.changed() => {
                if changed.is_ok() {
                    return task.shutdown().await.map_err(|error| channel_failure(error, &name));
                }
                return Ok(());
            }
            _ = tokio::time::sleep(poll_interval) => {
                if task.is_finished() {
                    return task.shutdown().await.map_err(|error| channel_failure(error, &name));
                }
            }
        }
    }
}

async fn run_inbound_consumer(
    mut receiver: mpsc::Receiver<InboundMessage>,
    reply: Arc<ReplyService>,
    mut cancel: watch::Receiver<bool>,
) -> Result<(), ComponentFailure> {
    loop {
        if *cancel.borrow() {
            return Ok(());
        }
        tokio::select! {
            message = receiver.recv() => {
                let Some(message) = message else {
                    return Ok(());
                };
                if let Err(error) = reply.handle(message).await {
                    if matches!(&error, ReplyError::Store(store_error) if store_error_is_fatal(store_error)) {
                        return Err(ComponentFailure::new(
                            error.code(),
                            error.message(),
                            true,
                        ));
                    }
                    tracing::warn!(code = error.code(), "处理入站回复失败");
                }
            }
            changed = cancel.changed() => {
                if changed.is_err() || *cancel.borrow() {
                    return Ok(());
                }
            }
        }
    }
}

async fn run_outbox_worker(
    delivery: Arc<DeliveryService>,
    mut cancel: watch::Receiver<bool>,
    idle_delay: Duration,
) -> Result<(), ComponentFailure> {
    loop {
        if *cancel.borrow() {
            return Ok(());
        }
        match delivery.process_next().await {
            Ok(_) => {}
            Err(error) => {
                if delivery_error_is_fatal(&error) {
                    return Err(ComponentFailure::new(error.code(), error.message(), true));
                }
                tracing::warn!(code = error.code(), "Outbox worker 处理失败");
                tokio::select! {
                    changed = cancel.changed() => {
                        if changed.is_err() || *cancel.borrow() {
                            return Ok(());
                        }
                    }
                    _ = tokio::time::sleep(idle_delay) => {}
                }
                continue;
            }
        }
        tokio::select! {
            changed = cancel.changed() => {
                if changed.is_err() || *cancel.borrow() {
                    return Ok(());
                }
            }
            _ = tokio::time::sleep(idle_delay) => {}
        }
    }
}

async fn run_status_refresher(
    status: Arc<StatusService>,
    supervisor: Supervisor,
    metadata: SnapshotMetadata,
    sender: watch::Sender<RuntimeSnapshot>,
    mut cancel: watch::Receiver<bool>,
    interval: Duration,
) -> Result<(), ComponentFailure> {
    loop {
        if *cancel.borrow() {
            return Ok(());
        }
        match status.snapshot().await {
            Ok(overview) => {
                let state = supervisor.runtime_state();
                let _ = sender.send(build_snapshot(
                    &metadata.app_version,
                    &metadata.platform,
                    state,
                    overview,
                    &supervisor,
                    &metadata.migration,
                ));
            }
            Err(error) => {
                if status_error_is_fatal(&error) {
                    return Err(ComponentFailure::new(error.code(), error.message(), true));
                }
                tracing::warn!(code = error.code(), "刷新运行状态失败");
            }
        }
        tokio::select! {
            changed = cancel.changed() => {
                if changed.is_err() || *cancel.borrow() {
                    return Ok(());
                }
            }
            _ = tokio::time::sleep(interval) => {}
        }
    }
}

async fn run_fatal_monitor(
    supervisor: Supervisor,
    cancel: watch::Receiver<bool>,
    cancel_sender: watch::Sender<bool>,
) -> Result<(), ComponentFailure> {
    let mut fatal = supervisor.subscribe_fatal_error();
    let mut cancel = cancel;
    loop {
        if *cancel.borrow() {
            return Ok(());
        }
        tokio::select! {
            changed = fatal.changed() => {
                if changed.is_err() {
                    return Ok(());
                }
                if fatal.borrow().is_some() {
                    let _ = cancel_sender.send(true);
                    return Ok(());
                }
            }
            changed = cancel.changed() => {
                if changed.is_err() {
                    return Ok(());
                }
            }
        }
    }
}

fn build_snapshot(
    app_version: &str,
    platform: &str,
    state: RuntimeState,
    overview: StatusOverview,
    supervisor: &Supervisor,
    migration: &MigrationSnapshot,
) -> RuntimeSnapshot {
    let components = supervisor.components();
    let failed_components = components
        .iter()
        .filter(|component| component.state == ComponentState::Failed)
        .count();
    let diagnostics = vec![
        DiagnosticItem {
            code: "storage".into(),
            level: if overview.storage.recent_error.is_some() {
                DiagnosticLevel::Warning
            } else {
                DiagnosticLevel::Ok
            },
            message: if overview.storage.recent_error.is_some() {
                "数据库可读写，但最近存在一次失败记录".into()
            } else {
                "数据库可读写".into()
            },
        },
        DiagnosticItem {
            code: "components".into(),
            level: if failed_components == 0 {
                DiagnosticLevel::Ok
            } else {
                DiagnosticLevel::Warning
            },
            message: if failed_components == 0 {
                "后台组件运行正常".into()
            } else {
                "部分后台组件已停止，其他组件仍在运行".into()
            },
        },
        migration_diagnostic(migration),
    ];
    RuntimeSnapshot {
        app_version: app_version.into(),
        platform: platform.into(),
        state,
        overview,
        components,
        diagnostics,
        migration: migration.clone(),
    }
}

fn migration_diagnostic(migration: &MigrationSnapshot) -> DiagnosticItem {
    let (level, message) = match migration.state {
        MigrationState::NotConfigured => (DiagnosticLevel::Ok, "旧数据迁移未配置".into()),
        MigrationState::NotDetected => (DiagnosticLevel::Ok, "未发现旧版 Agent-notify 数据".into()),
        MigrationState::Completed => (DiagnosticLevel::Ok, "旧数据迁移已完成".into()),
        MigrationState::Partial => (
            DiagnosticLevel::Warning,
            format!(
                "旧数据已导入，跳过 {} 条损坏记录",
                migration
                    .report
                    .as_ref()
                    .map(|report| report.skipped_records)
                    .unwrap_or_default()
            ),
        ),
        MigrationState::Required => (
            DiagnosticLevel::Error,
            migration
                .error
                .as_ref()
                .map(|error| error.message.clone())
                .unwrap_or_else(|| "旧数据迁移未完成，当前处于只读诊断模式".into()),
        ),
    };
    DiagnosticItem {
        code: "legacy-migration".into(),
        level,
        message,
    }
}

fn channel_failure(error: ChannelError, _name: &str) -> ComponentFailure {
    ComponentFailure::new(error.code(), error.message(), false)
}

fn delivery_error_is_fatal(error: &DeliveryError) -> bool {
    matches!(error, DeliveryError::Store(store_error) if store_error_is_fatal(store_error))
}

fn status_error_is_fatal(error: &StatusError) -> bool {
    matches!(error, StatusError::Store(store_error) if store_error_is_fatal(store_error))
}

fn store_error_is_fatal(error: &StoreError) -> bool {
    matches!(
        error.code(),
        "sqlite_error" | "store_corrupted" | "store_unavailable" | "migration_checksum_mismatch"
    )
}

fn map_status_error(error: StatusError) -> RuntimeError {
    match error {
        StatusError::Store(error) => RuntimeError::Store(error),
    }
}
