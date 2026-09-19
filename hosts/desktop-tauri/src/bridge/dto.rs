use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum BusinessCommand {
    GetSnapshot,
    ListAgents,
    UpdateAgentConfig,
    ListChannelAccounts,
    BeginChannelLogin,
    SubmitChannelLoginCode,
    LogoutChannelAccount,
    EnableChannelAccount,
    DisableChannelAccount,
    SendTestNotification,
    ListNotifications,
    GetNotificationDetail,
    RetryDelivery,
    GetDiagnostics,
    RetryLegacyMigration,
    GetSettings,
    UpdateSettings,
    SetRuntimePaused,
    QuitApp,
    GetUpdateStatus,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
pub enum HostEvent {
    #[serde(rename = "snapshot.changed")]
    SnapshotChanged,
    #[serde(rename = "delivery.changed")]
    DeliveryChanged,
    #[serde(rename = "channel.login.changed")]
    ChannelLoginChanged,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
pub enum RuntimeLifecycleStateDto {
    Starting,
    Running,
    Paused,
    MigrationRequired,
    Stopping,
    Stopped,
    Failed,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
pub enum MigrationStateDto {
    NotConfigured,
    NotDetected,
    Completed,
    Partial,
    Required,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
pub enum DeliveryStateDto {
    Pending,
    Sent,
    Failed,
    Unknown,
    Skipped,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
pub enum LoginSessionStateDto {
    Idle,
    Preparing,
    QrReady,
    WaitingScan,
    NeedVerifyCode,
    WaitingFirstInbound,
    Paired,
    Expired,
    Blocked,
    Failed,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
pub enum DiagnosticLevelDto {
    Normal,
    Waiting,
    Error,
    Paused,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
pub enum ComponentStateDto {
    Starting,
    Running,
    Paused,
    Stopped,
    Failed,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
pub enum UpdateChannelDto {
    Stable,
    Beta,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
pub enum UpdateStateDto {
    UpToDate,
    Available,
    ReadyToInstall,
    Unsupported,
    Failed,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct EmptyPayload {}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SafeErrorDto {
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MigrationWarningDto {
    pub code: String,
    pub file: String,
    pub record: Option<u64>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MigrationReportDto {
    pub imported_at: Option<String>,
    pub source_file_count: u32,
    pub settings_imported: u32,
    pub agent_configs_imported: u32,
    pub accounts_imported: u32,
    pub notifications_imported: u32,
    pub deliveries_imported: u32,
    pub routes_imported: u32,
    pub claims_imported: u32,
    pub skipped_records: u32,
    pub warnings: Vec<MigrationWarningDto>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MigrationIssueDto {
    pub code: String,
    pub message: String,
    pub file: Option<String>,
    pub field: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LegacyMigrationDto {
    pub state: MigrationStateDto,
    pub source_detected: bool,
    pub report_file: Option<String>,
    pub report: Option<MigrationReportDto>,
    pub error: Option<MigrationIssueDto>,
}

impl Default for LegacyMigrationDto {
    fn default() -> Self {
        Self {
            state: MigrationStateDto::NotConfigured,
            source_detected: false,
            report_file: None,
            report: None,
            error: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSummaryDto {
    pub app_version: String,
    pub platform: String,
    pub state: RuntimeLifecycleStateDto,
    pub paused: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct StorageStatusDto {
    pub notification_count: u32,
    pub delivery_count: u32,
    pub pending_outbox_count: u32,
    pub recent_error: Option<SafeErrorDto>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentCapabilitiesDto {
    pub notify: bool,
    pub resume: bool,
    pub session_title: bool,
    pub hook_installer: bool,
    pub reply_window: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentHealthDto {
    pub available: bool,
    pub detail: Option<SafeErrorDto>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentDto {
    pub id: String,
    pub display_name: String,
    pub description: String,
    #[specta(type = specta_typescript::Unknown)]
    pub config_schema: serde_json::Value,
    pub capabilities: AgentCapabilitiesDto,
    pub enabled: bool,
    #[specta(type = specta_typescript::Unknown)]
    pub config: serde_json::Value,
    pub health: AgentHealthDto,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ChannelCapabilitiesDto {
    pub send_text: bool,
    pub receive: bool,
    pub reply_routing: bool,
    pub edit_message: bool,
    pub attachments: bool,
    pub markdown: bool,
    pub max_text_bytes: Option<u32>,
    pub inbound_modes: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ChannelHealthDto {
    pub available: bool,
    pub stale: bool,
    pub detail: Option<SafeErrorDto>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ChannelAccountDto {
    pub id: String,
    pub channel_id: String,
    pub display_name: String,
    pub enabled: bool,
    #[specta(type = specta_typescript::Unknown)]
    pub config: serde_json::Value,
    pub health: ChannelHealthDto,
    pub last_inbound_at: Option<String>,
    pub last_delivery_at: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ChannelDto {
    pub id: String,
    pub display_name: String,
    #[specta(type = specta_typescript::Unknown)]
    pub config_schema: serde_json::Value,
    pub capabilities: ChannelCapabilitiesDto,
    pub accounts: Vec<ChannelAccountDto>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ChannelListDto {
    pub channels: Vec<ChannelDto>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryDto {
    pub id: String,
    pub notification_id: String,
    pub channel_id: String,
    pub account_id: String,
    pub state: DeliveryStateDto,
    pub external_message_id: Option<String>,
    pub error: Option<SafeErrorDto>,
    pub retryable: bool,
    pub updated_at: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NotificationSummaryDto {
    pub id: String,
    pub agent_id: String,
    pub session_id: Option<String>,
    pub session_title: Option<String>,
    pub title: String,
    pub preview: String,
    pub occurred_at: String,
    pub delivery_states: Vec<DeliveryStateDto>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NotificationDetailDto {
    pub notification: NotificationSummaryDto,
    pub body: String,
    pub metadata: BTreeMap<String, String>,
    pub deliveries: Vec<DeliveryDto>,
    pub route_exists: bool,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NotificationListDto {
    pub items: Vec<NotificationSummaryDto>,
    pub total: u32,
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ComponentDto {
    pub name: String,
    pub state: ComponentStateDto,
    pub detail: Option<SafeErrorDto>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticActionDto {
    pub label: String,
    pub command: BusinessCommand,
    #[specta(type = specta_typescript::Unknown)]
    pub payload: serde_json::Value,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticItemDto {
    pub code: String,
    pub level: DiagnosticLevelDto,
    pub message: String,
    pub checked_at: String,
    pub action: Option<DiagnosticActionDto>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotOverviewDto {
    pub storage: StorageStatusDto,
    pub agents: Vec<AgentDto>,
    pub channels: Vec<ChannelAccountDto>,
    pub recent_deliveries: Vec<DeliveryDto>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSnapshotDto {
    pub runtime: RuntimeSummaryDto,
    pub overview: SnapshotOverviewDto,
    pub components: Vec<ComponentDto>,
    pub diagnostics: Vec<DiagnosticItemDto>,
    pub migration: LegacyMigrationDto,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsDto {
    pub generated_at: String,
    pub runtime: RuntimeSummaryDto,
    pub storage: StorageStatusDto,
    pub components: Vec<ComponentDto>,
    pub items: Vec<DiagnosticItemDto>,
    pub migration: LegacyMigrationDto,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct QuietHoursDto {
    pub enabled: bool,
    pub start: String,
    pub end: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SettingsDto {
    pub notifications_paused: bool,
    pub quiet_hours: Option<QuietHoursDto>,
    pub cooldown_seconds: u32,
    pub default_channel_account_id: Option<String>,
    pub reply_enabled: bool,
    pub delivery_receipt_enabled: bool,
    pub route_ttl_seconds: u32,
    pub auto_start: bool,
    pub start_hidden: bool,
    pub update_channel: UpdateChannelDto,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatusDto {
    pub current_version: String,
    pub available_version: Option<String>,
    pub state: UpdateStateDto,
    pub signed: bool,
    pub preview: bool,
    pub message: String,
    pub checked_at: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAgentConfigPayload {
    pub agent_id: String,
    pub enabled: Option<bool>,
    #[specta(type = Option<specta_typescript::Unknown>)]
    pub config: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ChannelAccountIdPayload {
    pub account_id: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BeginChannelLoginPayload {
    pub channel_id: String,
    pub account_key: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SubmitChannelLoginCodePayload {
    pub session_id: String,
    pub code: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SendTestNotificationPayload {
    pub account_id: String,
    pub title: String,
    pub body: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NotificationFilterPayload {
    pub agent_id: Option<String>,
    pub channel_id: Option<String>,
    pub account_id: Option<String>,
    pub delivery_state: Option<DeliveryStateDto>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub query: Option<String>,
    pub cursor: Option<String>,
    pub limit: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NotificationIdPayload {
    pub notification_id: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryIdPayload {
    pub delivery_id: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SetRuntimePausedPayload {
    pub paused: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MutationAcceptedDto {
    pub accepted: bool,
    pub id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BeginChannelLoginResultDto {
    pub channel_id: String,
    pub session: LoginSessionDto,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LoginSessionDto {
    pub id: String,
    pub account_id: Option<String>,
    pub account_key: String,
    pub state: LoginSessionStateDto,
    pub qr_payload: Option<String>,
    pub created_at: String,
    pub message: Option<String>,
    pub error: Option<SafeErrorDto>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TestNotificationResultDto {
    pub accepted: bool,
    pub delivery: Option<DeliveryDto>,
}
