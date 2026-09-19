use agentnotify_domain::{DeliveryErrorKind, Timestamp};

const DEFAULT_MAX_ATTEMPTS: u32 = 8;
const DEFAULT_BASE_BACKOFF_SECONDS: u64 = 5;
const DEFAULT_MAX_BACKOFF_SECONDS: u64 = 300;
const DEFAULT_MAX_RETRY_AFTER_SECONDS: u64 = 900;
const DEFAULT_JITTER_PERCENT: u64 = 20;

/// 有界指数退避策略。抖动按尝试次数确定性计算，便于测试和恢复。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub base_backoff: time::Duration,
    pub max_backoff: time::Duration,
    pub max_retry_after: time::Duration,
    pub jitter_percent: u64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: DEFAULT_MAX_ATTEMPTS,
            base_backoff: time::Duration::seconds(DEFAULT_BASE_BACKOFF_SECONDS as i64),
            max_backoff: time::Duration::seconds(DEFAULT_MAX_BACKOFF_SECONDS as i64),
            max_retry_after: time::Duration::seconds(DEFAULT_MAX_RETRY_AFTER_SECONDS as i64),
            jitter_percent: DEFAULT_JITTER_PERCENT,
        }
    }
}

impl RetryPolicy {
    pub fn next_attempt(
        &self,
        attempt: u32,
        kind: DeliveryErrorKind,
        now: Timestamp,
    ) -> Option<Timestamp> {
        self.next_attempt_with_retry_after(attempt, kind, now, None)
    }

    pub fn next_attempt_with_retry_after(
        &self,
        attempt: u32,
        kind: DeliveryErrorKind,
        now: Timestamp,
        retry_after: Option<time::Duration>,
    ) -> Option<Timestamp> {
        if kind != DeliveryErrorKind::Retryable || attempt >= self.max_attempts {
            return None;
        }

        let exponent = attempt.saturating_sub(1).min(31);
        let multiplier = 1_u64 << exponent;
        let base_seconds = self.base_backoff.whole_seconds().max(1) as u64;
        let max_seconds = self.max_backoff.whole_seconds().max(1) as u64;
        let backoff_seconds = base_seconds.saturating_mul(multiplier).min(max_seconds);
        let jittered_seconds = backoff_seconds
            .saturating_mul(100 + (self.jitter_percent.min(20) * u64::from((attempt * 17) % 21)))
            / 100;
        let retry_after_seconds = retry_after
            .map(|value| value.whole_seconds().max(0) as u64)
            .unwrap_or(0)
            .min(self.max_retry_after.whole_seconds().max(0) as u64);
        let delay_seconds = jittered_seconds.max(retry_after_seconds).max(1);
        now.checked_add(time::Duration::seconds(delay_seconds as i64))
    }
}
