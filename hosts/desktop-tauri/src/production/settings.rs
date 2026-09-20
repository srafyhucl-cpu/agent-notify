use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use agentnotify_storage_sqlite::SqliteStore;
use serde_json::Value;

use crate::bridge::dto::{QuietHoursDto, SettingsDto, UpdateChannelDto};
use crate::bridge::error::CommandError;
use crate::lifecycle::LifecycleError;
use crate::lifecycle::pause::PauseSettingsStore;

pub const KEY_NOTIFICATIONS_PAUSED: &str = "notificationsPaused";
pub const KEY_QUIET_HOURS: &str = "notification.quietHours";
pub const KEY_COOLDOWN_MIN: &str = "notification.cooldownMin";
pub const KEY_COOLDOWN_SECONDS: &str = "notification.cooldownSeconds";
pub const KEY_DEFAULT_AGENT: &str = "notification.defaultAgent";
pub const KEY_DEFAULT_CHANNEL_ACCOUNT_ID: &str = "notification.defaultChannelAccountId";
pub const KEY_REPLY_ENABLED: &str = "reply.enabled";
pub const KEY_REPLY_CONFIRMATION: &str = "reply.confirmation";
pub const KEY_REPLY_ROUTE_TTL_SECONDS: &str = "reply.routeTtlSeconds";
pub const KEY_AUTO_START: &str = "autoStart";
pub const KEY_START_HIDDEN: &str = "startHidden";
pub const KEY_UPDATE_CHANNEL: &str = "updateChannel";

const DEFAULT_ROUTE_TTL_SECONDS: u32 = 86400;

#[derive(Clone)]
pub struct ProductionSettingsStore {
    store: Arc<SqliteStore>,
    legacy_settings_path: PathBuf,
}

impl ProductionSettingsStore {
    pub fn new(store: Arc<SqliteStore>, config_dir: impl AsRef<Path>) -> Self {
        Self {
            store,
            legacy_settings_path: config_dir.as_ref().join("settings.json"),
        }
    }

    pub fn store(&self) -> Arc<SqliteStore> {
        self.store.clone()
    }

    pub async fn load_settings(&self) -> Result<SettingsDto, CommandError> {
        let mut entries = self.store.settings_entries().await.map_err(|error| {
            CommandError::new("settings_read_failed", format!("读取设置失败：{error}"))
        })?;

        // 首次启动且 SQLite 中无暂停设置时，尝试兼容读取旧 settings.json 中的 notificationsPaused
        if !entries.contains_key(KEY_NOTIFICATIONS_PAUSED) && self.legacy_settings_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&self.legacy_settings_path) {
                if let Ok(json) = serde_json::from_str::<Value>(&content) {
                    let legacy_paused = json
                        .get(KEY_NOTIFICATIONS_PAUSED)
                        .or_else(|| json.get("paused"))
                        .and_then(Value::as_bool);
                    if let Some(paused) = legacy_paused {
                        entries.insert(KEY_NOTIFICATIONS_PAUSED.into(), Value::Bool(paused));
                        let mut one = BTreeMap::new();
                        one.insert(KEY_NOTIFICATIONS_PAUSED.into(), Value::Bool(paused));
                        let _ = self.store.write_settings_entries(one).await;
                    }
                }
            }
        }

        Ok(entries_to_dto(&entries))
    }

    pub async fn save_settings(&self, dto: &SettingsDto) -> Result<(), CommandError> {
        validate_settings_dto(dto)?;
        let entries = dto_to_entries(dto);
        self.store
            .write_settings_entries(entries)
            .await
            .map_err(|error| {
                CommandError::new("settings_write_failed", format!("保存设置失败：{error}"))
            })
    }

    /// 首次绑定时为新账号补默认目标；已有显式默认账号时保持不变。
    pub async fn ensure_default_channel_account(
        &self,
        account_id: &str,
    ) -> Result<bool, CommandError> {
        let account_id = account_id.trim();
        if account_id.is_empty() {
            return Err(CommandError::new(
                "default_channel_account_empty",
                "默认通知账号不能为空",
            ));
        }

        let mut settings = self.load_settings().await?;
        if settings
            .default_channel_account_id
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
        {
            return Ok(false);
        }

        settings.default_channel_account_id = Some(account_id.to_owned());
        self.save_settings(&settings).await?;
        Ok(true)
    }
}

