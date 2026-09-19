mod claims;
mod config;
mod credentials;
mod history;
mod report;
mod routes;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{Debug, Display, Formatter};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use agentnotify_application::{SecretError, SecretKind, SecretStore, StoreError};
use agentnotify_channel_sdk::ChannelAccount;
use agentnotify_domain::Timestamp;
use rusqlite::{Transaction, TransactionBehavior, params};

use crate::SqliteStore;
use crate::migrations::storage_error;
use crate::row_codec::{claim_from_row, route_from_row, safe_error_parts, timestamp_to_db};
use crate::sqlite_helpers::query_optional;

use claims::prepare_claims;
use config::{disabled_agent_configs, parse_config};
use credentials::{LegacyClawBotImport, prepare_credentials, sha256_hex, unbound_account};
use history::{LegacyHistoryRecord, prepare_history};
use routes::{LegacyRouteRecord, prepare_routes};

pub use report::{ImportReport, ImportWarning};

const LEGACY_IMPORT_STATUS_KEY: &str = "legacyImportV1";

/// 旧版 Agent-notify 使用的源文件位置。
#[derive(Clone, Debug)]
pub struct LegacyPaths {
    pub config_file: PathBuf,
    pub credential_file: PathBuf,
    pub opencode_marker: PathBuf,
    pub codex_marker: PathBuf,
    pub antigravity_marker: PathBuf,
    pub devin_marker: PathBuf,
    pub commandcode_marker: PathBuf,
    pub push_log: PathBuf,
    pub reply_routes: PathBuf,
    pub reply_state: PathBuf,
    pub report_file: PathBuf,
}

impl LegacyPaths {
    /// 根据旧配置目录、旧临时目录根和新应用数据目录生成默认路径。
    ///
    /// `temp_root` 对应旧版的 `%TEMP%`，旧历史文件位于其下的
    /// `agent-notify/push.log`。
    pub fn new(
        config_dir: impl Into<PathBuf>,
        temp_root: impl Into<PathBuf>,
        data_dir: impl Into<PathBuf>,
    ) -> Self {
        let config_dir = config_dir.into();
        let temp_root = temp_root.into();
        let data_dir = data_dir.into();
        Self {
            config_file: config_dir.join("config.json"),
            credential_file: config_dir.join("clawbot.json"),
            opencode_marker: config_dir.join("opencode.off"),
            codex_marker: config_dir.join("codex.off"),
            antigravity_marker: config_dir.join("antigravity.off"),
            devin_marker: config_dir.join("devin.off"),
            commandcode_marker: config_dir.join("commandcode.off"),
            push_log: temp_root.join("agent-notify").join("push.log"),
            reply_routes: config_dir.join("reply-routes.jsonl"),
            reply_state: config_dir.join("reply-state.jsonl"),
            report_file: data_dir.join("legacy-import-report.json"),
        }
    }

    /// 至少一个旧版源文件存在时才执行首次导入，避免空目录生成无意义状态。
    pub fn has_sources(&self) -> bool {
        self.source_files().iter().any(|(_, path)| path.is_file())
    }

