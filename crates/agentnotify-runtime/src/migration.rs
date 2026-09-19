use std::{
    fs::{File, OpenOptions},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime},
};

use agentnotify_application::SecretStore;
use agentnotify_storage_sqlite::{
    ImportReport, ImportWarning, LegacyImport, LegacyImportError, LegacyPaths, SqliteStore,
};
use fs2::FileExt;

const DEFAULT_ACTIVITY_MAX_AGE: Duration = Duration::from_secs(30);

/// 旧数据导入和单实例锁的启动参数。
pub struct MigrationConfig {
    pub legacy_paths: LegacyPaths,
    pub secrets: Arc<dyn SecretStore>,
    pub runtime_lock_path: PathBuf,
    pub legacy_activity_files: Vec<PathBuf>,
    pub legacy_activity_max_age: Duration,
}

impl MigrationConfig {
    pub fn new(
        legacy_paths: LegacyPaths,
        secrets: Arc<dyn SecretStore>,
        runtime_lock_path: impl Into<PathBuf>,
    ) -> Self {
        Self {
            legacy_paths,
            secrets,
            runtime_lock_path: runtime_lock_path.into(),
            legacy_activity_files: Vec::new(),
            legacy_activity_max_age: DEFAULT_ACTIVITY_MAX_AGE,
        }
    }

    pub fn watch_legacy_activity(mut self, paths: impl IntoIterator<Item = PathBuf>) -> Self {
        self.legacy_activity_files = paths.into_iter().collect();
        self
    }

