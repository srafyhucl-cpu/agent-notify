use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

const REPORT_VERSION: u32 = 1;

/// 不含正文和密钥的迁移报告，可直接写入数据库状态或用户数据目录。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub version: u32,
    pub imported_at: String,
    pub source_hashes: BTreeMap<String, String>,
    pub account_id: Option<String>,
    pub widget_agent_mode: Option<String>,
    pub settings_imported: usize,
    pub agent_configs_imported: usize,
    pub accounts_imported: usize,
    pub secrets_imported: usize,
    pub notifications_imported: usize,
    pub deliveries_imported: usize,
    pub routes_imported: usize,
    pub claims_imported: usize,
    pub skipped_records: usize,
    pub skipped_notifications: usize,
    pub skipped_routes: usize,
    pub skipped_claims: usize,
    pub warnings: Vec<ImportWarning>,
}

impl ImportReport {
    pub(crate) fn new(imported_at: String, source_hashes: BTreeMap<String, String>) -> Self {
        Self {
            version: REPORT_VERSION,
            imported_at,
            source_hashes,
            account_id: None,
            widget_agent_mode: None,
            settings_imported: 0,
            agent_configs_imported: 0,
            accounts_imported: 0,
            secrets_imported: 0,
            notifications_imported: 0,
            deliveries_imported: 0,
            routes_imported: 0,
            claims_imported: 0,
            skipped_records: 0,
            skipped_notifications: 0,
            skipped_routes: 0,
            skipped_claims: 0,
            warnings: Vec::new(),
        }
    }

    pub(crate) fn as_no_change(&self) -> Self {
        let mut report = self.clone();
        report.settings_imported = 0;
        report.agent_configs_imported = 0;
        report.accounts_imported = 0;
        report.secrets_imported = 0;
        report.notifications_imported = 0;
        report.deliveries_imported = 0;
        report.routes_imported = 0;
        report.claims_imported = 0;
        report.skipped_records = 0;
        report.skipped_notifications = 0;
        report.skipped_routes = 0;
        report.skipped_claims = 0;
        report.warnings.clear();
        report
    }
}

/// 只描述跳过原因和源记录位置，不复制旧消息正文。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportWarning {
    pub code: String,
    pub file: String,
    pub record: Option<u64>,
}

pub(crate) fn write_report(
    path: &Path,
    report: &ImportReport,
) -> Result<(), super::LegacyImportError> {
    let Some(parent) = path.parent() else {
        return Err(super::LegacyImportError::report_write(
            "迁移报告路径缺少父目录",
        ));
    };
    std::fs::create_dir_all(parent).map_err(|_| {
        super::LegacyImportError::report_write("无法创建迁移报告目录，请检查应用数据目录权限")
    })?;

    let data = serde_json::to_vec_pretty(report)
        .map_err(|_| super::LegacyImportError::report_write("迁移报告序列化失败"))?;
    let temporary = path.with_extension("json.tmp");
    let mut file = std::fs::File::create(&temporary).map_err(|_| {
        super::LegacyImportError::report_write("无法创建迁移报告临时文件，请检查磁盘空间")
    })?;
    file.write_all(&data)
        .and_then(|_| file.write_all(b"\n"))
        .and_then(|_| file.sync_all())
        .map_err(|_| super::LegacyImportError::report_write("写入迁移报告失败，请检查磁盘空间"))?;
    drop(file);

    if path.exists() {
        std::fs::remove_file(path).map_err(|_| {
            super::LegacyImportError::report_write("无法替换旧的迁移报告，请稍后重试")
        })?;
    }
    std::fs::rename(&temporary, path).map_err(|_| {
        let _ = std::fs::remove_file(&temporary);
        super::LegacyImportError::report_write("保存迁移报告失败，请检查应用数据目录权限")
    })
}