    fn agent_markers(&self) -> [(&'static str, &Path); 5] {
        [
            ("opencode", self.opencode_marker.as_path()),
            ("codex", self.codex_marker.as_path()),
            ("antigravity", self.antigravity_marker.as_path()),
            ("devin", self.devin_marker.as_path()),
            ("commandcode", self.commandcode_marker.as_path()),
        ]
    }

    fn source_files(&self) -> [(&'static str, &Path); 10] {
        [
            ("config.json", self.config_file.as_path()),
            ("clawbot.json", self.credential_file.as_path()),
            ("opencode.off", self.opencode_marker.as_path()),
            ("codex.off", self.codex_marker.as_path()),
            ("antigravity.off", self.antigravity_marker.as_path()),
            ("devin.off", self.devin_marker.as_path()),
            ("commandcode.off", self.commandcode_marker.as_path()),
            ("push.log", self.push_log.as_path()),
            ("reply-routes.jsonl", self.reply_routes.as_path()),
            ("reply-state.jsonl", self.reply_state.as_path()),
        ]
    }
}

/// 读取旧版数据并写入新版 SQLite 和 SecretStore。
#[derive(Clone)]
pub struct LegacyImport {
    paths: LegacyPaths,
    store: Arc<SqliteStore>,
    secrets: Arc<dyn SecretStore>,
}

impl LegacyImport {
    pub fn new(paths: LegacyPaths, store: Arc<SqliteStore>, secrets: Arc<dyn SecretStore>) -> Self {
        Self {
            paths,
            store,
            secrets,
        }
    }

    /// 读取已经持久化的导入报告，不重新读取旧文件或写数据库。
    pub async fn existing_report(&self) -> Result<Option<ImportReport>, LegacyImportError> {
        self.load_existing_report().await
    }

    /// 执行一次只读导入；已有 `legacyImportV1` 状态时直接返回原报告。
    pub async fn run(&self) -> Result<ImportReport, LegacyImportError> {
        if let Some(report) = self.load_existing_report().await? {
            return Ok(report.as_no_change());
        }

        let imported_at = Timestamp::now_utc();
        let imported_at_text = imported_at.to_rfc3339();
        let sources = read_sources(&self.paths)?;
        let source_hashes = sources.hashes();

        let (legacy_config, config_warnings) = parse_config(sources.get("config.json"))?;
        let (credentials, credential_warnings) =
            prepare_credentials(sources.get("clawbot.json"), imported_at)?;
        let primary_account_id = credentials.account_id.as_ref();
        let history_account_id = primary_account_id
            .cloned()
            .unwrap_or_else(|| unbound_account(imported_at).id);
        let (history, history_warnings, history_skipped) = prepare_history(
            sources.get("push.log"),
            &history_account_id,
            &sensitive_values(&credentials),
        )?;
        let (routes, route_warnings, route_skipped) = prepare_routes(
            sources.get("reply-routes.jsonl"),
            primary_account_id,
            imported_at,
        )?;
        let (claims, claim_warnings, claim_skipped) = prepare_claims(
            sources.get("reply-state.jsonl"),
            primary_account_id,
            imported_at,
        )?;

        let mut report = ImportReport::new(imported_at_text, source_hashes);
        report.account_id = credentials.account_id.as_ref().map(ToString::to_string);
        report.widget_agent_mode = legacy_config.widget_agent_mode.clone();
        report.warnings.extend(config_warnings);
        report.warnings.extend(credential_warnings);
        report.warnings.extend(history_warnings);
        report.warnings.extend(route_warnings);
        report.warnings.extend(claim_warnings);
        report.skipped_notifications = history_skipped;
        report.skipped_routes = route_skipped;
        report.skipped_claims = claim_skipped;
        report.skipped_records = history_skipped + route_skipped + claim_skipped;

        for secret in &credentials.secrets {
            self.secrets
                .set(&secret.account_id, secret.kind, secret.value.clone())
                .await
                .map_err(LegacyImportError::secret_failed)?;
            report.secrets_imported += 1;
        }

        let disabled_agents = disabled_agent_configs(&self.paths);
        let report = self
            .store
            .run(move |connection| {
                let transaction = connection
                    .transaction_with_behavior(TransactionBehavior::Immediate)
                    .map_err(|error| storage_error("开启旧数据导入事务失败", error))?;
                let mut report = report;
                let now = timestamp_to_db(imported_at);
                let mut counts = DatabaseCounts::default();

                for setting in legacy_config.settings() {
                    let value = serde_json::to_string(&setting.value)
                        .map_err(|_| StoreError::corrupted("旧设置无法编码"))?;
                    counts.settings_imported += transaction
                        .execute(
                            "INSERT OR IGNORE INTO settings(key, value_json, updated_at) \
                             VALUES (?1, ?2, ?3)",
                            params![setting.key, value, now],
                        )
                        .map_err(|error| storage_error("导入旧设置失败", error))?;
                }
                for agent in disabled_agents {
                    counts.agent_configs_imported += transaction
                        .execute(
                            "INSERT OR IGNORE INTO agent_configs(\
                                agent_id, enabled, config_json, updated_at\
                             ) VALUES (?1, ?2, ?3, ?4)",
                            params![
                                agent.agent_id,
                                agent.enabled,
                                serde_json::to_string(&agent.config)
                                    .map_err(|_| StoreError::corrupted("旧 Agent 配置无法编码"))?,
                                now
                            ],
                        )
                        .map_err(|error| storage_error("导入旧 Agent 开关失败", error))?;
                }

                counts.accounts_imported += insert_accounts(
                    &transaction,
                    credentials.account.as_ref(),
                    &routes,
                    imported_at,
                    &now,
                )?;
                counts.notifications_imported +=
                    insert_notifications(&transaction, &history, &now)?;
                counts.deliveries_imported += insert_deliveries(&transaction, &history, &now)?;
                let route_counts = insert_routes(&transaction, &routes)?;
                counts.routes_imported += route_counts.imported;
                counts.warnings.extend(route_counts.warnings);
                counts.skipped_routes += route_counts.skipped;
                let claim_counts = insert_claims(&transaction, &claims)?;
                counts.claims_imported += claim_counts.imported;
                counts.warnings.extend(claim_counts.warnings);
                counts.skipped_claims += claim_counts.skipped;

                report.settings_imported = counts.settings_imported;
                report.agent_configs_imported = counts.agent_configs_imported;
                report.accounts_imported = counts.accounts_imported;
                report.notifications_imported = counts.notifications_imported;
                report.deliveries_imported = counts.deliveries_imported;
                report.routes_imported = counts.routes_imported;
                report.claims_imported = counts.claims_imported;
                report.skipped_routes += counts.skipped_routes;
                report.skipped_claims += counts.skipped_claims;
                report.skipped_records += counts.skipped_routes + counts.skipped_claims;
                report.warnings.extend(counts.warnings);
                let status_json = serde_json::to_string(&report)
                    .map_err(|_| StoreError::corrupted("迁移报告编码失败"))?;
                transaction
                    .execute(
                        "INSERT INTO settings(key, value_json, updated_at) VALUES (?1, ?2, ?3) \
                         ON CONFLICT(key) DO UPDATE SET \
                            value_json = excluded.value_json, updated_at = excluded.updated_at",
                        params![LEGACY_IMPORT_STATUS_KEY, status_json, now],
                    )
                    .map_err(|error| storage_error("保存旧数据导入状态失败", error))?;

                transaction
                    .commit()
                    .map_err(|error| storage_error("提交旧数据导入事务失败", error))?;
                Ok(report)
            })
            .await
            .map_err(LegacyImportError::store_failed)?;
        report::write_report(&self.paths.report_file, &report)?;
        Ok(report)
    }

    async fn load_existing_report(&self) -> Result<Option<ImportReport>, LegacyImportError> {
        self.store
            .run(|connection| {
                query_optional(
                    connection,
                    "SELECT value_json FROM settings WHERE key = ?1",
                    params![LEGACY_IMPORT_STATUS_KEY],
                    |row| {
                        let value = row
                            .get::<_, String>(0)
                            .map_err(|_| StoreError::corrupted("读取旧数据导入状态失败"))?;
                        serde_json::from_str(&value)
                            .map_err(|_| StoreError::corrupted("旧数据导入状态损坏"))
                    },
                )
            })
            .await
            .map_err(LegacyImportError::store_failed)
    }
}

#[derive(Default)]
struct DatabaseCounts {
    settings_imported: usize,
    agent_configs_imported: usize,
    accounts_imported: usize,
    notifications_imported: usize,
    deliveries_imported: usize,
    routes_imported: usize,
    claims_imported: usize,
    skipped_routes: usize,
    skipped_claims: usize,
    warnings: Vec<ImportWarning>,
}

#[derive(Default)]
struct RowCounts {
    imported: usize,
    skipped: usize,
    warnings: Vec<ImportWarning>,
}

fn insert_accounts(
    transaction: &Transaction<'_>,
    primary: Option<&ChannelAccount>,
    routes: &[LegacyRouteRecord],
    imported_at: Timestamp,
    now: &str,
) -> Result<usize, StoreError> {
    let mut accounts = Vec::new();
    let mut seen = BTreeSet::new();
    if let Some(primary) = primary {
        if seen.insert(primary.id.clone()) {
            accounts.push(primary.clone());
        }
    } else {
        accounts.push(unbound_account(imported_at));
    }
    for route in routes {
        if let Some(account) = &route.additional_account {
            if seen.insert(account.id.clone()) {
                accounts.push(account.clone());
            }
        }
    }

    let mut inserted = 0;
    for account in accounts {
        inserted += transaction
            .execute(
                "INSERT OR IGNORE INTO channel_accounts(\
                    account_id, channel_id, display_name, enabled, config_json, secret_ref, \
                    cursor_json, created_at, updated_at\
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    account.id.as_str(),
                    account.channel_id.as_str(),
                    account.display_name,
                    account.enabled,
                    serde_json::to_string(&account.config)
                        .map_err(|_| StoreError::corrupted("旧渠道账号配置无法编码"))?,
                    account.secret_ref.as_ref().map(|value| value.as_str()),
                    serde_json::to_string(&account.cursor)
                        .map_err(|_| StoreError::corrupted("旧渠道账号游标无法编码"))?,
                    timestamp_to_db(account.created_at),
                    now,
                ],
            )
            .map_err(|error| storage_error("导入旧渠道账号失败", error))?;
    }
    Ok(inserted)
}

fn insert_notifications(
    transaction: &Transaction<'_>,
    history: &[LegacyHistoryRecord],
    now: &str,
) -> Result<usize, StoreError> {
    let mut inserted = 0;
    for record in history {
        let notification = &record.notification;
        inserted += transaction
            .execute(
                "INSERT OR IGNORE INTO notifications(\
                    notification_id, agent_id, ingest_key, session_id, session_title, \
                    title, body, occurred_at, metadata_json, created_at\
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    notification.id.as_str(),
                    notification.agent_id.as_str(),
                    notification.ingest_key,
                    notification.session_id.as_ref().map(|value| value.as_str()),
                    notification.session_title,
                    notification.title,
                    notification.body,
                    timestamp_to_db(notification.occurred_at),
                    serde_json::to_string(&notification.metadata)
                        .map_err(|_| StoreError::corrupted("旧通知元数据无法编码"))?,
                    now,
                ],
            )
            .map_err(|error| storage_error("导入旧通知失败", error))?;
    }
    Ok(inserted)
}

