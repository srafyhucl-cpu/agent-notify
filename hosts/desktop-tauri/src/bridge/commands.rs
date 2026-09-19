use std::sync::Arc;

use tauri::State;

use super::dto::{
    AgentDto, BeginChannelLoginPayload, BeginChannelLoginResultDto, ChannelAccountDto,
    ChannelAccountIdPayload, ChannelListDto, DeliveryDto, DeliveryIdPayload, DiagnosticsDto,
    EmptyPayload, LegacyMigrationDto, LoginSessionDto, MutationAcceptedDto, NotificationDetailDto,
    NotificationFilterPayload, NotificationIdPayload, NotificationListDto, RuntimeSnapshotDto,
    RuntimeSummaryDto, SendTestNotificationPayload, SetRuntimePausedPayload, SettingsDto,
    SubmitChannelLoginCodePayload, TestNotificationResultDto, UpdateAgentConfigPayload,
    UpdateStatusDto,
};
use super::error::CommandError;

/// 桌面命令的唯一业务端口。实现必须只返回脱敏 DTO，并保持命令语义稳定。
#[async_trait::async_trait]
pub trait HostCommandService: Send + Sync {
    async fn get_snapshot(&self, payload: EmptyPayload)
    -> Result<RuntimeSnapshotDto, CommandError>;

    async fn list_agents(&self, payload: EmptyPayload) -> Result<Vec<AgentDto>, CommandError>;

    async fn update_agent_config(
        &self,
        payload: UpdateAgentConfigPayload,
    ) -> Result<AgentDto, CommandError>;

    async fn list_channel_accounts(
        &self,
        payload: EmptyPayload,
    ) -> Result<ChannelListDto, CommandError>;

    async fn begin_channel_login(
        &self,
        payload: BeginChannelLoginPayload,
    ) -> Result<BeginChannelLoginResultDto, CommandError>;

    async fn submit_channel_login_code(
        &self,
        payload: SubmitChannelLoginCodePayload,
    ) -> Result<LoginSessionDto, CommandError>;

    async fn logout_channel_account(
        &self,
        payload: ChannelAccountIdPayload,
    ) -> Result<MutationAcceptedDto, CommandError>;

    async fn enable_channel_account(
        &self,
        payload: ChannelAccountIdPayload,
    ) -> Result<ChannelAccountDto, CommandError>;

    async fn disable_channel_account(
        &self,
        payload: ChannelAccountIdPayload,
    ) -> Result<ChannelAccountDto, CommandError>;

    async fn send_test_notification(
        &self,
        payload: SendTestNotificationPayload,
    ) -> Result<TestNotificationResultDto, CommandError>;

    async fn list_notifications(
        &self,
        payload: NotificationFilterPayload,
    ) -> Result<NotificationListDto, CommandError>;

    async fn get_notification_detail(
        &self,
        payload: NotificationIdPayload,
    ) -> Result<NotificationDetailDto, CommandError>;

    async fn retry_delivery(&self, payload: DeliveryIdPayload)
    -> Result<DeliveryDto, CommandError>;

    async fn get_diagnostics(&self, payload: EmptyPayload) -> Result<DiagnosticsDto, CommandError>;

    async fn retry_legacy_migration(
        &self,
        payload: EmptyPayload,
    ) -> Result<LegacyMigrationDto, CommandError>;

    async fn get_settings(&self, payload: EmptyPayload) -> Result<SettingsDto, CommandError>;

    async fn update_settings(&self, payload: SettingsDto) -> Result<SettingsDto, CommandError>;

    async fn set_runtime_paused(
        &self,
        payload: SetRuntimePausedPayload,
    ) -> Result<RuntimeSummaryDto, CommandError>;

    async fn quit_app(&self, payload: EmptyPayload) -> Result<MutationAcceptedDto, CommandError>;

    async fn get_update_status(
        &self,
        payload: EmptyPayload,
    ) -> Result<UpdateStatusDto, CommandError>;
}

/// Tauri 管理的命令状态；后续宿主任务只负责注入新的服务实现。
#[derive(Clone)]
pub struct BridgeState {
    service: Arc<dyn HostCommandService>,
}

impl BridgeState {
    pub fn new(service: Arc<dyn HostCommandService>) -> Self {
        Self { service }
    }
}

