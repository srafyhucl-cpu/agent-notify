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
    InstallUpdate,
    CreateOrcTask,
    ListOrcTasks,
    AdvanceOrcTask,
    MarkBlockedOrcTask,
    RecoverBlockedOrcTask,
    StartOrcTask,
    GetCurrentOrcWorkflow,
    ListOrcTemplates,
    SaveOrcTemplateConfig,
    ListOpencodeProjects,
    ListOpencodeModels,
    UpdateOrcTask,
    DeleteOrcTask,
    ContinueOrcTask,
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

/// 编排任务状态（稳定字符串，与 A2A `TaskState` 一一对应，§8.3 映射表）。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum OrcTaskStateDto {
    Unspecified,
    Submitted,
    Working,
    Completed,
    Failed,
    Canceled,
    InputRequired,
    Rejected,
    AuthRequired,
}

/// 编排消息 kind（推进命令用，§4 消息总线）。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum OrcMessageKindDto {
    Report,
    Instruction,
    Confirm,
    Question,
    Info,
}

/// 编排任务视图：只暴露脱敏后的任务上下文（§3.2 TASK）。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OrcTaskDto {
    pub id: String,
    pub workflow_id: String,
    pub state: OrcTaskStateDto,
    pub current_step: u32,
    /// 是否已开始执行（创建后默认 false；人工确认「开始执行」后置 true 并派活第 1 步）。
    pub started: bool,
    /// 该任务所属工作流的节点列表（供详情页展示每步角色与派给谁）。
    pub workflow: OrcWorkflowDto,
    /// 被卡住的步骤（未阻塞为 None）
    pub blocked_step: Option<u32>,
    /// 阻塞原因（未阻塞为 None；写清哪步失败/谁不可用/未送达）
    pub block_reason: Option<String>,
    /// 通知节奏：final_only / verbose（§4.6）
    pub notify_mode: String,
    pub goal: String,
    /// 任务名称（短名 ≤8 字；用于集群列表与真实会话标题；旧任务按目标前 8 字推导）。
    pub name: String,
    /// 迭代轮次（从 1 起；「继续迭代」后 +1）。
    pub round: u32,
    /// 本轮要求/上一轮结论（第 1 轮为 None）。
    pub round_input: Option<String>,
    /// 任务创建时间（RFC3339；旧任务为 None，界面不显示）。
    pub created_at: Option<String>,
    /// 轮次时间线（每轮要求 + 结论摘要；旧任务为空，UI 按任务描述合成第 1 轮）。
    pub round_history: Vec<OrcRoundRecordDto>,
    /// 任务工作目录（OpenCode 会话创建位置）；None = 旧任务，跟随宿主当前项目。
    pub working_dir: Option<String>,
    /// 是否处于「项目经理汇总阶段」（最后一步完成、等待首节点汇总，§4）。
    pub finalizing: bool,
}

/// 单轮迭代记录（轮次时间线）：本轮要求 + 本轮结论摘要。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OrcRoundRecordDto {
    /// 轮次（从 1 起）
    pub round: u32,
    /// 本轮要求（第 1 轮/未填写为 None = 界面按任务描述或留空呈现）
    pub input: Option<String>,
    /// 本轮结论摘要（本轮未结束为 None）
    pub summary: Option<String>,
}

/// 编排工作流视图（预置工作流或其用户配置）：节点列表供 UI 预览「每步做什么、派给谁」。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OrcWorkflowDto {
    pub id: String,
    pub name: String,
    pub steps: Vec<OrcWorkflowStepDto>,
}

/// 工作流单个节点：角色、建议 Agent 与人工确认门。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OrcWorkflowStepDto {
    pub order: u32,
    /// 角色（orchestrator / planner / executor / reviewer）——中文说明由前端映射。
    pub role: String,
    /// 建议 Agent（可为空：留空时该步不派活，仅等待人工推进）。
    pub agent_hint: Option<String>,
    /// 该步使用的模型（`provider/model`；None = 未指定，由该 Agent 自己决定）。
    pub model: Option<String>,
    /// 是否需人确认才进入下一步。
    pub human_gate: bool,
}

/// 固定工作流模板视图（设置页节点配置与创建任务预览共用）：节点含合并后的 Agent/模型。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OrcTemplateDto {
    pub id: String,
    pub name: String,
    pub steps: Vec<OrcTemplateStepDto>,
}

/// 模板单个节点：角色 + 合并后的 Agent/模型（未配置为 None）。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OrcTemplateStepDto {
    pub order: u32,
    pub role: String,
    pub agent: Option<String>,
    pub model: Option<String>,
}

/// 保存某模板的节点配置（覆盖式：提交全量节点，空 Agent/模型 = 清除该节点覆盖）。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SaveOrcTemplateConfigPayload {
    pub template_id: String,
    pub steps: Vec<OrcTemplateStepConfigDto>,
}