fn insert_deliveries(
    transaction: &Transaction<'_>,
    history: &[LegacyHistoryRecord],
    now: &str,
) -> Result<usize, StoreError> {
    let mut inserted = 0;
    for record in history {
        let delivery = &record.delivery;
        let (error_code, error_message) = safe_error_parts(delivery.error());
        let retryable = false;
        inserted += transaction
            .execute(
                "INSERT OR IGNORE INTO deliveries(\
                    delivery_id, notification_id, channel_id, account_id, state, \
                    external_message_id, error_code, error_message, retryable, attempt_count, \
                    created_at, updated_at\
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)",
                params![
                    delivery.id().as_str(),
                    delivery.notification_id().as_str(),
                    delivery.channel_id().as_str(),
                    delivery.account_id().as_str(),
                    delivery.state().as_str(),
                    delivery.external_message_id().map(|value| value.as_str()),
                    error_code,
                    error_message,
                    retryable,
                    1_i64,
                    now,
                ],
            )
            .map_err(|error| storage_error("导入旧投递记录失败", error))?;
    }
    Ok(inserted)
}

fn insert_routes(
    transaction: &Transaction<'_>,
    routes: &[LegacyRouteRecord],
) -> Result<RowCounts, StoreError> {
    let mut counts = RowCounts::default();
    for item in routes {
        let route = &item.route;
        let existing = query_optional(
            transaction,
            "SELECT channel_id, account_id, external_message_id, agent_id, session_id, \
                    created_at, expires_at FROM reply_routes \
             WHERE channel_id = ?1 AND account_id = ?2 AND external_message_id = ?3",
            params![
                route.key.channel_id.as_str(),
                route.key.account_id.as_str(),
                route.key.external_message_id.as_str()
            ],
            route_from_row,
        )?;
        if let Some(existing) = existing {
            if existing != *route {
                counts.skipped += 1;
                counts.warnings.push(ImportWarning {
                    code: "route_conflict".into(),
                    file: "reply-routes.jsonl".into(),
                    record: None,
                });
            }
            continue;
        }
        transaction
            .execute(
                "INSERT INTO reply_routes(\
                    channel_id, account_id, external_message_id, agent_id, session_id, \
                    created_at, expires_at\
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    route.key.channel_id.as_str(),
                    route.key.account_id.as_str(),
                    route.key.external_message_id.as_str(),
                    route.agent_id.as_str(),
                    route.session_id.as_str(),
                    timestamp_to_db(route.created_at),
                    timestamp_to_db(route.expires_at),
                ],
            )
            .map_err(|error| storage_error("导入旧回复路由失败", error))?;
        counts.imported += 1;
    }
    Ok(counts)
}

