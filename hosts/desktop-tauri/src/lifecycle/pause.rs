use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use async_trait::async_trait;
use serde_json::Value;

use super::LifecycleError;

const SETTINGS_FILE_NAME: &str = "settings.json";
const NOTIFICATIONS_PAUSED_KEY: &str = "notificationsPaused";

/// Runtime 的暂停端口只允许控制 Outbox 领取，不得提供 ingress/spool 停止能力。
#[async_trait]
pub trait RuntimeControl: Send + Sync {
    fn state(&self) -> super::window::RuntimeState;

    async fn set_outbox_paused(&self, paused: bool) -> Result<(), LifecycleError>;

    async fn shutdown_runtime(&self) -> Result<(), LifecycleError>;
}

#[async_trait]
pub trait PauseSettingsStore: Send + Sync {
    async fn load_paused(&self) -> Result<bool, LifecycleError>;

    async fn save_paused(&self, paused: bool) -> Result<(), LifecycleError>;
}

/// 协调暂停设置与 Outbox 门控。初始化时会恢复持久化状态。
pub struct PauseCoordinator {
    runtime: Arc<dyn RuntimeControl>,
    settings: Arc<dyn PauseSettingsStore>,
    paused: AtomicBool,
}

impl PauseCoordinator {
    pub async fn initialize(
        runtime: Arc<dyn RuntimeControl>,
        settings: Arc<dyn PauseSettingsStore>,
    ) -> Result<Self, LifecycleError> {
        let paused = settings.load_paused().await?;
        if paused {
            runtime.set_outbox_paused(true).await?;
        }
        Ok(Self {
            runtime,
            settings,
            paused: AtomicBool::new(paused),
        })
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Acquire)
    }

    pub async fn set_paused(&self, paused: bool) -> Result<(), LifecycleError> {
        let previous = self.is_paused();
        if previous == paused {
            return Ok(());
        }

        self.settings.save_paused(paused).await?;
        if let Err(error) = self.runtime.set_outbox_paused(paused).await {
            if let Err(rollback_error) = self.settings.save_paused(previous).await {
                return Err(LifecycleError::new(
                    "pause_state_rollback_failed",
                    format!(
                        "{}；恢复原暂停设置也失败：{}",
                        error.message(),
                        rollback_error.message()
                    ),
                ));
            }
            return Err(error);
        }
        self.paused.store(paused, Ordering::Release);
        Ok(())
    }

    pub async fn shutdown_runtime(&self) -> Result<(), LifecycleError> {
        self.runtime.shutdown_runtime().await
    }
}

/// settings.json 的暂停字段适配器，保留其他宿主设置字段。
#[derive(Clone, Debug)]
pub struct JsonPauseSettingsStore {
    path: PathBuf,
}

impl JsonPauseSettingsStore {
    pub fn new(config_dir: impl AsRef<Path>) -> Self {
        Self {
            path: config_dir.as_ref().join(SETTINGS_FILE_NAME),
        }
    }

    fn read_object(&self) -> Result<serde_json::Map<String, Value>, LifecycleError> {
        if !self.path.exists() {
            return Ok(serde_json::Map::new());
        }
        let source = std::fs::read_to_string(&self.path).map_err(|error| {
            LifecycleError::new(
                "pause_settings_read_failed",
                format!("读取暂停设置失败：{error}"),
            )
        })?;
        let value: Value = serde_json::from_str(&source).map_err(|error| {
            LifecycleError::new(
                "pause_settings_invalid",
                format!("暂停设置文件损坏：{error}"),
            )
        })?;
        value.as_object().cloned().ok_or_else(|| {
            LifecycleError::new("pause_settings_invalid", "暂停设置文件必须是 JSON 对象")
        })
    }

    fn write_object(&self, object: &serde_json::Map<String, Value>) -> Result<(), LifecycleError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                LifecycleError::new(
                    "pause_settings_save_failed",
                    format!("创建暂停设置目录失败：{error}"),
                )
            })?;
        }
        let source = serde_json::to_vec_pretty(object).map_err(|error| {
            LifecycleError::new(
                "pause_settings_save_failed",
                format!("序列化暂停设置失败：{error}"),
            )
        })?;
        std::fs::write(&self.path, source).map_err(|error| {
            LifecycleError::new(
                "pause_settings_save_failed",
                format!("保存暂停设置失败：{error}"),
            )
        })
    }
}

#[async_trait]
impl PauseSettingsStore for JsonPauseSettingsStore {
    async fn load_paused(&self) -> Result<bool, LifecycleError> {
        let object = self.read_object()?;
        match object.get(NOTIFICATIONS_PAUSED_KEY) {
            Some(value) => value.as_bool().ok_or_else(|| {
                LifecycleError::new(
                    "pause_settings_invalid",
                    "暂停设置 notificationsPaused 必须是布尔值",
                )
            }),
            None => Ok(false),
        }
    }

    async fn save_paused(&self, paused: bool) -> Result<(), LifecycleError> {
        let mut object = self.read_object()?;
        object.insert(NOTIFICATIONS_PAUSED_KEY.into(), Value::Bool(paused));
        self.write_object(&object)
    }
}
