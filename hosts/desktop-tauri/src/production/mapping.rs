//! 领域记录 → bridge DTO 的纯映射与脱敏：不含 IO 与业务策略，
//! 只做形状转换和敏感字段剔除，便于逐个核对界面暴露了什么。

use agentnotify_storage_sqlite::{DeliveryViewRecord, NotificationRecord};

use super::events::{map_delivery_state, map_login_session_state};
use crate::bridge::dto::*;
use crate::update::{InstallReport, UpdateError};

/// 把更新错误码映射到界面状态：版本号不可比是 Unsupported，已是最新是 UpToDate，其余是 Failed。
pub(super) fn update_state_for_error(error: &UpdateError) -> UpdateStateDto {
    match error.code() {
        "update_version_unsupported" => UpdateStateDto::Unsupported,
        "update_up_to_date" => UpdateStateDto::UpToDate,
        _ => UpdateStateDto::Failed,
    }
}

pub(super) fn install_result_from_report(report: InstallReport) -> InstallUpdateResultDto {
    InstallUpdateResultDto {
        // 安装已就绪：安装器启动中或文件已替换，重启/安装完成后生效。
        state: UpdateStateDto::ReadyToInstall,
        message: report.message,
        installed_version: Some(report.version),
        signed: report.signed,
        preview: report.preview,
    }
}

pub(super) fn sanitize_account_config(config: &serde_json::Value) -> serde_json::Value {
    if let serde_json::Value::Object(map) = config {
        let mut safe_map = serde_json::Map::new();
        for (k, v) in map {
            if !k.to_lowercase().contains("token")
                && !k.to_lowercase().contains("secret")
                && !k.to_lowercase().contains("password")
            {
                safe_map.insert(k.clone(), v.clone());
            }
        }
        serde_json::Value::Object(safe_map)
    } else {
        config.clone()
    }
}

pub(super) fn map_login_session_dto(
    session: &agentnotify_channel_sdk::LoginSession,
) -> LoginSessionDto {
    LoginSessionDto {
        id: session.id().as_str().to_string(),
        account_id: session.account_id().map(str::to_owned),
        account_key: session.account_key().to_string(),
        state: map_login_session_state(session.state()),
        qr_payload: session.qr_payload().map(str::to_owned),
        created_at: session.created_at().to_rfc3339(),
        message: session.error().map(|e| e.message().to_string()),
        error: session.error().map(|e| SafeErrorDto {
            code: e.code().to_string(),
            message: e.message().to_string(),
        }),
    }
}

pub(super) fn map_delivery_view_record(record: DeliveryViewRecord) -> DeliveryDto {
    DeliveryDto {
        id: record.delivery.id().to_string(),
        notification_id: record.delivery.notification_id().to_string(),
        channel_id: record.delivery.channel_id().to_string(),
        account_id: record.delivery.account_id().to_string(),
        state: map_delivery_state(record.delivery.state()),
        external_message_id: record.delivery.external_message_id().map(|m| m.to_string()),
        error: record.delivery.error().map(|e| SafeErrorDto {
            code: e.code().to_string(),
            message: e.message().to_string(),
        }),
        retryable: record.delivery.can_retry(),
        updated_at: record.updated_at.to_rfc3339(),
    }
}

pub(super) fn map_notification_record(record: NotificationRecord) -> NotificationSummaryDto {
    NotificationSummaryDto {
        id: record.notification.id.to_string(),
        agent_id: record.notification.agent_id.to_string(),
        session_id: record.notification.session_id.map(|s| s.to_string()),
        session_title: record.notification.session_title,
        title: record.notification.title,
        preview: record.notification.body.chars().take(100).collect(),
        occurred_at: record.notification.occurred_at.to_rfc3339(),
        delivery_states: record
            .delivery_states
            .into_iter()
            .map(map_delivery_state)
            .collect(),
    }
}

pub(super) fn map_migration_snapshot(
    snapshot: &agentnotify_runtime::MigrationSnapshot,
) -> LegacyMigrationDto {
    LegacyMigrationDto {
        state: match snapshot.state {
            agentnotify_runtime::MigrationState::NotConfigured => MigrationStateDto::NotConfigured,
            agentnotify_runtime::MigrationState::NotDetected => MigrationStateDto::NotDetected,
            agentnotify_runtime::MigrationState::Completed => MigrationStateDto::Completed,
            agentnotify_runtime::MigrationState::Partial => MigrationStateDto::Partial,
            agentnotify_runtime::MigrationState::Required => MigrationStateDto::Required,
        },
        source_detected: snapshot.source_detected,
        report_file: snapshot.report_file.clone(),
        report: snapshot.report.as_ref().map(|r| MigrationReportDto {
            imported_at: r.imported_at.clone(),
            source_file_count: r.source_file_count as u32,
            settings_imported: r.settings_imported as u32,
            agent_configs_imported: r.agent_configs_imported as u32,
            accounts_imported: r.accounts_imported as u32,
            notifications_imported: r.notifications_imported as u32,
            deliveries_imported: r.deliveries_imported as u32,
            routes_imported: r.routes_imported as u32,
            claims_imported: r.claims_imported as u32,
            skipped_records: r.skipped_records as u32,
            warnings: r
                .warnings
                .iter()
                .map(|w| MigrationWarningDto {
                    code: w.code.clone(),
                    file: w.file.clone(),
                    record: w.record,
                })
                .collect(),
        }),
        error: snapshot.error.as_ref().map(|f| MigrationIssueDto {
            code: f.code.clone(),
            message: f.message.clone(),
            file: f.file.clone(),
            field: f.field.clone(),
        }),
    }
}
