#[derive(
    Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, serde::Deserialize, serde::Serialize,
)]
#[serde(transparent)]
pub struct Timestamp(time::OffsetDateTime);

impl Timestamp {
    pub fn now_utc() -> Self {
        Self(time::OffsetDateTime::now_utc())
    }

    pub fn parse_rfc3339(value: &str) -> Result<Self, crate::DomainError> {
        time::OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339)
            .map(Self)
            .map_err(|_| crate::DomainError::InvalidTimestamp)
    }

    pub fn to_rfc3339(self) -> String {
        self.0
            .format(&time::format_description::well_known::Rfc3339)
            .expect("OffsetDateTime 使用固定 RFC3339 格式不会失败")
    }

    pub fn checked_add(self, duration: time::Duration) -> Option<Self> {
        self.0.checked_add(duration).map(Self)
    }

    /// Unix 毫秒时间戳（内部比较用：如编排看门狗判断「回合完成晚于派活」）。
    pub fn unix_millis(self) -> i64 {
        (self.0.unix_timestamp_nanos() / 1_000_000) as i64
    }
}