    pub fn with_legacy_activity_max_age(mut self, max_age: Duration) -> Self {
        self.legacy_activity_max_age = max_age;
        self
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MigrationState {
    NotConfigured,
    NotDetected,
    Completed,
    Partial,
    Required,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationReportSummary {
    pub imported_at: Option<String>,
    pub source_file_count: usize,
    pub settings_imported: usize,
    pub agent_configs_imported: usize,
    pub accounts_imported: usize,
    pub notifications_imported: usize,
    pub deliveries_imported: usize,
    pub routes_imported: usize,
    pub claims_imported: usize,
    pub skipped_records: usize,
    pub warnings: Vec<MigrationWarning>,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationWarning {
    pub code: String,
    pub file: String,
    pub record: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationIssue {
    pub code: String,
    pub message: String,
    pub file: Option<String>,
    pub field: Option<String>,
}

/// 可直接进入 runtime snapshot 的迁移状态，不包含旧正文或密钥。
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationSnapshot {
    pub state: MigrationState,
    pub source_detected: bool,
    pub report_file: Option<String>,
    pub report: Option<MigrationReportSummary>,
    pub error: Option<MigrationIssue>,
}

impl MigrationSnapshot {
    pub fn not_configured() -> Self {
        Self {
            state: MigrationState::NotConfigured,
            source_detected: false,
            report_file: None,
            report: None,
            error: None,
        }
    }

    pub(crate) fn required(
        source_detected: bool,
        report_file: Option<PathBuf>,
        issue: MigrationIssue,
    ) -> Self {
        Self {
            state: MigrationState::Required,
            source_detected,
            report_file: report_file.map(display_path),
            report: None,
            error: Some(issue),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MigrationFailure {
    pub code: String,
    pub message: String,
    pub snapshot: Box<MigrationSnapshot>,
}

impl MigrationFailure {
    fn new(code: &str, message: impl Into<String>, snapshot: MigrationSnapshot) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            snapshot: Box::new(snapshot),
        }
    }
}

impl std::fmt::Display for MigrationFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.message.fmt(formatter)
    }
}

impl std::error::Error for MigrationFailure {}

pub(crate) struct PreparedMigration {
    pub snapshot: MigrationSnapshot,
    pub lock: Option<RuntimeLock>,
}

impl PreparedMigration {
    pub(crate) fn not_configured() -> Self {
        Self {
            snapshot: MigrationSnapshot::not_configured(),
            lock: None,
        }
    }
}

pub(crate) async fn prepare_migration(
    config: Option<&MigrationConfig>,
    store: Arc<SqliteStore>,
) -> Result<PreparedMigration, MigrationFailure> {
    let Some(config) = config else {
        return Ok(PreparedMigration::not_configured());
    };

    let source_detected = config.legacy_paths.has_sources();
    let lock = RuntimeLock::acquire(&config.runtime_lock_path, source_detected)?;
    if let Some(active_path) = active_legacy_activity(config, SystemTime::now()) {
        let issue = MigrationIssue {
            code: "legacy_app_running".into(),
            message: "检测到旧版 Agent-notify 仍在运行，请先从托盘退出旧版后重新检测".into(),
            file: Some(display_path(active_path)),
            field: None,
        };
        let snapshot = MigrationSnapshot::required(
            source_detected,
            Some(config.legacy_paths.report_file.clone()),
            issue,
        );
        return Err(MigrationFailure::new(
            "legacy_app_running",
            "旧版 Agent-notify 仍在运行，当前不会读取或改写旧数据",
            snapshot,
        ));
    }

    let importer = LegacyImport::new(config.legacy_paths.clone(), store, config.secrets.clone());
    let existing = importer.existing_report().await.map_err(|error| {
        migration_import_failure(&error, source_detected, &config.legacy_paths.report_file)
    })?;

    let report = match existing {
        Some(report) => report,
        None if !source_detected => {
            return Ok(PreparedMigration {
                snapshot: MigrationSnapshot {
                    state: MigrationState::NotDetected,
                    source_detected: false,
                    report_file: Some(display_path(&config.legacy_paths.report_file)),
                    report: None,
                    error: None,
                },
                lock: Some(lock),
            });
        }
        None => importer.run().await.map_err(|error| {
            migration_import_failure(&error, source_detected, &config.legacy_paths.report_file)
        })?,
    };

    let state = if report.skipped_records > 0 {
        MigrationState::Partial
    } else if report.source_hashes.is_empty() && !source_detected {
        MigrationState::NotDetected
    } else {
        MigrationState::Completed
    };
    let snapshot = MigrationSnapshot {
        state,
        source_detected,
        report_file: Some(display_path(&config.legacy_paths.report_file)),
        report: Some(report_summary(&report)),
        error: None,
    };
    tracing::info!(
        migration_state = ?state,
        notifications = report.notifications_imported,
        deliveries = report.deliveries_imported,
        routes = report.routes_imported,
        claims = report.claims_imported,
        skipped = report.skipped_records,
        "旧数据迁移检查完成"
    );
    Ok(PreparedMigration {
        snapshot,
        lock: Some(lock),
    })
}

fn migration_import_failure(
    error: &LegacyImportError,
    source_detected: bool,
    report_file: &Path,
) -> MigrationFailure {
    let issue = MigrationIssue {
        code: error.code().into(),
        message: error.message().into(),
        file: error.file().map(ToOwned::to_owned),
        field: error.field().map(ToOwned::to_owned),
    };
    let snapshot =
        MigrationSnapshot::required(source_detected, Some(report_file.to_path_buf()), issue);
    MigrationFailure::new(
        "legacy_import_failed",
        format!("旧版数据导入失败：{}", error.message()),
        snapshot,
    )
}

fn report_summary(report: &ImportReport) -> MigrationReportSummary {
    MigrationReportSummary {
        imported_at: Some(report.imported_at.clone()),
        source_file_count: report.source_hashes.len(),
        settings_imported: report.settings_imported,
        agent_configs_imported: report.agent_configs_imported,
        accounts_imported: report.accounts_imported,
        notifications_imported: report.notifications_imported,
        deliveries_imported: report.deliveries_imported,
        routes_imported: report.routes_imported,
        claims_imported: report.claims_imported,
        skipped_records: report.skipped_records,
        warnings: report.warnings.iter().map(warning_summary).collect(),
    }
}

fn warning_summary(warning: &ImportWarning) -> MigrationWarning {
    MigrationWarning {
        code: warning.code.clone(),
        file: warning.file.clone(),
        record: warning.record,
    }
}

fn active_legacy_activity(config: &MigrationConfig, now: SystemTime) -> Option<PathBuf> {
    let max_age = if config.legacy_activity_max_age.is_zero() {
        DEFAULT_ACTIVITY_MAX_AGE
    } else {
        config.legacy_activity_max_age
    };
    config
        .legacy_activity_files
        .iter()
        .find(|path| path_is_fresh(path, now, max_age))
        .cloned()
}

fn path_is_fresh(path: &Path, now: SystemTime, max_age: Duration) -> bool {
    let Ok(modified) = std::fs::metadata(path).and_then(|metadata| metadata.modified()) else {
        return false;
    };
    match now.duration_since(modified) {
        Ok(age) => age <= max_age,
        Err(_) => true,
    }
}

fn display_path(path: impl AsRef<Path>) -> String {
    path.as_ref().to_string_lossy().into_owned()
}

pub(crate) struct RuntimeLock {
    file: File,
}

impl RuntimeLock {
    fn acquire(path: &Path, source_detected: bool) -> Result<Self, MigrationFailure> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)
            .map_err(|error| {
                runtime_lock_failure(
                    source_detected,
                    "runtime_lock_failed",
                    format!(
                        "无法打开运行时锁 {}：{error}。请检查应用数据目录权限",
                        path.display()
                    ),
                )
            })?;
        FileExt::try_lock_exclusive(&file).map_err(|error| {
            let locked = matches!(
                error.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::PermissionDenied
            ) || error.raw_os_error() == Some(33);
            if locked {
                runtime_lock_failure(
                    source_detected,
                    "runtime_lock_unavailable",
                    "另一个 AgentNotify 实例正在运行，请切换到已有窗口",
                )
            } else {
                runtime_lock_failure(
                    source_detected,
                    "runtime_lock_failed",
                    format!("无法获取运行时锁 {}：{error}", path.display()),
                )
            }
        })?;
        Ok(Self { file })
    }
}

impl Drop for RuntimeLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

fn runtime_lock_failure(
    source_detected: bool,
    code: &str,
    message: impl Into<String>,
) -> MigrationFailure {
    let message = message.into();
    MigrationFailure::new(
        code,
        message.clone(),
        MigrationSnapshot::required(
            source_detected,
            None,
            MigrationIssue {
                code: code.into(),
                message,
                file: None,
                field: None,
            },
        ),
    )
}
