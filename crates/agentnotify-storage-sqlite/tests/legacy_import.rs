use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use agentnotify_application::{
    ChannelAccountStore, ClaimStore, SecretError, SecretKind, SecretStore, SecretValue,
};
use agentnotify_channel_clawbot::{ClawBotAccount, ClawBotContext, ClawBotCredentials};
use agentnotify_domain::{ChannelAccountId, ClaimKey, ClaimState, Timestamp};
use agentnotify_storage_sqlite::{LegacyImport, LegacyPaths, SqliteStore};
use async_trait::async_trait;
use rusqlite::Connection;
use sha2::{Digest, Sha256};

const LEGACY_FILES: [&str; 7] = [
    "config.json",
    "clawbot.json",
    "opencode.off",
    "push.log",
    "reply-routes.jsonl",
    "reply-state.jsonl",
    "codex.off",
];

struct Fixture {
    _temp: tempfile::TempDir,
    importer: LegacyImport,
    store: Arc<SqliteStore>,
    secrets: Arc<TestSecretStore>,
    config_dir: PathBuf,
    temp_dir: PathBuf,
    data_dir: PathBuf,
    db_path: PathBuf,
    source_snapshots: Vec<SourceSnapshot>,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let config_dir = root.join("legacy-config");
        let temp_dir = root.join("legacy-temp");
        let data_dir = root.join("new-data");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::create_dir_all(temp_dir.join("agent-notify")).unwrap();
        copy_fixture("config.json", &config_dir.join("config.json"));
        copy_fixture("clawbot.json", &config_dir.join("clawbot.json"));
        copy_fixture("opencode.off", &config_dir.join("opencode.off"));
        copy_fixture("reply-routes.jsonl", &config_dir.join("reply-routes.jsonl"));
        copy_fixture("reply-state.jsonl", &config_dir.join("reply-state.jsonl"));
        copy_fixture("push.log", &temp_dir.join("agent-notify").join("push.log"));

        let paths = LegacyPaths::new(&config_dir, &temp_dir, &data_dir);
        let source_snapshots = snapshot_sources(&paths);
        let db_path = root.join("state.db");
        let store = Arc::new(SqliteStore::open(&db_path).unwrap());
        let secrets = Arc::new(TestSecretStore::default());
        let importer = LegacyImport::new(paths, store.clone(), secrets.clone());
        Self {
            _temp: temp,
            importer,
            store,
            secrets,
            config_dir,
            temp_dir,
            data_dir,
            db_path,
            source_snapshots,
        }
    }

    fn paths(&self) -> LegacyPaths {
        LegacyPaths::new(&self.config_dir, &self.temp_dir, &self.data_dir)
    }

    async fn legacy_files_unchanged(&self) -> bool {
        let paths = self.paths();
        for expected in &self.source_snapshots {
            let Some(path) = source_path(&paths, expected.name) else {
                return false;
            };
            let Ok(bytes) = std::fs::read(path) else {
                return false;
            };
            if sha256_hex(&bytes) != expected.hash {
                return false;
            }
            let Ok(metadata) = std::fs::metadata(path) else {
                return false;
            };
            if metadata.modified().ok() != expected.modified {
                return false;
            }
        }
        true
    }

    fn notification_count(&self) -> i64 {
        count(&self.db_path, "notifications")
    }

    fn outbox_count(&self) -> i64 {
        count(&self.db_path, "outbox")
    }

    fn setting(&self, key: &str) -> Option<String> {
        Connection::open(&self.db_path)
            .unwrap()
            .query_row(
                "SELECT value_json FROM settings WHERE key = ?1",
                [key],
                |row| row.get(0),
            )
            .ok()
    }
}

#[derive(Clone)]
struct SourceSnapshot {
    name: &'static str,
    hash: String,
    modified: Option<std::time::SystemTime>,
}

#[derive(Default)]
struct TestSecretStore {
    values: Mutex<HashMap<(ChannelAccountId, SecretKind), SecretValue>>,
}

impl TestSecretStore {
    fn get_sync(&self, account_id: &ChannelAccountId, kind: SecretKind) -> Option<SecretValue> {
        self.values
            .lock()
            .unwrap()
            .get(&(account_id.clone(), kind))
            .cloned()
    }
}

