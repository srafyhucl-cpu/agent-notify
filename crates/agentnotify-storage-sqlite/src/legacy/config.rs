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

pub(crate) fn disabled_agent_configs(paths: &LegacyPaths) -> Vec<LegacyAgentConfig> {
    paths
        .agent_markers()
        .into_iter()
        .filter(|(_, path)| path.exists())
        .map(|(agent_id, _)| LegacyAgentConfig {
            agent_id,
            enabled: false,
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
}
