use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tokio::sync::{Notify, RwLock};

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
/// 宿主初始化完成前，命令最多等待这么久；超时后仍按"未初始化"明确报错。
const HOST_READY_WAIT_TIMEOUT: Duration = Duration::from_secs(20);

struct UnavailableHostCommandService {
    /// 具体原因：初始化中，或初始化失败的真实错误。
    message: String,
}

impl UnavailableHostCommandService {
    fn not_ready() -> Self {
        Self {
            message: "桌面宿主尚未完成初始化，请稍后重试".to_owned(),
        }
    }

    fn failed(reason: &str) -> Self {
        Self {
            message: format!("桌面宿主初始化失败：{reason}"),
        }
    }
}

#[derive(Clone)]
struct HostCommandServiceSlot {
    inner: Arc<RwLock<Arc<dyn HostCommandService>>>,
    ready: Arc<Notify>,
    initialized: Arc<AtomicBool>,
}

impl HostCommandServiceSlot {
    fn new(service: Arc<dyn HostCommandService>) -> Self {
        Self {
            inner: Arc::new(RwLock::new(service)),
            ready: Arc::new(Notify::new()),
            initialized: Arc::new(AtomicBool::new(false)),
        }
    }

    async fn replace(&self, service: Arc<dyn HostCommandService>) {
        *self.inner.write().await = service;
        self.mark_ready();
    }

    /// 初始化失败：同样标记就绪，让后续命令立即返回具体原因而不是空等。
    async fn fail(&self, service: Arc<dyn HostCommandService>) {
        *self.inner.write().await = service;
        self.mark_ready();
    }

    fn mark_ready(&self) {
        self.initialized.store(true, Ordering::Release);
        self.ready.notify_waiters();
    }

    /// 取当前服务：宿主仍在初始化时**等待**（有上限），避免窗口先于宿主就绪时报"未初始化"。
    async fn current(&self) -> Arc<dyn HostCommandService> {
        let notified = self.ready.notified();
        tokio::pin!(notified);
        if !self.initialized.load(Ordering::Acquire) {
            let _ = tokio::time::timeout(HOST_READY_WAIT_TIMEOUT, &mut notified).await;
        }
        self.inner.read().await.clone()
    }
}

#[derive(Clone)]
pub struct BridgeState {
    service: HostCommandServiceSlot,
}

impl BridgeState {
    pub fn new(service: Arc<dyn HostCommandService>) -> Self {
        Self {
            service: HostCommandServiceSlot::new(service),
        }
    }

    pub fn unavailable() -> Self {
        Self::new(Arc::new(UnavailableHostCommandService::not_ready()))
    }

    pub async fn replace(&self, service: Arc<dyn HostCommandService>) {
        self.service.replace(service).await;
    }

    /// 初始化失败：唤醒等待中的命令并给出具体原因。
    pub async fn fail(&self, reason: &str) {
        self.service
            .fail(Arc::new(UnavailableHostCommandService::failed(reason)))
            .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 宿主初始化前命令必须等待而不是立刻报"未初始化"；初始化完成后立即返回。
    #[tokio::test]
    async fn commands_wait_for_host_initialization_instead_of_failing_fast() {
        let slot =
            HostCommandServiceSlot::new(Arc::new(UnavailableHostCommandService::not_ready()));
        let writer = slot.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(80)).await;
            writer
                .replace(Arc::new(UnavailableHostCommandService::not_ready()))
                .await;
        });

        let started = std::time::Instant::now();
        let _ = slot.current().await;

        assert!(
            started.elapsed() >= Duration::from_millis(60),
            "必须等到初始化完成再返回"
        );
        assert!(
            started.elapsed() < HOST_READY_WAIT_TIMEOUT,
            "不得空等到超时"
        );
    }

    /// 初始化失败时立即返回具体原因，不继续空等。
    #[tokio::test]
    async fn failed_initialization_returns_specific_reason_without_waiting() {
        let slot =
            HostCommandServiceSlot::new(Arc::new(UnavailableHostCommandService::not_ready()));
        slot.fail(Arc::new(UnavailableHostCommandService::failed(
            "数据库损坏",
        )))
        .await;

        let started = std::time::Instant::now();
        let service = slot.current().await;
        assert!(
            started.elapsed() < Duration::from_millis(50),
            "失败后不得继续等待"
        );

        let error = service
            .get_snapshot(EmptyPayload {})
            .await
            .expect_err("必须返回错误");
        assert!(
            error.message().contains("数据库损坏"),
            "错误必须包含具体原因，而不是笼统的稍后重试"
        );
    }
}

