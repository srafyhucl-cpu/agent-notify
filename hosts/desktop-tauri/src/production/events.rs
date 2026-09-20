use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::Mutex;

use agentnotify_channel_sdk::{LoginSession, LoginSessionState};
use agentnotify_runtime::RuntimeEvent;
use tauri::{AppHandle, Wry};
use tauri_specta::Event;

use super::runtime::ProductionRuntimeCoordinator;
use crate::bridge::dto::{DeliveryStateDto, LoginSessionStateDto};
use crate::bridge::events::{ChannelLoginChangedEvent, DeliveryChangedEvent, SnapshotChangedEvent};

pub struct EventForwarder {
    app: AppHandle<Wry>,
    runtime: Arc<ProductionRuntimeCoordinator>,
    restarted_sessions: Arc<Mutex<HashSet<String>>>,
}

impl EventForwarder {
    pub fn new(app: AppHandle<Wry>, runtime: Arc<ProductionRuntimeCoordinator>) -> Self {
        Self {
            app,
            runtime,
            restarted_sessions: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    pub fn start(&self) {
        let app = self.app.clone();
        let runtime = self.runtime.clone();

        // 1. 订阅 ClawBot 登录会话广播
        let mut login_rx = runtime.login_adapter().subscribe();
        let app_login = app.clone();
        let runtime_login = runtime.clone();
        let restarted_sessions = self.restarted_sessions.clone();

        tauri::async_runtime::spawn(async move {
            while let Ok(session) = login_rx.recv().await {
                let event = map_login_session_event(&session);
                if let Err(error) = event.emit(&app_login) {
                    tracing::error!(%error, "向前端发送 channel.login.changed 事件失败");
                }

                // 当进入 WaitingFirstInbound 且拿到 account_id 时，幂等重建 runtime 启动轮询
                if session.state() == LoginSessionState::WaitingFirstInbound {
                    if let Some(account_id) = session.account_id() {
                        let session_key = format!("{}:{}", session.id().as_str(), account_id);
                        let should_restart = {
                            let set = restarted_sessions.lock().await;
                            !set.contains(&session_key)
                        };
                        if should_restart {
                            tracing::info!(
                                account_id,
                                session_id = %session.id(),
                                "新账号登录成功，重启桌面运行时以建立长轮询"
                            );
                            match runtime_login.start_or_restart().await {
                                Ok(_) => {
                                    restarted_sessions.lock().await.insert(session_key);
                                    let _ = (SnapshotChangedEvent {
                                        reason: "channel_account_added".into(),
                                    })
                                    .emit(&app_login);
                                }
                                Err(error) => {
                                    tracing::error!(%error, "新账号登录后重建运行时失败");
                                }
                            }
                        }
                    }
                }
            }
        });

        // 2. 订阅 Runtime 事件广播
        let app_runtime = app.clone();
        let runtime_events = runtime.clone();
        tauri::async_runtime::spawn(async move {
            // 循环监听，以防 runtime 重启
            loop {
                let receiver = runtime_events.subscribe_events().await;
                let Some(mut rx) = receiver else {
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    continue;
                };

                while let Ok(event) = rx.recv().await {
                    match event {
                        RuntimeEvent::NotificationChanged { .. } => {
                            let _ = (SnapshotChangedEvent {
                                reason: "notification_changed".into(),
                            })
                            .emit(&app_runtime);
                        }
                        RuntimeEvent::ReplyChanged { .. } => {
                            let _ = (SnapshotChangedEvent {
                                reason: "reply_changed".into(),
                            })
                            .emit(&app_runtime);
                        }
                        RuntimeEvent::DeliveryChanged { delivery_id } => {
                            let delivery_dto = DeliveryChangedEvent {
                                delivery_id: delivery_id.to_string(),
                                notification_id: None,
                                state: None,
                            };
                            let _ = delivery_dto.emit(&app_runtime);
                        }
                        RuntimeEvent::RuntimeStopped => {
                            let _ = (SnapshotChangedEvent {
                                reason: "runtime_stopped".into(),
                            })
                            .emit(&app_runtime);
                        }
                    }
                }

                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        });
    }
}

pub fn map_login_session_event(session: &LoginSession) -> ChannelLoginChangedEvent {
    ChannelLoginChangedEvent {
        account_id: session.account_id().map(str::to_owned),
        session_id: Some(session.id().as_str().to_owned()),
        state: map_login_session_state(session.state()),
        message: session.error().map(|e| e.message().to_string()),
    }
}

pub fn map_login_session_state(state: LoginSessionState) -> LoginSessionStateDto {
    match state {
        LoginSessionState::Preparing => LoginSessionStateDto::Preparing,
        LoginSessionState::QrReady => LoginSessionStateDto::QrReady,
        LoginSessionState::WaitingScan => LoginSessionStateDto::WaitingScan,
        LoginSessionState::NeedVerifyCode => LoginSessionStateDto::NeedVerifyCode,
        LoginSessionState::WaitingFirstInbound => LoginSessionStateDto::WaitingFirstInbound,
        LoginSessionState::Paired => LoginSessionStateDto::Paired,
        LoginSessionState::Expired => LoginSessionStateDto::Expired,
        LoginSessionState::Blocked => LoginSessionStateDto::Blocked,
        LoginSessionState::Failed => LoginSessionStateDto::Failed,
    }
}

pub fn map_delivery_state(state: agentnotify_domain::DeliveryState) -> DeliveryStateDto {
    match state {
        agentnotify_domain::DeliveryState::Pending => DeliveryStateDto::Pending,
        agentnotify_domain::DeliveryState::Sent => DeliveryStateDto::Sent,
        agentnotify_domain::DeliveryState::Failed => DeliveryStateDto::Failed,
        agentnotify_domain::DeliveryState::Unknown => DeliveryStateDto::Unknown,
        agentnotify_domain::DeliveryState::Skipped => DeliveryStateDto::Skipped,
    }
}