#[async_trait]
impl SecretStore for TestSecretStore {
    async fn get(
        &self,
        account_id: &ChannelAccountId,
        kind: SecretKind,
    ) -> Result<SecretValue, SecretError> {
        self.get_sync(account_id, kind)
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

#[tokio::test]
async fn import_is_idempotent_and_preserves_claim_suppression() {
    let fixture = Fixture::new();
    let first = fixture.importer.run().await.unwrap();
    assert!(fixture.setting("legacyImportV1").is_some());
    let second = fixture.importer.run().await.unwrap();

    assert_eq!(first.notifications_imported, 3);
    assert_eq!(first.deliveries_imported, 3);
    assert_eq!(first.routes_imported, 1);
    assert_eq!(first.claims_imported, 3);
    assert_eq!(first.skipped_notifications, 1);
    assert_eq!(first.skipped_routes, 2);
    assert_eq!(first.skipped_claims, 2);
    assert_eq!(second.notifications_imported, 0);
    assert_eq!(fixture.notification_count(), 3);
    assert_eq!(fixture.outbox_count(), 0);

    let account_id = ChannelAccountId::new(first.account_id.clone().unwrap()).unwrap();
    let claim = fixture
        .store
        .find_claim(&ClaimKey::new("legacy-claim-key").unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(claim.state, ClaimState::Unknown);
    assert_eq!(
        fixture
            .store
            .find_claim(&ClaimKey::new("legacy-sent-key").unwrap())
            .await
            .unwrap()
            .unwrap()
            .state,
        ClaimState::Completed
    );
    assert_eq!(
        fixture
            .store
            .find_claim(&ClaimKey::new("legacy-failed-key").unwrap())
            .await
            .unwrap()
            .unwrap()
            .state,
        ClaimState::Failed
    );

    let account = fixture.store.get(&account_id).await.unwrap().unwrap();
    let account = ClawBotAccount::from_channel_account(account).unwrap();
    assert_eq!(
        account.state().session_alert_at,
        Some(Timestamp::parse_rfc3339("2026-09-17T10:30:00Z").unwrap())
    );
    assert_eq!(
        account.state().stale_at,
        Some(Timestamp::parse_rfc3339("2026-09-17T10:00:00Z").unwrap())
    );
    assert_eq!(
        fixture.setting("notification.quietHours").as_deref(),
        Some("\"22-8\"")
    );
    assert_eq!(
        fixture.setting("notification.cooldownMin").as_deref(),
        Some("15")
    );
    assert_eq!(
        fixture.setting("reply.confirmation").as_deref(),
        Some("false")
    );

    let bot_secret = fixture
        .secrets
        .get_sync(&account_id, SecretKind::BotToken)
        .unwrap();
    let bot_credentials = serde_json::from_str::<ClawBotCredentials>(bot_secret.expose()).unwrap();
    assert_eq!(bot_credentials.bot_token(), "bot-token-fixture");
    let context_secret = fixture
        .secrets
        .get_sync(&account_id, SecretKind::ContextToken)
        .unwrap();
    let context = serde_json::from_str::<ClawBotContext>(context_secret.expose()).unwrap();
    assert_eq!(context.context_token(), "context-token-fixture");

    let marker_enabled: i64 = Connection::open(&fixture.db_path)
        .unwrap()
        .query_row(
            "SELECT enabled FROM agent_configs WHERE agent_id = 'opencode'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(marker_enabled, 0);

    let report =
        std::fs::read_to_string(fixture.data_dir.join("legacy-import-report.json")).unwrap();
    assert!(!report.contains("bot-token-fixture"));
    assert!(!report.contains("context-token-fixture"));
    assert!(!report.contains("第一条历史正文"));
    assert!(fixture.legacy_files_unchanged().await);
}

/// 旧版开着的 Agent（没有 marker）迁移后必须保持启用；有 marker 的保持关闭。
/// 这是升级用户不丢通知的关键：宿主补齐逻辑只会在没有迁移结果时写默认关闭行。
#[tokio::test]
async fn import_inherits_legacy_agent_switches() {
    let fixture = Fixture::new();

    let first = fixture.importer.run().await.unwrap();
    assert_eq!(
        first.agent_configs_imported, 5,
        "五个旧版 Agent 都要有开关行"
    );

    let configs = fixture.store.agent_configs().await.unwrap();
    assert_eq!(configs.len(), 5);
    for agent_id in ["codex", "antigravity", "devin", "commandcode"] {
        let record = configs
            .get(agent_id)
            .unwrap_or_else(|| panic!("{agent_id} 必须继承旧版启用状态"));
        assert!(record.enabled, "{agent_id} 旧版没有 marker，必须继承为启用");
        assert_eq!(record.config, serde_json::json!({}), "{agent_id}");
    }
    let opencode = configs.get("opencode").expect("opencode 必须有开关行");
    assert!(!opencode.enabled, "opencode.off 存在，必须保持关闭");

    // 重复运行不重复写开关，也不改结果。
    let second = fixture.importer.run().await.unwrap();
    assert_eq!(second.agent_configs_imported, 0);
    let after = fixture.store.agent_configs().await.unwrap();
    assert!(after["codex"].enabled);
    assert!(!after["opencode"].enabled);
}

/// 旧版用 marker 关掉的 Agent（含新接入的适配器）迁移后必须保持关闭。
#[tokio::test]
async fn import_keeps_marker_agents_disabled() {
    let fixture = Fixture::new();
    std::fs::write(fixture.config_dir.join("codex.off"), b"").unwrap();

    fixture.importer.run().await.unwrap();

    let configs = fixture.store.agent_configs().await.unwrap();
    assert!(!configs["codex"].enabled, "codex.off 存在，必须保持关闭");
    assert!(
        !configs["opencode"].enabled,
        "opencode.off 存在，必须保持关闭"
    );
    for agent_id in ["antigravity", "devin", "commandcode"] {
        assert!(
            configs[agent_id].enabled,
            "{agent_id} 没有 marker，必须继承为启用"
        );
    }
}

#[tokio::test]
async fn malformed_credentials_roll_back_database_and_keep_old_files() {
    let fixture = Fixture::new();
    std::fs::write(
        fixture.config_dir.join("clawbot.json"),
        br#"{"bot_token": "incomplete"}"#,
    )
    .unwrap();

    let error = fixture.importer.run().await.unwrap_err();
    assert_eq!(error.code(), "legacy_credentials_invalid");
    assert_eq!(fixture.notification_count(), 0);
    assert!(fixture.setting("legacyImportV1").is_none());
}

#[tokio::test]
async fn claims_without_bound_account_fail_before_any_import() {
    let fixture = Fixture::new();
    std::fs::remove_file(fixture.config_dir.join("clawbot.json")).unwrap();

    let error = fixture.importer.run().await.unwrap_err();
    assert_eq!(error.code(), "legacy_claim_account_missing");
    assert_eq!(fixture.notification_count(), 0);
    assert!(fixture.setting("legacyImportV1").is_none());
    let account_count: i64 = Connection::open(&fixture.db_path)
        .unwrap()
        .query_row("SELECT COUNT(*) FROM channel_accounts", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(account_count, 0);
}

#[tokio::test]
async fn old_error_text_redacts_imported_tokens() {
    let fixture = Fixture::new();
    fixture.importer.run().await.unwrap();

    let message: String = Connection::open(&fixture.db_path)
        .unwrap()
        .query_row(
            "SELECT error_message FROM deliveries WHERE state = 'Failed' LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(!message.contains("bot-token-fixture"));
    assert!(message.contains("[REDACTED]"));
}

fn copy_fixture(name: &str, destination: &Path) {
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("legacy")
        .join(name);
    std::fs::copy(source, destination).unwrap();
}

fn snapshot_sources(paths: &LegacyPaths) -> Vec<SourceSnapshot> {
    LEGACY_FILES
        .into_iter()
        .filter_map(|name| {
            let path = source_path(paths, name)?;
            if !path.exists() {
                return None;
            }
            let bytes = std::fs::read(path).unwrap();
            let modified = std::fs::metadata(path).unwrap().modified().ok();
            Some(SourceSnapshot {
                name,
                hash: sha256_hex(&bytes),
                modified,
            })
        })
        .collect()
}

fn source_path<'a>(paths: &'a LegacyPaths, name: &str) -> Option<&'a Path> {
    match name {
        "config.json" => Some(&paths.config_file),
        "clawbot.json" => Some(&paths.credential_file),
        "opencode.off" => Some(&paths.opencode_marker),
        "codex.off" => Some(&paths.codex_marker),
        "push.log" => Some(&paths.push_log),
        "reply-routes.jsonl" => Some(&paths.reply_routes),
        "reply-state.jsonl" => Some(&paths.reply_state),
        _ => None,
    }
}

fn count(db_path: &Path, table: &str) -> i64 {
    Connection::open(db_path)
        .unwrap()
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .unwrap()
}

fn sha256_hex(value: &[u8]) -> String {
    let digest = Sha256::digest(value);
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write;
        let _ = write!(encoded, "{byte:02x}");
    }
    encoded
}
