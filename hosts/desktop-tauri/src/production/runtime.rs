use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use tauri::{AppHandle, Wry};
use tauri_specta::Event;

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
use crate::bridge::events::SnapshotChangedEvent;
use crate::lifecycle::LifecycleError;
use crate::lifecycle::pause::RuntimeControl;
use crate::lifecycle::window::RuntimeState;
use crate::platform::AppPaths;

const RUNTIME_LOG_FILE_NAME: &str = "runtime.log";

/// 旧版心跳文件（`%TEMP%\agent-notify\widget-alive.txt`）在 runtime 侧有 30 秒的存活判定窗口。
/// 升级安装器会先杀掉旧版进程、几秒后拉起新版，此时心跳往往还新鲜，首启就会落到
/// "迁移诊断模式"——该模式不启动渠道与 ingress，用户表现为"所有推送突然收不到"。
/// 重试间隔取略大于窗口，等心跳过期即可自愈。
const MIGRATION_AUTORETRY_INTERVAL: Duration = Duration::from_secs(35);
/// 自愈重试上限（约 3.5 分钟）。旧版真的还在运行时不做无限重试，
/// 保留诊断模式与界面上的"重新检测"入口，日志给出可操作提示。
const MIGRATION_AUTORETRY_MAX_ATTEMPTS: u32 = 6;
/// runtime 侧"旧版仍在运行"的失败码（`migration.rs` 固定字符串），只有它值得自动重试。
const LEGACY_APP_RUNNING_CODE: &str = "legacy_app_running";

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
    /// 迁移自愈重试是否已在跑，避免重复排程。
    migration_autoretry_inflight: Arc<AtomicBool>,
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
            migration_autoretry_inflight: Arc::new(AtomicBool::new(false)),
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

    /// 迁移诊断模式的自愈重试。
    ///
    /// 升级后首启常见"旧版心跳仍在 30 秒窗口内"，此时运行时会落到诊断模式（不启动渠道与
    /// ingress，用户表现为"推送全断"）。心跳过期后再走一次常规启动即可恢复，这里后台定时重试，
    /// 恢复后发快照事件让界面刷新；其它迁移失败不重试，保持原样暴露。
    pub fn spawn_migration_autoretry(self: &Arc<Self>, app: Option<AppHandle<Wry>>) {
        if self
            .migration_autoretry_inflight
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return;
        }

        let coordinator = self.clone();
        let inflight = self.migration_autoretry_inflight.clone();
        tokio::spawn(async move {
            let outcome = retry_until_ready(
                MIGRATION_AUTORETRY_INTERVAL,
                MIGRATION_AUTORETRY_MAX_ATTEMPTS,
                move || {
                    let coordinator = coordinator.clone();
                    async move {
                        let snapshot = coordinator.retry_legacy_migration().await?;
                        migration_recovered(
                            snapshot.state,
                            snapshot
                                .migration
                                .error
                                .as_ref()
                                .map(|issue| issue.code.as_str()),
                        )
                    }
                },
            )
            .await;

            match outcome {
                MigrationAutoretryOutcome::Recovered { attempts } => {
                    tracing::info!(attempts, "旧版心跳失效后迁移自愈完成，运行时已恢复正常");
                    if let Some(app) = app.as_ref() {
                        let _ = (SnapshotChangedEvent {
                            reason: "migration_recovered".into(),
                        })
                        .emit(app);
                    }
                }
                MigrationAutoretryOutcome::Exhausted => tracing::warn!(
                    attempts = MIGRATION_AUTORETRY_MAX_ATTEMPTS,
                    "旧版进程仍未退出，迁移自愈重试已用尽；请退出旧版后在界面点击“重新检测”，或重启应用"
                ),
                MigrationAutoretryOutcome::Aborted { reason } => {
                    tracing::warn!(%reason, "迁移自愈重试提前停止，保留迁移诊断模式")
                }
            }

            inflight.store(false, Ordering::SeqCst);
        });
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

/// 迁移自愈重试的结果。
#[derive(Debug, Eq, PartialEq)]
enum MigrationAutoretryOutcome {
    Recovered { attempts: u32 },
    Exhausted,
    Aborted { reason: String },
}

