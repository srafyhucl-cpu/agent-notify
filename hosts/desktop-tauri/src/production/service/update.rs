//! 更新命令域（`HostCommandService` 的 `update` 片段 + 更新相关辅助，2026-09 从 service.rs 拆分）。

use super::*;
use crate::bridge::commands::UpdateCommands;

#[async_trait::async_trait]
impl UpdateCommands for ProductionHostCommandService {
    async fn get_update_status(
        &self,
        _payload: EmptyPayload,
    ) -> Result<UpdateStatusDto, CommandError> {
        let current_version = self.current_app_version().await;
        let channel = self.update_channel().await;
        let preview = channel.is_preview();
        let checked_at = Some(Timestamp::now_utc().to_rfc3339());

        Ok(
            match self.updates.check_latest(&current_version, channel).await {
                Ok(Some(release)) => UpdateStatusDto {
                    current_version,
                    available_version: Some(release.version.clone()),
                    state: UpdateStateDto::Available,
                    signed: false,
                    preview,
                    message: format!("发现新版本 v{}，可下载并安装。", release.version),
                    checked_at,
                },
                Ok(None) => UpdateStatusDto {
                    current_version,
                    available_version: None,
                    state: UpdateStateDto::UpToDate,
                    signed: false,
                    preview,
                    message: "当前已是最新版本。".into(),
                    checked_at,
                },
                Err(error) => UpdateStatusDto {
                    current_version,
                    available_version: None,
                    state: update_state_for_error(&error),
                    signed: false,
                    preview,
                    message: error.message().to_owned(),
                    checked_at,
                },
            },
        )
    }

    async fn install_update(
        &self,
        _payload: InstallUpdatePayload,
    ) -> Result<InstallUpdateResultDto, CommandError> {
        let current_version = self.current_app_version().await;
        let channel = self.update_channel().await;
        let preview = channel.is_preview();
        let install_root = current_install_root()?;
        let launcher = SystemInstallerLauncher;
        let exit = ProductionAppExitRequester::new(
            self.app.clone(),
            self.runtime.clone(),
            self.store.clone(),
        );

        Ok(
            match self
                .updates
                .install_latest(&current_version, channel, &install_root, &launcher, &exit)
                .await
            {
                Ok(report) => install_result_from_report(report),
                // 安装失败不抛异常：用 DTO 的 Failed 状态把中文原因交给界面展示。
                Err(error) => InstallUpdateResultDto {
                    state: update_state_for_error(&error),
                    message: error.message().to_owned(),
                    installed_version: None,
                    signed: false,
                    preview,
                },
            },
        )
    }
}

impl ProductionHostCommandService {
    async fn current_app_version(&self) -> String {
        match self.runtime.current_snapshot().await {
            Some(snapshot) => snapshot.app_version,
            None => env!("CARGO_PKG_VERSION").to_string(),
        }
    }

    /// 更新通道来自设置；读取失败按正式通道处理（更保守，必须校验签名指纹）。
    async fn update_channel(&self) -> UpdateChannel {
        match self.settings.load_settings().await {
            Ok(settings) => match settings.update_channel {
                UpdateChannelDto::Stable => UpdateChannel::Stable,
                UpdateChannelDto::Beta => UpdateChannel::Beta,
            },
            Err(_) => UpdateChannel::Stable,
        }
    }
}

/// 更新包要落回的安装目录就是当前程序所在目录（安装器路径用 /DIR= 锁定同一位置）。
fn current_install_root() -> Result<PathBuf, CommandError> {
    let executable = std::env::current_exe().map_err(|error| {
        CommandError::new(
            "update_install_dir_missing",
            format!("无法确定当前程序位置，无法安装更新：{error}"),
        )
    })?;
    executable
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| {
            CommandError::new(
                "update_install_dir_missing",
                format!("无法确定安装目录：{}", executable.display()),
            )
        })
}
