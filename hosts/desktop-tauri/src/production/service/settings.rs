//! 设置与应用控制命令域（`HostCommandService` 的 `settings` 片段，2026-09 从 service.rs 拆分）。

use super::*;
use crate::bridge::commands::SettingsCommands;

#[async_trait::async_trait]
impl SettingsCommands for ProductionHostCommandService {
    async fn get_settings(&self, _payload: EmptyPayload) -> Result<SettingsDto, CommandError> {
        let mut settings = self.settings.load_settings().await?;
        settings.notifications_paused = self.runtime.is_outbox_paused().await;
        Ok(settings)
    }

    async fn update_settings(&self, payload: SettingsDto) -> Result<SettingsDto, CommandError> {
        let current = self.settings.load_settings().await?;

        // 1. 若当前用户自启动发生变化，调用系统自启动服务
        if let Some(app) = &self.app {
            if current.auto_start != payload.auto_start {
                let autostart = TauriCurrentUserAutostart::new(app.clone());
                set_autostart(&autostart, payload.auto_start)
                    .map_err(|e| CommandError::new(e.code(), e.to_string()))?;
            }
        }

        self.settings.save_settings(&payload).await?;
        self.runtime
            .set_outbox_paused(payload.notifications_paused)
            .await?;

        // 2. 若暂停状态变更，同步托盘菜单与事件
        if let Some(app) = &self.app {
            if current.notifications_paused != payload.notifications_paused {
                let _ = sync_tray_paused(app, payload.notifications_paused);
            }
        }

        // 3. 仅在关键运行时配置发生变化时重启 runtime
        let needs_runtime_restart = current.reply_enabled != payload.reply_enabled
            || current.delivery_receipt_enabled != payload.delivery_receipt_enabled
            || current.route_ttl_seconds != payload.route_ttl_seconds
            || current.quiet_hours != payload.quiet_hours
            || current.cooldown_seconds != payload.cooldown_seconds
            || current.default_channel_account_id != payload.default_channel_account_id;

        if needs_runtime_restart {
            self.runtime.start_or_restart().await?;
        }

        self.get_settings(EmptyPayload {}).await
    }

    async fn set_runtime_paused(
        &self,
        payload: SetRuntimePausedPayload,
    ) -> Result<RuntimeSummaryDto, CommandError> {
        let current_paused = self.runtime.is_outbox_paused().await;
        if current_paused == payload.paused {
            let snapshot = match self.runtime.current_snapshot().await {
                Some(s) => s,
                None => self.runtime.start_or_restart().await?,
            };
            return Ok(RuntimeSummaryDto {
                app_version: snapshot.app_version,
                platform: snapshot.platform,
                state: if payload.paused {
                    RuntimeLifecycleStateDto::Paused
                } else {
                    RuntimeLifecycleStateDto::Running
                },
                paused: payload.paused,
            });
        }

        // 先持久化到 settings 表
        self.settings
            .save_settings(&SettingsDto {
                notifications_paused: payload.paused,
                ..self.settings.load_settings().await?
            })
            .await?;

        // 再控制 Outbox 门控；若失败，尝试回滚设置
        if let Err(error) = self.runtime.set_outbox_paused(payload.paused).await {
            let _ = self
                .settings
                .save_settings(&SettingsDto {
                    notifications_paused: current_paused,
                    ..self.settings.load_settings().await?
                })
                .await;
            return Err(error);
        }

        // 同步托盘菜单与托盘事件
        if let Some(app) = &self.app {
            let _ = sync_tray_paused(app, payload.paused);
        }

        let snapshot = match self.runtime.current_snapshot().await {
            Some(s) => s,
            None => self.runtime.start_or_restart().await?,
        };

        Ok(RuntimeSummaryDto {
            app_version: snapshot.app_version,
            platform: snapshot.platform,
            state: if payload.paused {
                RuntimeLifecycleStateDto::Paused
            } else {
                RuntimeLifecycleStateDto::Running
            },
            paused: payload.paused,
        })
    }

    async fn quit_app(&self, _payload: EmptyPayload) -> Result<MutationAcceptedDto, CommandError> {
        if let Some(app) = &self.app {
            spawn_graceful_exit(
                app.clone(),
                self.runtime.clone(),
                self.store.clone(),
                Duration::ZERO,
            );
        } else {
            let _ = self.runtime.shutdown_runtime().await;
            let _ = self.store.wal_checkpoint_truncate().await;
        }

        Ok(MutationAcceptedDto {
            accepted: true,
            id: None,
        })
    }
}