#[async_trait::async_trait]
impl HostCommandService for UnavailableHostCommandService {
    async fn get_snapshot(
        &self,
        _payload: EmptyPayload,
    ) -> Result<RuntimeSnapshotDto, CommandError> {
        Err(CommandError::unavailable_message(&self.message))
    }

    async fn list_agents(&self, _payload: EmptyPayload) -> Result<Vec<AgentDto>, CommandError> {
        Err(CommandError::unavailable_message(&self.message))
    }

    async fn update_agent_config(
        &self,
        _payload: UpdateAgentConfigPayload,
    ) -> Result<AgentDto, CommandError> {
        Err(CommandError::unavailable_message(&self.message))
    }

    async fn list_channel_accounts(
        &self,
        _payload: EmptyPayload,
    ) -> Result<ChannelListDto, CommandError> {
        Err(CommandError::unavailable_message(&self.message))
    }

    async fn begin_channel_login(
        &self,
        _payload: BeginChannelLoginPayload,
    ) -> Result<BeginChannelLoginResultDto, CommandError> {
        Err(CommandError::unavailable_message(&self.message))
    }

    async fn submit_channel_login_code(
        &self,
        _payload: SubmitChannelLoginCodePayload,
    ) -> Result<LoginSessionDto, CommandError> {
        Err(CommandError::unavailable_message(&self.message))
    }

    async fn logout_channel_account(
        &self,
        _payload: ChannelAccountIdPayload,
    ) -> Result<MutationAcceptedDto, CommandError> {
        Err(CommandError::unavailable_message(&self.message))
    }

    async fn enable_channel_account(
        &self,
        _payload: ChannelAccountIdPayload,
    ) -> Result<ChannelAccountDto, CommandError> {
        Err(CommandError::unavailable_message(&self.message))
    }

    async fn disable_channel_account(
        &self,
        _payload: ChannelAccountIdPayload,
    ) -> Result<ChannelAccountDto, CommandError> {
        Err(CommandError::unavailable_message(&self.message))
    }

    async fn send_test_notification(
        &self,
        _payload: SendTestNotificationPayload,
    ) -> Result<TestNotificationResultDto, CommandError> {
        Err(CommandError::unavailable_message(&self.message))
    }

    async fn list_notifications(
        &self,
        _payload: NotificationFilterPayload,
    ) -> Result<NotificationListDto, CommandError> {
        Err(CommandError::unavailable_message(&self.message))
    }

    async fn get_notification_detail(
        &self,
        _payload: NotificationIdPayload,
    ) -> Result<NotificationDetailDto, CommandError> {
        Err(CommandError::unavailable_message(&self.message))
    }

    async fn retry_delivery(
        &self,
        _payload: DeliveryIdPayload,
    ) -> Result<DeliveryDto, CommandError> {
        Err(CommandError::unavailable_message(&self.message))
    }

    async fn get_diagnostics(
        &self,
        _payload: EmptyPayload,
    ) -> Result<DiagnosticsDto, CommandError> {
        Err(CommandError::unavailable_message(&self.message))
    }

    async fn retry_legacy_migration(
        &self,
        _payload: EmptyPayload,
    ) -> Result<LegacyMigrationDto, CommandError> {
        Err(CommandError::unavailable_message(&self.message))
    }

    async fn get_settings(&self, _payload: EmptyPayload) -> Result<SettingsDto, CommandError> {
        Err(CommandError::unavailable_message(&self.message))
    }

    async fn update_settings(&self, _payload: SettingsDto) -> Result<SettingsDto, CommandError> {
        Err(CommandError::unavailable_message(&self.message))
    }

    async fn set_runtime_paused(
        &self,
        _payload: SetRuntimePausedPayload,
    ) -> Result<RuntimeSummaryDto, CommandError> {
        Err(CommandError::unavailable_message(&self.message))
    }

    async fn quit_app(&self, _payload: EmptyPayload) -> Result<MutationAcceptedDto, CommandError> {
        Err(CommandError::unavailable_message(&self.message))
    }

    async fn get_update_status(
        &self,
        _payload: EmptyPayload,
    ) -> Result<UpdateStatusDto, CommandError> {
        Err(CommandError::unavailable_message(&self.message))
    }
}