/// 单节点配置输入：`order` 必须与模板一致；agent/model 均可空。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OrcTemplateStepConfigDto {
    pub order: u32,
    pub agent: Option<String>,
    pub model: Option<String>,
}

/// OpenCode 已知项目（工作目录下拉数据源，§3.1）：只读本地库的 project 表。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OpencodeProjectDto {
    /// 项目工作目录（绝对路径）。
    pub directory: String,
    /// 项目名（库中为空则为 None）。
    pub name: Option<String>,
    /// 最近活跃时间（库中原始整数时间戳；缺失为 None）。
    pub last_active_at: Option<i64>,
}

/// OpenCode 可用模型（模型下拉数据源）：`provider_id`/`model_id` 拼成 `provider/model`。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OpencodeModelDto {
    /// 提供方 id（如 `opencode-go`）。
    pub provider_id: String,
    /// 模型 id（如 `deepseek-v4.1-flash`）。
    pub model_id: String,
    /// 显示名（OpenCode 界面里的模型名，如 `DeepSeek V4.1 Flash`）。
    pub name: String,
}

/// 当前编排工作流（供创建任务前预览节点）。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CurrentOrcWorkflowDto {
    pub workflow: OrcWorkflowDto,
}

/// 创建编排任务（§3.1）：`template_id` 必选（任务锁定模板）；`working_dir` 必填（必须是已存在目录）；
/// `notify_mode` 缺省为 final_only（只推最终汇报，默认）；`steps` 为任务级节点配置（创建即锁定）。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CreateOrcTaskPayload {
    pub goal: String,
    /// 任务名称（必填、≤8 字；只用于集群列表与真实会话标题展示）。None = 旧调用方，按目标前 8 字推导。
    pub name: Option<String>,
    /// 工作流模板 id（内置三档模板之一；旧预设仅兼容已存在任务，不再对新任务开放）。
    pub template_id: String,
    /// 任务工作目录（OpenCode 会话创建位置；必须是已存在的目录）。
    pub working_dir: String,
    pub notify_mode: Option<String>,
    /// 任务级节点配置（启动器弹窗逐个节点确认）：order 必须与模板一致、每个节点都有 Agent；
    /// 提交后**创建即锁定**为该任务的步骤快照（此后改设置不影响），不需要再在设置页配置。
    /// None = 旧调用方：保持原行为（start 时按当时设置实时合并并锁定）。
    pub steps: Option<Vec<OrcTemplateStepConfigDto>>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OrcTaskIdPayload {
    pub task_id: String,
}

/// 更新编排任务（集群页「编辑」）：名称随时可改；描述仅未开始任务可改；通知节奏随时可改。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UpdateOrcTaskPayload {
    pub task_id: String,
    pub name: Option<String>,
    pub goal: Option<String>,
    pub notify_mode: Option<String>,
}

/// 继续迭代（集群页「继续迭代」）：本轮结束后开始新一轮（轮次 +1、回到第 1 步）。
/// `instruction` = 本轮要求（用户填写；留空则交给项目经理按上一轮结论继续）。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ContinueOrcTaskPayload {
    pub task_id: String,
    pub instruction: Option<String>,
}

/// 推进编排任务：消息驱动（§4.4），`kind` 决定转移语义。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AdvanceOrcTaskPayload {
    pub task_id: String,
    pub kind: OrcMessageKindDto,
}

/// 标记编排任务阻塞（§4.6 不自动重推）：`step` 为失败步骤，`reason` 写清谁不可用/未送达。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MarkBlockedOrcTaskPayload {
    pub task_id: String,
    pub step: u32,
    pub reason: String,
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
    /// 编排功能开关（`orchestration.enabled`，默认关闭；开启后重启用）。
    #[serde(default)]
    pub orchestration_enabled: bool,
    /// 编排会话无人值守（`orchestration.unattended`，默认 true）：编排会话权限 ask 自动放行。
    #[serde(default = "default_orchestration_unattended")]
    pub orchestration_unattended: bool,
}

/// `orchestration_unattended` 的默认值：缺失 = 无人值守开启（§6）。
fn default_orchestration_unattended() -> bool {
    true
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

/// 一键升级命令无输入参数：目标版本来自最近一次检查更新。
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct InstallUpdatePayload {}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct InstallUpdateResultDto {
    pub state: UpdateStateDto,
    pub message: String,
    /// 已下载并校验、准备就绪的版本号；失败时为 None。
    pub installed_version: Option<String>,
    /// 产物是否带通过校验的签名（预览通道允许未签名）。
    pub signed: bool,
    /// 是否走测试通道安装（不强制签名指纹）。
    pub preview: bool,
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
