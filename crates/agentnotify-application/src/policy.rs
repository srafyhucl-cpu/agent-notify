use std::collections::BTreeMap;

use agentnotify_domain::{AgentId, AgentSessionId, Timestamp};
use time::format_description::well_known::Rfc3339;

/// 通知抑制原因，作为稳定诊断码使用。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SkipReason {
    AgentNotConfigured,
    AgentDisabled,
    TitleSuppressed,
    QuietHours,
    Cooldown,
}

impl SkipReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AgentNotConfigured => "agent_not_configured",
            Self::AgentDisabled => "agent_disabled",
            Self::TitleSuppressed => "title_suppressed",
            Self::QuietHours => "quiet_hours",
            Self::Cooldown => "cooldown",
        }
    }
}

/// 策略对一条标准化通知作出的决定。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PolicyDecision {
    Deliver,
    Skip(SkipReason),
}

/// 本地安静时段配置。时间按指定 UTC 偏移计算，不依赖操作系统时区。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QuietHours {
    pub start_minute: u16,
    pub end_minute: u16,
    pub utc_offset_minutes: i16,
}

impl QuietHours {
    fn contains(self, now: Timestamp) -> bool {
        let Ok(parsed) = time::OffsetDateTime::parse(&now.to_rfc3339(), &Rfc3339) else {
            return false;
        };
        let Ok(offset) =
            time::UtcOffset::from_whole_seconds(i32::from(self.utc_offset_minutes) * 60)
        else {
            return false;
        };
        let local = parsed.to_offset(offset);
        let minute = u16::from(local.hour()) * 60 + u16::from(local.minute());
        if self.start_minute == self.end_minute {
            return false;
        }
        if self.start_minute < self.end_minute {
            minute >= self.start_minute && minute < self.end_minute
        } else {
            minute >= self.start_minute || minute < self.end_minute
        }
    }
}

/// 单个 Agent 的通知策略配置。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentNotificationConfig {
    pub enabled: bool,
    pub quiet_hours: Option<QuietHours>,
    pub cooldown: Option<time::Duration>,
}

impl Default for AgentNotificationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            quiet_hours: None,
            cooldown: None,
        }
    }
}

/// 评估策略所需的纯输入；服务层负责提供持久化的最近通知时间。
#[derive(Clone, Copy, Debug)]
pub struct PolicyInput<'a> {
    pub agent_id: &'a AgentId,
    pub session_id: Option<&'a AgentSessionId>,
    pub title: &'a str,
    pub now: Timestamp,
    pub recent_notification_at: Option<Timestamp>,
}

/// 不读取系统时钟、不访问数据库的通知策略。
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct NotificationPolicy {
    agents: BTreeMap<AgentId, AgentNotificationConfig>,
}

impl NotificationPolicy {
    pub fn with_agent(mut self, agent_id: AgentId, config: AgentNotificationConfig) -> Self {
        self.agents.insert(agent_id, config);
        self
    }

    pub fn evaluate(&self, input: &PolicyInput<'_>) -> PolicyDecision {
        let Some(config) = self.agents.get(input.agent_id) else {
            return PolicyDecision::Skip(SkipReason::AgentNotConfigured);
        };
        if !config.enabled {
            return PolicyDecision::Skip(SkipReason::AgentDisabled);
        }
        if input.title.contains("🔕") || input.title.contains("[勿扰]") {
            return PolicyDecision::Skip(SkipReason::TitleSuppressed);
        }
        if config
            .quiet_hours
            .is_some_and(|quiet_hours| quiet_hours.contains(input.now))
        {
            return PolicyDecision::Skip(SkipReason::QuietHours);
        }
        if let (Some(recent), Some(cooldown)) = (input.recent_notification_at, config.cooldown) {
            if recent
                .checked_add(cooldown)
                .is_some_and(|deadline| input.now < deadline)
            {
                return PolicyDecision::Skip(SkipReason::Cooldown);
            }
        }
        PolicyDecision::Deliver
    }
}