#[async_trait::async_trait]
impl PauseSettingsStore for ProductionSettingsStore {
    async fn load_paused(&self) -> Result<bool, LifecycleError> {
        let settings = self
            .load_settings()
            .await
            .map_err(|error| LifecycleError::new(error.code(), error.message()))?;
        Ok(settings.notifications_paused)
    }

    async fn save_paused(&self, paused: bool) -> Result<(), LifecycleError> {
        let mut entries = BTreeMap::new();
        entries.insert(KEY_NOTIFICATIONS_PAUSED.into(), Value::Bool(paused));
        self.store
            .write_settings_entries(entries)
            .await
            .map_err(|error| {
                LifecycleError::new(
                    "pause_settings_save_failed",
                    format!("保存暂停设置失败：{error}"),
                )
            })
    }
}

pub fn entries_to_dto(entries: &BTreeMap<String, Value>) -> SettingsDto {
    let notifications_paused = entries
        .get(KEY_NOTIFICATIONS_PAUSED)
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let quiet_hours = parse_quiet_hours(entries.get(KEY_QUIET_HOURS));

    let cooldown_seconds =
        if let Some(seconds) = entries.get(KEY_COOLDOWN_SECONDS).and_then(Value::as_u64) {
            seconds as u32
        } else if let Some(min) = entries.get(KEY_COOLDOWN_MIN).and_then(Value::as_u64) {
            (min as u32) * 60
        } else {
            0
        };

    let default_channel_account_id = entries
        .get(KEY_DEFAULT_CHANNEL_ACCOUNT_ID)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);

    let reply_enabled = entries
        .get(KEY_REPLY_ENABLED)
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let delivery_receipt_enabled = entries
        .get(KEY_REPLY_CONFIRMATION)
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let route_ttl_seconds = entries
        .get(KEY_REPLY_ROUTE_TTL_SECONDS)
        .and_then(Value::as_u64)
        .map(|v| v as u32)
        .unwrap_or(DEFAULT_ROUTE_TTL_SECONDS);

    let auto_start = entries
        .get(KEY_AUTO_START)
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let start_hidden = entries
        .get(KEY_START_HIDDEN)
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let update_channel = match entries
        .get(KEY_UPDATE_CHANNEL)
        .and_then(Value::as_str)
        .unwrap_or("stable")
    {
        "beta" => UpdateChannelDto::Beta,
        _ => UpdateChannelDto::Stable,
    };

    SettingsDto {
        notifications_paused,
        quiet_hours,
        cooldown_seconds,
        default_channel_account_id,
        reply_enabled,
        delivery_receipt_enabled,
        route_ttl_seconds,
        auto_start,
        start_hidden,
        update_channel,
    }
}

