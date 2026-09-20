use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    time::Duration,
};

use agentnotify_agent_sdk::AgentRegistry;
use agentnotify_application::{
    Clock, IdGenerator, NotificationPolicy, ReplyConfig, SecretError, SecretKind, SecretStore,
    SecretValue,
};
use agentnotify_channel_sdk::{
    ChannelAccount, ChannelAdapter, ChannelCapabilities, ChannelDescriptor, ChannelError,
    ChannelHealth, ChannelRegistry, ChannelTask, DeliveryReceipt, InboundEmitter, InboundMode,
    OutboundMessage,
};
use agentnotify_domain::{ChannelAccountId, ChannelId, Timestamp};
use agentnotify_runtime::{
    AppRuntime, MigrationConfig, MigrationState, RuntimeConfig, RuntimeError, RuntimeState,
    start_migration_diagnostics,
};
use agentnotify_storage_sqlite::{LegacyImport, LegacyPaths};
use async_trait::async_trait;
use tempfile::TempDir;
use tokio::sync::watch;

const CONFIG_JSON: &[u8] =
    include_bytes!("../../agentnotify-storage-sqlite/tests/fixtures/legacy/config.json");
const CLAWBOT_JSON: &[u8] =
    include_bytes!("../../agentnotify-storage-sqlite/tests/fixtures/legacy/clawbot.json");
const VALID_PUSH_LOG: &str = r#"{"timestamp":"2026-09-18T10:00:00Z","agent":"opencode","session":"session-1","title":"构建完成","summary":"第一条历史正文","status":"成功","messageID":"wx-message-1","clientID":"client-1"}
{"timestamp":"2026-09-18T11:00:00Z","agent":"opencode","session":"session-1","title":"构建失败","summary":"第二条历史正文","status":"失败","error":"Bearer bot-token-fixture timed out"}
{"timestamp":"2026-09-18T12:00:00Z","agent":"codex","session":"session-2","title":"预览","summary":"第三条历史正文","status":"DryRun"}
"#;

struct FixedClock;

impl Clock for FixedClock {
    fn now(&self) -> Timestamp {
        Timestamp::parse_rfc3339("2026-09-19T10:00:00Z").unwrap()
    }
}

struct SequenceIds(AtomicU64);

impl SequenceIds {
    fn new() -> Self {
        Self(AtomicU64::new(1))
    }
}

impl IdGenerator for SequenceIds {
    fn next_id(&self) -> String {
        format!("generated-{}", self.0.fetch_add(1, Ordering::Relaxed))
    }
}

#[derive(Default)]
struct TestSecrets {
    values: Mutex<HashMap<(ChannelAccountId, SecretKind), SecretValue>>,
}

#[async_trait]
impl SecretStore for TestSecrets {
    async fn get(
        &self,
        account_id: &ChannelAccountId,
        kind: SecretKind,
    ) -> Result<SecretValue, SecretError> {
        self.values
            .lock()
            .unwrap()
            .get(&(account_id.clone(), kind))
            .cloned()
            .ok_or_else(|| SecretError::new("secret_not_found", "测试密钥不存在"))
    }

    async fn set(
        &self,
        account_id: &ChannelAccountId,
        kind: SecretKind,
        value: SecretValue,
    ) -> Result<(), SecretError> {
        self.values
            .lock()
            .unwrap()
            .insert((account_id.clone(), kind), value);
        Ok(())
    }

    async fn delete(
        &self,
        account_id: &ChannelAccountId,
        kind: SecretKind,
    ) -> Result<(), SecretError> {
        self.values
            .lock()
            .unwrap()
            .remove(&(account_id.clone(), kind));
        Ok(())
    }
}

struct CountingChannel {
    id: ChannelId,
    starts: Arc<AtomicUsize>,
}

impl CountingChannel {
    fn new(starts: Arc<AtomicUsize>) -> Self {
        Self {
            id: ChannelId::new("clawbot").unwrap(),
            starts,
        }
    }
}

#[async_trait]
impl ChannelAdapter for CountingChannel {
    fn descriptor(&self) -> ChannelDescriptor {
        ChannelDescriptor {
            id: self.id.clone(),
            display_name: "ClawBot".into(),
            config_schema: serde_json::json!({"type": "object"}),
        }
    }

    fn capabilities(&self) -> ChannelCapabilities {
        ChannelCapabilities {
            send_text: true,
            receive: true,
            reply_routing: true,
            edit_message: false,
            attachments: false,
            markdown: false,
            max_text_bytes: None,
            inbound_modes: vec![InboundMode::LongPolling],
        }
    }

    async fn start(
        &self,
        _account: ChannelAccount,
        _emit: InboundEmitter,
    ) -> Result<ChannelTask, ChannelError> {
        self.starts.fetch_add(1, Ordering::Relaxed);
        let (cancel, mut cancel_receiver) = watch::channel(false);
        let handle = tokio::spawn(async move {
            let _ = cancel_receiver.changed().await;
            Ok(())
        });
        Ok(ChannelTask::new(handle, cancel))
    }