fn insert_claims(
    transaction: &Transaction<'_>,
    claims: &[agentnotify_domain::InboundClaim],
) -> Result<RowCounts, StoreError> {
    let mut counts = RowCounts::default();
    for claim in claims {
        let existing = query_optional(
            transaction,
            "SELECT claim_key, channel_id, account_id, external_message_id, state, \
                    received_at, updated_at, expires_at FROM inbound_claims WHERE claim_key = ?1",
            params![claim.key.as_str()],
            claim_from_row,
        )?;
        if let Some(existing) = existing {
            if existing != *claim {
                counts.skipped += 1;
                counts.warnings.push(ImportWarning {
                    code: "claim_conflict".into(),
                    file: "reply-state.jsonl".into(),
                    record: None,
                });
            }
            continue;
        }
        transaction
            .execute(
                "INSERT INTO inbound_claims(\
                    claim_key, channel_id, account_id, external_message_id, state, \
                    error_code, error_message, received_at, updated_at, expires_at\
                ) VALUES (?1, ?2, ?3, ?4, ?5, NULL, NULL, ?6, ?7, ?8)",
                params![
                    claim.key.as_str(),
                    claim.channel_id.as_str(),
                    claim.account_id.as_str(),
                    claim
                        .external_message_id
                        .as_ref()
                        .map(|value| value.as_str()),
                    claim.state.as_str(),
                    timestamp_to_db(claim.received_at),
                    timestamp_to_db(claim.updated_at),
                    timestamp_to_db(claim.expires_at),
                ],
            )
            .map_err(|error| storage_error("导入旧 Claim 失败", error))?;
        counts.imported += 1;
    }
    Ok(counts)
}