pub fn dto_to_entries(dto: &SettingsDto) -> BTreeMap<String, Value> {
    let mut entries = BTreeMap::new();
    entries.insert(
        KEY_NOTIFICATIONS_PAUSED.into(),
        Value::Bool(dto.notifications_paused),
    );

    let quiet_hours_str = match &dto.quiet_hours {
        Some(qh) if qh.enabled => format!("{}-{}", qh.start, qh.end),
        _ => String::new(),
    };
    entries.insert(KEY_QUIET_HOURS.into(), Value::String(quiet_hours_str));

    entries.insert(
        KEY_COOLDOWN_SECONDS.into(),
        Value::from(dto.cooldown_seconds),
    );
    entries.insert(
        KEY_COOLDOWN_MIN.into(),
        Value::from(dto.cooldown_seconds.div_ceil(60)),
    );

    if let Some(account_id) = &dto.default_channel_account_id {
        entries.insert(
            KEY_DEFAULT_CHANNEL_ACCOUNT_ID.into(),
            Value::String(account_id.clone()),
        );
    } else {
        entries.insert(KEY_DEFAULT_CHANNEL_ACCOUNT_ID.into(), Value::Null);
    }

    entries.insert(KEY_REPLY_ENABLED.into(), Value::Bool(dto.reply_enabled));
    entries.insert(
        KEY_REPLY_CONFIRMATION.into(),
        Value::Bool(dto.delivery_receipt_enabled),
    );
    entries.insert(
        KEY_REPLY_ROUTE_TTL_SECONDS.into(),
        Value::from(dto.route_ttl_seconds),
    );
    entries.insert(KEY_AUTO_START.into(), Value::Bool(dto.auto_start));
    entries.insert(KEY_START_HIDDEN.into(), Value::Bool(dto.start_hidden));

    let channel_str = match dto.update_channel {
        UpdateChannelDto::Beta => "beta",
        UpdateChannelDto::Stable => "stable",
    };
    entries.insert(KEY_UPDATE_CHANNEL.into(), Value::String(channel_str.into()));

    entries
}

fn parse_quiet_hours(val: Option<&Value>) -> Option<QuietHoursDto> {
    let raw = match val {
        Some(Value::String(s)) => s.trim(),
        Some(Value::Object(obj)) => {
            let enabled = obj.get("enabled").and_then(Value::as_bool).unwrap_or(true);
            let start = obj
                .get("start")
                .and_then(Value::as_str)
                .unwrap_or("22:00")
                .to_owned();
            let end = obj
                .get("end")
                .and_then(Value::as_str)
                .unwrap_or("07:00")
                .to_owned();
            return Some(QuietHoursDto {
                enabled,
                start,
                end,
            });
        }
        _ => return None,
    };

    if raw.is_empty() {
        return None;
    }

    let parts: Vec<&str> = raw.split('-').map(str::trim).collect();
    if parts.len() != 2 {
        return None;
    }

    let format_time = |part: &str| -> Option<String> {
        if part.contains(':') {
            let time_parts: Vec<&str> = part.split(':').collect();
            if time_parts.len() == 2 {
                let h: u32 = time_parts[0].parse().ok()?;
                let m: u32 = time_parts[1].parse().ok()?;
                if h < 24 && m < 60 {
                    return Some(format!("{h:02}:{m:02}"));
                }
            }
            None
        } else {
            let h: u32 = part.parse().ok()?;
            if h < 24 {
                Some(format!("{h:02}:00"))
            } else {
                None
            }
        }
    };

    let start = format_time(parts[0])?;
    let end = format_time(parts[1])?;

    Some(QuietHoursDto {
        enabled: true,
        start,
        end,
    })
}

fn validate_settings_dto(dto: &SettingsDto) -> Result<(), CommandError> {
    if dto.cooldown_seconds > 3600 {
        return Err(CommandError::new(
            "cooldown_seconds_out_of_range",
            "通知冷却时间必须在 0 到 3600 秒之间",
        ));
    }
    if dto.route_ttl_seconds < 60 || dto.route_ttl_seconds > 7 * 24 * 3600 {
        return Err(CommandError::new(
            "route_ttl_out_of_range",
            "回复路由有效期必须在 60 到 604800 秒之间",
        ));
    }
    if let Some(qh) = &dto.quiet_hours {
        if qh.enabled {
            if qh.start.trim().is_empty() || qh.end.trim().is_empty() {
                return Err(CommandError::new(
                    "quiet_hours_invalid",
                    "勿扰时段的开始和结束时间不能为空",
                ));
            }
            if qh.start == qh.end {
                return Err(CommandError::new(
                    "quiet_hours_same",
                    "勿扰时段的开始和结束时间不能相同",
                ));
            }
        }
    }
    Ok(())
}