    async fn send(
        &self,
        _account: ChannelAccount,
        _message: OutboundMessage,
    ) -> Result<DeliveryReceipt, ChannelError> {
        Err(ChannelError::unsupported_capability(
            "test_send",
            "测试渠道不发送消息",
        ))
    }

    async fn inspect(&self, _account: ChannelAccount) -> ChannelHealth {
        ChannelHealth::healthy()
    }

    async fn logout(&self, _account: ChannelAccount) -> Result<(), ChannelError> {
        Ok(())
    }
}

struct Fixture {
    _temp: TempDir,
    config_dir: PathBuf,
    temp_dir: PathBuf,
    data_dir: PathBuf,
    database_path: PathBuf,
    lock_path: PathBuf,
    activity_path: PathBuf,
    secrets: Arc<TestSecrets>,
    starts: Arc<AtomicUsize>,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().to_path_buf();
        let config_dir = root.join("legacy-config");
        let temp_dir = root.join("legacy-temp");
        let data_dir = root.join("data");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::create_dir_all(temp_dir.join("agent-notify")).unwrap();
        std::fs::create_dir_all(&data_dir).unwrap();
        write_file(&config_dir.join("config.json"), CONFIG_JSON);
        write_file(&config_dir.join("clawbot.json"), CLAWBOT_JSON);
        write_file(
            &temp_dir.join("agent-notify").join("push.log"),
            VALID_PUSH_LOG.as_bytes(),
        );
        let database_path = data_dir.join("state.db");
        let lock_path = data_dir.join("AgentNotify.runtime.lock");
        let activity_path = temp_dir.join("widget-alive.txt");
        Self {
            _temp: temp,
            config_dir,
            temp_dir,
            data_dir,
            database_path,
            lock_path,
            activity_path,
            secrets: Arc::new(TestSecrets::default()),
            starts: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn paths(&self) -> LegacyPaths {
        LegacyPaths::new(&self.config_dir, &self.temp_dir, &self.data_dir)
    }

    fn runtime_config(&self) -> RuntimeConfig {
        let mut channels = ChannelRegistry::default();
        channels
            .register(Arc::new(CountingChannel::new(self.starts.clone())))
            .unwrap();
        RuntimeConfig {
            database_path: self.database_path.clone(),
            migration: Some(
                MigrationConfig::new(self.paths(), self.secrets.clone(), self.lock_path.clone())
                    .watch_legacy_activity([self.activity_path.clone()]),
            ),
            agents: Arc::new(AgentRegistry::default()),
            channels: Arc::new(channels),
            clock: Arc::new(FixedClock),
            id_generator: Arc::new(SequenceIds::new()),
            notification_policy: NotificationPolicy::default(),
            delivery_targets: Vec::new(),
            reply_targets: Vec::new(),
            reply_config: ReplyConfig::default(),
            target_provider: None,
            app_version: "2.0.0-test".into(),
            platform: "windows".into(),
            ingress_spool_dir: None,
            ingress_pipe_enabled: false,
            telemetry: None,
            inbound_capacity: 16,
            worker_idle_delay: Duration::from_millis(5),
            status_refresh_interval: Duration::from_millis(5),
            channel_poll_interval: Duration::from_millis(5),
        }
    }

    async fn existing_report(&self) -> Option<agentnotify_storage_sqlite::ImportReport> {
        let store =
            Arc::new(agentnotify_storage_sqlite::SqliteStore::open(&self.database_path).unwrap());
        LegacyImport::new(self.paths(), store, self.secrets.clone())
            .existing_report()
            .await
            .unwrap()
    }

    fn source_snapshots(&self) -> Vec<(PathBuf, Vec<u8>, Option<std::time::SystemTime>)> {
        [
            self.config_dir.join("config.json"),
            self.config_dir.join("clawbot.json"),
            self.temp_dir.join("agent-notify").join("push.log"),
        ]
        .into_iter()
        .map(|path| {
            let bytes = std::fs::read(&path).unwrap();
            let modified = std::fs::metadata(&path).unwrap().modified().ok();
            (path, bytes, modified)
        })
        .collect()
    }
}

fn write_file(path: &Path, bytes: &[u8]) {
    std::fs::write(path, bytes).unwrap();
}

fn assert_source_snapshots(expected: &[(PathBuf, Vec<u8>, Option<std::time::SystemTime>)]) {
    for (path, bytes, modified) in expected {
        assert_eq!(std::fs::read(path).unwrap().as_slice(), bytes.as_slice());
        assert_eq!(std::fs::metadata(path).unwrap().modified().ok(), *modified);
    }
}

async fn expect_start_error(config: RuntimeConfig) -> RuntimeError {
    match AppRuntime::start(config).await {
        Err(error) => error,
        Ok(mut runtime) => {
            runtime.shutdown().await.unwrap();
            panic!("运行时应在迁移预检阶段失败");
        }
    }
}

#[tokio::test]
async fn invalid_legacy_credentials_block_write_mode_and_keep_old_files() {
    let fixture = Fixture::new();
    let credential_path = fixture.config_dir.join("clawbot.json");
    write_file(&credential_path, b"{not-json}");
    let old_files = fixture.source_snapshots();

    let error = expect_start_error(fixture.runtime_config()).await;

    assert_eq!(error.code(), "legacy_import_failed");
    assert_eq!(fixture.starts.load(Ordering::Relaxed), 0);
    assert!(fixture.existing_report().await.is_none());
    assert_source_snapshots(&old_files);
}

#[tokio::test]
async fn valid_import_is_idempotent_and_starts_runtime_after_migration() {
    let fixture = Fixture::new();
    let old_files = fixture.source_snapshots();

    let mut first = AppRuntime::start(fixture.runtime_config()).await.unwrap();
    let first_snapshot = first.snapshot();
    assert_eq!(first_snapshot.migration.state, MigrationState::Completed);
    assert_eq!(
        first_snapshot
            .migration
            .report
            .as_ref()
            .unwrap()
            .notifications_imported,
        3
    );
    assert_eq!(first_snapshot.overview.storage.notification_count, 3);
    assert_eq!(first.runtime_state(), RuntimeState::Running);
    first.shutdown().await.unwrap();

    let mut second = AppRuntime::start(fixture.runtime_config()).await.unwrap();
    assert_eq!(second.snapshot().overview.storage.notification_count, 3);
    assert_eq!(second.runtime_state(), RuntimeState::Running);
    second.shutdown().await.unwrap();

    assert_source_snapshots(&old_files);
}

#[tokio::test]
async fn corrupt_history_is_reported_as_partial_without_blocking_runtime() {
    let fixture = Fixture::new();
    let push_log = fixture.temp_dir.join("agent-notify").join("push.log");
    write_file(
        &push_log,
        b"{\"timestamp\":\"2026-09-18T10:00:00Z\",\"agent\":\"opencode\",\"title\":\"ok\",\"summary\":\"ok\",\"status\":\"DryRun\"}\n{bad json}\n",
    );

    let mut runtime = AppRuntime::start(fixture.runtime_config()).await.unwrap();
    let snapshot = runtime.snapshot();

    assert_eq!(snapshot.migration.state, MigrationState::Partial);
    assert_eq!(
        snapshot.migration.report.as_ref().unwrap().skipped_records,
        1
    );
    assert_eq!(runtime.runtime_state(), RuntimeState::Running);
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn active_legacy_widget_blocks_migration_before_channels_start() {
    let fixture = Fixture::new();
    write_file(&fixture.activity_path, b"fresh");

    let error = expect_start_error(fixture.runtime_config()).await;

    assert_eq!(error.code(), "legacy_app_running");
    assert_eq!(fixture.starts.load(Ordering::Relaxed), 0);
    assert!(fixture.existing_report().await.is_none());
}

#[tokio::test]
async fn runtime_lock_rejects_a_second_instance_until_shutdown() {
    let fixture = Fixture::new();
    let mut first = AppRuntime::start(fixture.runtime_config()).await.unwrap();

    let error = expect_start_error(fixture.runtime_config()).await;
    assert_eq!(error.code(), "runtime_lock_unavailable");

    first.shutdown().await.unwrap();
    let mut restarted = AppRuntime::start(fixture.runtime_config()).await.unwrap();
    assert_eq!(
        restarted.snapshot().migration.state,
        MigrationState::Completed
    );
    restarted.shutdown().await.unwrap();
}

#[tokio::test]
async fn migration_diagnostics_never_start_channels_or_outbox() {
    let fixture = Fixture::new();
    write_file(&fixture.config_dir.join("clawbot.json"), b"{not-json}");

    let failure = match AppRuntime::start(fixture.runtime_config()).await {
        Err(RuntimeError::Migration(failure)) => *failure,
        Ok(mut runtime) => {
            runtime.shutdown().await.unwrap();
            panic!("应返回迁移失败，实际启动成功");
        }
        Err(error) => panic!("应返回迁移失败，实际为 {error:?}"),
    };
    let mut diagnostics = start_migration_diagnostics(fixture.runtime_config(), failure)
        .await
        .unwrap();
    let snapshot = diagnostics.snapshot();

    assert_eq!(snapshot.state, RuntimeState::MigrationRequired);
    assert_eq!(snapshot.migration.state, MigrationState::Required);
    assert!(snapshot.components.is_empty());
    assert_eq!(fixture.starts.load(Ordering::Relaxed), 0);
    diagnostics.shutdown().await.unwrap();
}