fn sensitive_values(credentials: &LegacyClawBotImport) -> Vec<String> {
    let mut values = Vec::new();
    for secret in &credentials.secrets {
        if secret.kind == SecretKind::BotToken {
            if let Ok(credentials) = serde_json::from_str::<
                agentnotify_channel_clawbot::ClawBotCredentials,
            >(secret.value.expose())
            {
                values.push(credentials.bot_token().to_owned());
            }
        }
        if secret.kind == SecretKind::ContextToken {
            if let Ok(context) = serde_json::from_str::<agentnotify_channel_clawbot::ClawBotContext>(
                secret.value.expose(),
            ) {
                values.push(context.context_token().to_owned());
            }
        }
    }
    values
}

struct SourceSet {
    files: Vec<SourceFile>,
}

struct SourceFile {
    name: &'static str,
    bytes: Option<Vec<u8>>,
}

impl SourceSet {
    fn get(&self, name: &str) -> Option<&[u8]> {
        self.files
            .iter()
            .find(|file| file.name == name)
            .and_then(|file| file.bytes.as_deref())
    }

    fn hashes(&self) -> BTreeMap<String, String> {
        self.files
            .iter()
            .filter_map(|file| {
                file.bytes
                    .as_deref()
                    .map(|bytes| (file.name.to_owned(), sha256_hex(bytes)))
            })
            .collect()
    }
}

fn read_sources(paths: &LegacyPaths) -> Result<SourceSet, LegacyImportError> {
    let mut files = Vec::new();
    for (name, path) in paths.source_files() {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(_) => {
                return Err(LegacyImportError::source_read_failed(
                    name,
                    "读取旧版数据文件失败，请确认文件未被其他程序独占",
                ));
            }
        };
        files.push(SourceFile { name, bytes });
    }
    Ok(SourceSet { files })
}

/// 旧版导入失败时提供给 UI 的稳定错误。
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct LegacyImportError {
    code: String,
    message: String,
    file: Option<String>,
    field: Option<String>,
}

impl LegacyImportError {
    pub fn code(&self) -> &str {
        &self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn file(&self) -> Option<&str> {
        self.file.as_deref()
    }

    pub fn field(&self) -> Option<&str> {
        self.field.as_deref()
    }

    pub(crate) fn invalid_field(
        code: impl Into<String>,
        file: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            file: Some(file.into()),
            field: None,
        }
    }

    pub(crate) fn source_read_failed(file: &str, message: &str) -> Self {
        Self {
            code: "legacy_source_read_failed".into(),
            message: message.into(),
            file: Some(file.into()),
            field: None,
        }
    }

    pub(crate) fn claim_account_missing() -> Self {
        Self {
            code: "legacy_claim_account_missing".into(),
            message: "旧版回复状态存在 Claim，但缺少 ClawBot 登录凭据，已停止导入以避免串号".into(),
            file: Some("reply-state.jsonl".into()),
            field: Some("clawbot.json".into()),
        }
    }

    pub(crate) fn store_failed(error: StoreError) -> Self {
        Self {
            code: "legacy_store_failed".into(),
            message: format!("旧版数据写入新数据库失败：{}", error.message()),
            file: None,
            field: None,
        }
    }

    pub(crate) fn secret_failed(error: SecretError) -> Self {
        Self {
            code: "legacy_secret_failed".into(),
            message: format!("旧版 ClawBot 凭据写入系统密钥存储失败：{}", error.message()),
            file: Some("clawbot.json".into()),
            field: None,
        }
    }

    pub(crate) fn report_write(message: &str) -> Self {
        Self {
            code: "legacy_report_write_failed".into(),
            message: message.into(),
            file: Some("legacy-import-report.json".into()),
            field: None,
        }
    }
}

impl Display for LegacyImportError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for LegacyImportError {}
