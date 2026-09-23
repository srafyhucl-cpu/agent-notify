use serde::Deserialize;
use serde_json::{Value, json};

use super::{ImportWarning, LegacyImportError, LegacyPaths};

const DEFAULT_COOLDOWN_MIN: i64 = 10;
const MAX_COOLDOWN_MIN: i64 = 24 * 60;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LegacyConfig {
    pub quiet_hours: String,
    pub cooldown_min: i64,
    pub reply_enabled: bool,
    pub reply_confirmation: bool,
    pub default_agent: Option<String>,
    pub widget_agent_mode: Option<String>,
    pub theme: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LegacySetting {
    pub key: &'static str,
    pub value: Value,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LegacyAgentConfig {
    pub agent_id: &'static str,
    pub enabled: bool,
    pub config: Value,
}

impl LegacyConfig {
    pub(crate) fn settings(&self) -> Vec<LegacySetting> {
        let mut settings = vec![
            LegacySetting {
                key: "notification.quietHours",
                value: json!(self.quiet_hours),
            },
            LegacySetting {
                key: "notification.cooldownMin",
                value: json!(self.cooldown_min),
            },
            LegacySetting {
                key: "reply.enabled",
                value: json!(self.reply_enabled),
            },
            LegacySetting {
                key: "reply.confirmation",
                value: json!(self.reply_confirmation),
            },
            LegacySetting {
                key: "notification.defaultAgent",
                value: json!(self.default_agent.as_deref().unwrap_or_default()),
            },
        ];
        if let Some(theme) = &self.theme {
            settings.push(LegacySetting {
                key: "ui.theme",
                value: json!(theme),
            });
        }
        settings
    }
}

pub(crate) fn parse_config(
    bytes: Option<&[u8]>,
) -> Result<(LegacyConfig, Vec<ImportWarning>), LegacyImportError> {
    let raw = match bytes {
        Some(bytes) => serde_json::from_slice::<RawConfig>(bytes).map_err(|_| {
            LegacyImportError::invalid_field(
                "legacy_config_invalid",
                "config.json",
                "旧版 config.json 不是有效的配置对象",
            )
        })?,
        None => RawConfig::default(),
    };

    let mut warnings = Vec::new();
    let quiet_hours_was_present = raw.quiet_hours.is_some();
    let quiet_hours = normalize_quiet_hours(raw.quiet_hours.unwrap_or_default());
    if quiet_hours_was_present && quiet_hours.is_empty() {
        warnings.push(ImportWarning {
            code: "legacy_quiet_hours_invalid".into(),
            file: "config.json".into(),
            record: None,
        });
    }

    let cooldown_min = normalize_cooldown(raw.cooldown_min.unwrap_or(DEFAULT_COOLDOWN_MIN));
    let default_agent = normalize_optional(raw.default_agent);
    let widget_agent_mode = normalize_widget_mode(raw.widget_agent_mode);
    let theme = normalize_theme(raw.theme);

    Ok((
        LegacyConfig {
            quiet_hours,
            cooldown_min,
            reply_enabled: raw.reply_enabled.unwrap_or(false),
            reply_confirmation: raw.reply_confirmation.unwrap_or(true),
            default_agent,
            widget_agent_mode,
            theme,
        },
        warnings,
    ))
}

/// 旧版 Agent 开关的继承结果。
///
/// 旧版语义：marker 存在 = 关闭，没有 marker = 开启。所以没有 marker 的 Agent
/// 必须写出 `enabled = true`，否则升级后会被宿主的“新适配器默认关闭”静默关掉；
/// 有 marker 的写 `enabled = false`。
///
/// 只有确认这台机器存在旧版遗留时才产出配置行：全新安装没有旧版文件，
/// 默认关闭行交给宿主补齐，避免把新安装当成旧版。
pub(crate) fn agent_configs(paths: &LegacyPaths) -> Vec<LegacyAgentConfig> {
    if !paths.has_legacy_installation() {
        return Vec::new();
    }
    paths
        .agent_markers()
        .map(|(agent_id, marker)| LegacyAgentConfig {
            agent_id,
            enabled: !marker.exists(),
            config: json!({}),
        })
        .collect()
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawConfig {
    quiet_hours: Option<String>,
    cooldown_min: Option<i64>,
    reply_enabled: Option<bool>,
    reply_confirmation: Option<bool>,
    default_agent: Option<String>,
    widget_agent_mode: Option<String>,
    theme: Option<String>,
}

fn normalize_cooldown(value: i64) -> i64 {
    if value <= 0 {
        DEFAULT_COOLDOWN_MIN
    } else {
        value.min(MAX_COOLDOWN_MIN)
    }
}

fn normalize_optional(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
}

fn normalize_widget_mode(value: Option<String>) -> Option<String> {
    let value = normalize_optional(value)?;
    matches!(value.as_str(), "grid" | "single").then_some(value)
}

fn normalize_theme(value: Option<String>) -> Option<String> {
    let value = normalize_optional(value)?;
    matches!(value.as_str(), "light" | "dark").then_some(value)
}

fn normalize_quiet_hours(value: String) -> String {
    let value = value.trim();
    if value.is_empty() {
        return String::new();
    }
    let Some((start, end)) = value.split_once('-') else {
        return String::new();
    };
    let (Ok(start), Ok(end)) = (start.trim().parse::<u8>(), end.trim().parse::<u8>()) else {
        return String::new();
    };
    if start <= 23 && end <= 23 && start != end {
        format!("{start}-{end}")
    } else {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use crate::legacy::LEGACY_AGENT_IDS;

    use super::*;

    #[test]
    fn config_defaults_and_normalization_match_legacy_behavior() {
        let (config, warnings) = parse_config(None).unwrap();
        assert!(warnings.is_empty());
        assert_eq!(config.cooldown_min, 10);
        assert!(config.reply_confirmation);
        assert_eq!(config.quiet_hours, "");
    }

    #[test]
    fn invalid_quiet_hours_and_theme_are_not_imported() {
        let (config, warnings) = parse_config(Some(
            br#"{"quietHours":"8-8","theme":"blue","widgetAgentMode":"single"}"#,
        ))
        .unwrap();
        assert_eq!(config.quiet_hours, "");
        assert_eq!(config.theme, None);
        assert_eq!(config.widget_agent_mode.as_deref(), Some("single"));
        assert_eq!(warnings.len(), 1);
    }

    fn legacy_paths(root: &Path) -> (PathBuf, LegacyPaths) {
        let config_dir = root.join("legacy-config");
        let paths = LegacyPaths::new(&config_dir, root.join("temp"), root.join("data"));
        (config_dir, paths)
    }

    /// 旧版遗留存在时继承开关：没有 marker 的 Agent 保持启用，有 marker 的保持关闭。
    #[test]
    fn agent_configs_inherit_legacy_enabled_state() {
        let temp = tempfile::tempdir().expect("测试临时目录必须可创建");
        let (config_dir, paths) = legacy_paths(temp.path());
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(&paths.config_file, b"{}").expect("旧版 config.json 必须可写入");
        std::fs::write(&paths.codex_marker, b"").expect("旧版 marker 必须可写入");

        let configs = agent_configs(&paths);

        assert_eq!(
            configs
                .iter()
                .map(|config| config.agent_id)
                .collect::<Vec<_>>(),
            LEGACY_AGENT_IDS.to_vec(),
            "继承顺序与 LEGACY_AGENT_IDS 一致，缺一个都会让升级用户少一条开关"
        );
        for config in &configs {
            assert_eq!(
                config.enabled,
                config.agent_id != "codex",
                "{} 的开关必须继承旧版语义",
                config.agent_id
            );
            assert_eq!(config.config, json!({}), "{}", config.agent_id);
        }
    }

    /// `setup-state.json` 是旧版首次运行接入成功后才写的文件，必须算旧版遗留。
    #[test]
    fn setup_state_file_counts_as_legacy_installation() {
        let temp = tempfile::tempdir().expect("测试临时目录必须可创建");
        let (config_dir, paths) = legacy_paths(temp.path());
        std::fs::create_dir_all(&config_dir).unwrap();
        assert!(!paths.has_legacy_installation());

        std::fs::write(&paths.setup_state_file, br#"{"version":"1.9.0"}"#)
            .expect("旧版 setup-state.json 必须可写入");

        assert!(paths.has_legacy_installation());
        assert_eq!(agent_configs(&paths).len(), LEGACY_AGENT_IDS.len());
    }

    /// 全新安装：配置目录存在、但只有新版自己的文件时不得产出任何配置行。
    #[test]
    fn agent_configs_are_empty_without_legacy_files() {
        let temp = tempfile::tempdir().expect("测试临时目录必须可创建");
        let (config_dir, paths) = legacy_paths(temp.path());
        // 新版启动同样会在配置目录里创建回复收件箱，不能当成旧版痕迹。
        std::fs::create_dir_all(config_dir.join("opencode-reply-inbox")).unwrap();

        assert!(!paths.has_legacy_installation());
        assert!(agent_configs(&paths).is_empty());
    }
}