#[tauri::command]
#[specta::specta]
pub async fn get_snapshot(
    state: State<'_, BridgeState>,
    payload: EmptyPayload,
) -> Result<RuntimeSnapshotDto, CommandError> {
    let service = state.service.current().await;
    service.get_snapshot(payload).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_agents(
    state: State<'_, BridgeState>,
    payload: EmptyPayload,
) -> Result<Vec<AgentDto>, CommandError> {
    let service = state.service.current().await;
    service.list_agents(payload).await
}

#[tauri::command]
#[specta::specta]
pub async fn update_agent_config(
    state: State<'_, BridgeState>,
    payload: UpdateAgentConfigPayload,
) -> Result<AgentDto, CommandError> {
    let service = state.service.current().await;
    service.update_agent_config(payload).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_channel_accounts(
    state: State<'_, BridgeState>,
    payload: EmptyPayload,
) -> Result<ChannelListDto, CommandError> {
    let service = state.service.current().await;
    service.list_channel_accounts(payload).await
}

#[tauri::command]
#[specta::specta]
pub async fn begin_channel_login(
    state: State<'_, BridgeState>,
    payload: BeginChannelLoginPayload,
) -> Result<BeginChannelLoginResultDto, CommandError> {
    let service = state.service.current().await;
    service.begin_channel_login(payload).await
}

#[tauri::command]
#[specta::specta]
pub async fn submit_channel_login_code(
    state: State<'_, BridgeState>,
    payload: SubmitChannelLoginCodePayload,
) -> Result<LoginSessionDto, CommandError> {
    let service = state.service.current().await;
    service.submit_channel_login_code(payload).await
}

#[tauri::command]
#[specta::specta]
pub async fn logout_channel_account(
    state: State<'_, BridgeState>,
    payload: ChannelAccountIdPayload,
) -> Result<MutationAcceptedDto, CommandError> {
    let service = state.service.current().await;
    service.logout_channel_account(payload).await
}

#[tauri::command]
#[specta::specta]
pub async fn enable_channel_account(
    state: State<'_, BridgeState>,
    payload: ChannelAccountIdPayload,
) -> Result<ChannelAccountDto, CommandError> {
    let service = state.service.current().await;
    service.enable_channel_account(payload).await
}

#[tauri::command]
#[specta::specta]
pub async fn disable_channel_account(
    state: State<'_, BridgeState>,
    payload: ChannelAccountIdPayload,
) -> Result<ChannelAccountDto, CommandError> {
    let service = state.service.current().await;
    service.disable_channel_account(payload).await
}

#[tauri::command]
#[specta::specta]
pub async fn send_test_notification(
    state: State<'_, BridgeState>,
    payload: SendTestNotificationPayload,
) -> Result<TestNotificationResultDto, CommandError> {
    let service = state.service.current().await;
    service.send_test_notification(payload).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_notifications(
    state: State<'_, BridgeState>,
    payload: NotificationFilterPayload,
) -> Result<NotificationListDto, CommandError> {
    let service = state.service.current().await;
    service.list_notifications(payload).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_notification_detail(
    state: State<'_, BridgeState>,
    payload: NotificationIdPayload,
) -> Result<NotificationDetailDto, CommandError> {
    let service = state.service.current().await;
    service.get_notification_detail(payload).await
}

#[tauri::command]
#[specta::specta]
pub async fn retry_delivery(
    state: State<'_, BridgeState>,
    payload: DeliveryIdPayload,
) -> Result<DeliveryDto, CommandError> {
    let service = state.service.current().await;
    service.retry_delivery(payload).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_diagnostics(
    state: State<'_, BridgeState>,
    payload: EmptyPayload,
) -> Result<DiagnosticsDto, CommandError> {
    let service = state.service.current().await;
    service.get_diagnostics(payload).await
}

#[tauri::command]
#[specta::specta]
pub async fn retry_legacy_migration(
    state: State<'_, BridgeState>,
    payload: EmptyPayload,
) -> Result<LegacyMigrationDto, CommandError> {
    let service = state.service.current().await;
    service.retry_legacy_migration(payload).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_settings(
    state: State<'_, BridgeState>,
    payload: EmptyPayload,
) -> Result<SettingsDto, CommandError> {
    let service = state.service.current().await;
    service.get_settings(payload).await
}

#[tauri::command]
#[specta::specta]
pub async fn update_settings(
    state: State<'_, BridgeState>,
    payload: SettingsDto,
) -> Result<SettingsDto, CommandError> {
    let service = state.service.current().await;
    service.update_settings(payload).await
}

#[tauri::command]
#[specta::specta]
pub async fn set_runtime_paused(
    state: State<'_, BridgeState>,
    payload: SetRuntimePausedPayload,
) -> Result<RuntimeSummaryDto, CommandError> {
    let service = state.service.current().await;
    service.set_runtime_paused(payload).await
}

#[tauri::command]
#[specta::specta]
pub async fn quit_app(
    state: State<'_, BridgeState>,
    payload: EmptyPayload,
) -> Result<MutationAcceptedDto, CommandError> {
    let service = state.service.current().await;
    service.quit_app(payload).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_update_status(
    state: State<'_, BridgeState>,
    payload: EmptyPayload,
) -> Result<UpdateStatusDto, CommandError> {
    let service = state.service.current().await;
    service.get_update_status(payload).await
}