/// 每隔 `interval` 探测一次，直到恢复（`Ok(true)`）、用完尝试次数或出现不可自愈失败（`Err`）。
async fn retry_until_ready<F, Fut>(
    interval: Duration,
    max_attempts: u32,
    mut probe: F,
) -> MigrationAutoretryOutcome
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<bool, CommandError>>,
{
    for attempt in 1..=max_attempts {
        tokio::time::sleep(interval).await;
        match probe().await {
            Ok(true) => return MigrationAutoretryOutcome::Recovered { attempts: attempt },
            Ok(false) => continue,
            Err(error) => {
                return MigrationAutoretryOutcome::Aborted {
                    reason: format!("{}：{}", error.code(), error.message()),
                };
            }
        }
    }
    MigrationAutoretryOutcome::Exhausted
}

/// 判断一次重试后的状态：已离开诊断模式即恢复；仍在诊断模式时，只有"旧版仍在运行"可自愈。
fn migration_recovered(
    state: CoreRuntimeState,
    issue_code: Option<&str>,
) -> Result<bool, CommandError> {
    if state != CoreRuntimeState::MigrationRequired {
        return Ok(true);
    }
    match issue_code {
        Some(LEGACY_APP_RUNNING_CODE) => Ok(false),
        Some(code) => Err(CommandError::new(
            "migration_autoretry_unsupported",
            format!("迁移诊断模式的原因不是旧版心跳占用（{code}），不再自动重试"),
        )),
        None => Err(CommandError::new(
            "migration_autoretry_unsupported",
            "迁移诊断模式未给出失败原因，不再自动重试",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn retry_until_ready_recovers_and_stops_probing() {
        let mut calls = 0;
        let outcome = retry_until_ready(Duration::from_millis(1), 5, || {
            calls += 1;
            let current = calls;
            async move { if current == 1 { Ok(false) } else { Ok(true) } }
        })
        .await;

        assert_eq!(
            outcome,
            MigrationAutoretryOutcome::Recovered { attempts: 2 },
            "第二次探测恢复后必须停止重试"
        );
    }

    #[tokio::test]
    async fn retry_until_ready_exhausts_attempts_when_still_blocked() {
        let mut calls = 0;
        let outcome = retry_until_ready(Duration::from_millis(1), 3, || {
            calls += 1;
            async move { Ok(false) }
        })
        .await;

        assert_eq!(outcome, MigrationAutoretryOutcome::Exhausted);
        assert_eq!(calls, 3, "用尽尝试次数后必须停止重试");
    }

    #[tokio::test]
    async fn retry_until_ready_stops_on_unrecoverable_error() {
        let mut calls = 0;
        let outcome = retry_until_ready(Duration::from_millis(1), 4, || {
            calls += 1;
            let current = calls;
            async move {
                if current == 1 {
                    Ok(false)
                } else {
                    Err(CommandError::new("runtime_unavailable", "运行时尚未启动"))
                }
            }
        })
        .await;

        match outcome {
            MigrationAutoretryOutcome::Aborted { reason } => {
                assert!(
                    reason.contains("runtime_unavailable"),
                    "原因要带上错误码：{reason}"
                );
            }
            other => panic!("不可自愈错误必须停止重试，实际得到 {other:?}"),
        }
        assert_eq!(calls, 2, "不可自愈错误后不得继续探测");
    }

    #[test]
    fn migration_recovered_classifies_states() {
        assert!(
            migration_recovered(CoreRuntimeState::Running, None).unwrap_or(false),
            "已恢复常规运行时即视为自愈"
        );
        assert!(
            !migration_recovered(
                CoreRuntimeState::MigrationRequired,
                Some(LEGACY_APP_RUNNING_CODE)
            )
            .unwrap_or(true),
            "旧版仍在运行属于可自愈原因，应继续重试"
        );
        assert!(
            migration_recovered(
                CoreRuntimeState::MigrationRequired,
                Some("legacy_import_failed")
            )
            .is_err(),
            "其它迁移失败原因不得自动重试"
        );
        assert!(
            migration_recovered(CoreRuntimeState::MigrationRequired, None).is_err(),
            "没有失败原因时不得自动重试"
        );
    }
}