#[tauri::command]
#[specta::specta]
pub async fn get_snapshot(
    state: State<'_, BridgeState>,
    payload: EmptyPayload,
) -> Result<RuntimeSnapshotDto, CommandError> {
    state.service.get_snapshot(payload).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_agents(
    state: State<'_, BridgeState>,
    payload: EmptyPayload,
) -> Result<Vec<AgentDto>, CommandError> {
    state.service.list_agents(payload).await
}

#[tauri::command]
#[specta::specta]
pub async fn update_agent_config(
    state: State<'_, BridgeState>,
    payload: UpdateAgentConfigPayload,
) -> Result<AgentDto, CommandError> {
    state.service.update_agent_config(payload).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_channel_accounts(
    state: State<'_, BridgeState>,
    payload: EmptyPayload,
) -> Result<ChannelListDto, CommandError> {
    state.service.list_channel_accounts(payload).await
}

#[tauri::command]
#[specta::specta]
pub async fn begin_channel_login(
    state: State<'_, BridgeState>,
    payload: BeginChannelLoginPayload,
) -> Result<BeginChannelLoginResultDto, CommandError> {
    state.service.begin_channel_login(payload).await
}

#[tauri::command]
#[specta::specta]
pub async fn submit_channel_login_code(
    state: State<'_, BridgeState>,
    payload: SubmitChannelLoginCodePayload,
) -> Result<LoginSessionDto, CommandError> {
    state.service.submit_channel_login_code(payload).await
}

#[tauri::command]
#[specta::specta]
pub async fn logout_channel_account(
    state: State<'_, BridgeState>,
    payload: ChannelAccountIdPayload,
) -> Result<MutationAcceptedDto, CommandError> {
    state.service.logout_channel_account(payload).await
}

#[tauri::command]
#[specta::specta]
pub async fn enable_channel_account(
    state: State<'_, BridgeState>,
    payload: ChannelAccountIdPayload,
) -> Result<ChannelAccountDto, CommandError> {
    state.service.enable_channel_account(payload).await
}

#[tauri::command]
#[specta::specta]
pub async fn disable_channel_account(
    state: State<'_, BridgeState>,
    payload: ChannelAccountIdPayload,
) -> Result<ChannelAccountDto, CommandError> {
    state.service.disable_channel_account(payload).await
}

#[tauri::command]
#[specta::specta]
pub async fn send_test_notification(
    state: State<'_, BridgeState>,
    payload: SendTestNotificationPayload,
) -> Result<TestNotificationResultDto, CommandError> {
    state.service.send_test_notification(payload).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_notifications(
    state: State<'_, BridgeState>,
    payload: NotificationFilterPayload,
) -> Result<NotificationListDto, CommandError> {
    state.service.list_notifications(payload).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_notification_detail(
    state: State<'_, BridgeState>,
    payload: NotificationIdPayload,
) -> Result<NotificationDetailDto, CommandError> {
    state.service.get_notification_detail(payload).await
}

#[tauri::command]
#[specta::specta]
pub async fn retry_delivery(
    state: State<'_, BridgeState>,
    payload: DeliveryIdPayload,
) -> Result<DeliveryDto, CommandError> {
    state.service.retry_delivery(payload).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_diagnostics(
    state: State<'_, BridgeState>,
    payload: EmptyPayload,
) -> Result<DiagnosticsDto, CommandError> {
    state.service.get_diagnostics(payload).await
}

#[tauri::command]
#[specta::specta]
pub async fn retry_legacy_migration(
    state: State<'_, BridgeState>,
    payload: EmptyPayload,
) -> Result<LegacyMigrationDto, CommandError> {
    state.service.retry_legacy_migration(payload).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_settings(
    state: State<'_, BridgeState>,
    payload: EmptyPayload,
) -> Result<SettingsDto, CommandError> {
    state.service.get_settings(payload).await
}

#[tauri::command]
#[specta::specta]
pub async fn update_settings(
    state: State<'_, BridgeState>,
    payload: SettingsDto,
) -> Result<SettingsDto, CommandError> {
    state.service.update_settings(payload).await
}

#[tauri::command]
#[specta::specta]
pub async fn set_runtime_paused(
    state: State<'_, BridgeState>,
    payload: SetRuntimePausedPayload,
) -> Result<RuntimeSummaryDto, CommandError> {
    state.service.set_runtime_paused(payload).await
}

#[tauri::command]
#[specta::specta]
pub async fn quit_app(
    state: State<'_, BridgeState>,
    payload: EmptyPayload,
) -> Result<MutationAcceptedDto, CommandError> {
    state.service.quit_app(payload).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_update_status(
    state: State<'_, BridgeState>,
    payload: EmptyPayload,
) -> Result<UpdateStatusDto, CommandError> {
    state.service.get_update_status(payload).await
}
