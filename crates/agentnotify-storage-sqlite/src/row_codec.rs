use std::sync::LazyLock;

use agentnotify_application::{OutboxItem, OutboxState, StoreError};
use agentnotify_domain::{
    AgentId, AgentSessionId, ChannelAccountId, ChannelId, ClaimKey, ClaimState, Delivery,
    DeliveryId, DomainError, ExternalMessageId, InboundClaim, Notification, NotificationId,
    NotificationMetadata, ReplyRoute, RouteKey, SafeError, Timestamp,
};
use rusqlite::Row;
use time::format_description::{FormatItem, well_known::Rfc3339};

static DATABASE_TIMESTAMP_FORMAT: LazyLock<Vec<FormatItem<'static>>> = LazyLock::new(|| {
    // 用 parse_borrowed：time 0.3.55 起废弃了 format_description::parse。
    time::format_description::parse_borrowed::<2>(
        "[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z",
    )
    .expect("数据库时间格式必须有效")
});

pub(crate) fn timestamp_to_db(value: Timestamp) -> String {
    let parsed = time::OffsetDateTime::parse(&value.to_rfc3339(), &Rfc3339)
        .expect("Timestamp 始终保存为 RFC3339");
    parsed
        .to_offset(time::UtcOffset::UTC)
        .format(&*DATABASE_TIMESTAMP_FORMAT)
        .expect("数据库时间格式化不会失败")
}

pub(crate) fn notification_from_row(row: &Row<'_>) -> Result<Notification, StoreError> {
    let metadata_json: String = column(row, "metadata_json")?;
    let metadata: NotificationMetadata = serde_json::from_str(&metadata_json)
        .map_err(|_| StoreError::corrupted("通知元数据损坏，无法读取历史记录"))?;
    let session_id = optional_column::<String>(row, "session_id")?
        .map(AgentSessionId::new)
        .transpose()
        .map_err(|error| domain_error("通知会话标识损坏", error))?;

    Notification::new(
        NotificationId::new(column::<String>(row, "notification_id")?)
            .map_err(|error| domain_error("通知标识损坏", error))?,
        column::<String>(row, "ingest_key")?,
        AgentId::new(column::<String>(row, "agent_id")?)
            .map_err(|error| domain_error("Agent 标识损坏", error))?,
        session_id,
        optional_column(row, "session_title")?,
        column::<String>(row, "title")?,
        column::<String>(row, "body")?,
        timestamp_column(row, "occurred_at")?,
        metadata,
    )
    .map_err(|error| domain_error("通知内容损坏", error))
}

pub(crate) fn delivery_from_row(row: &Row<'_>) -> Result<Delivery, StoreError> {
    let mut delivery = Delivery::pending(
        DeliveryId::new(column::<String>(row, "delivery_id")?)
            .map_err(|error| domain_error("投递标识损坏", error))?,
        NotificationId::new(column::<String>(row, "notification_id")?)
            .map_err(|error| domain_error("通知标识损坏", error))?,
        ChannelId::new(column::<String>(row, "channel_id")?)
            .map_err(|error| domain_error("渠道标识损坏", error))?,
        ChannelAccountId::new(column::<String>(row, "account_id")?)
            .map_err(|error| domain_error("渠道账号标识损坏", error))?,
    );

    match column::<String>(row, "state")?.as_str() {
        "Pending" => {}
        "Sent" => {
            let external_message_id = optional_column::<String>(row, "external_message_id")?
                .ok_or_else(|| StoreError::corrupted("已发送投递缺少外部消息标识"))?;
            let external_message_id = ExternalMessageId::new(external_message_id)
                .map_err(|error| domain_error("外部消息标识损坏", error))?;
            delivery
                .mark_sent(external_message_id)
                .map_err(|error| domain_error("投递状态损坏", error))?;
        }
        "Failed" => {
            let error = safe_error_from_row(row)?;
            if column::<i64>(row, "retryable")? != 0 {
                delivery
                    .mark_retryable(error)
                    .map_err(|domain| domain_error("投递状态损坏", domain))?;
            } else {
                delivery
                    .mark_permanent_failure(error)
                    .map_err(|domain| domain_error("投递状态损坏", domain))?;
            }
        }
        "Unknown" => {
            delivery
                .mark_unknown(safe_error_from_row(row)?)
                .map_err(|error| domain_error("投递状态损坏", error))?;
        }
        "Skipped" => {
            delivery
                .mark_skipped(safe_error_from_row(row)?)
                .map_err(|error| domain_error("投递状态损坏", error))?;
        }
        _ => return Err(StoreError::corrupted("投递状态包含未知值")),
    }

    Ok(delivery)
}

pub(crate) fn route_from_row(row: &Row<'_>) -> Result<ReplyRoute, StoreError> {
    Ok(ReplyRoute::new(
        RouteKey::new(
            ChannelId::new(column::<String>(row, "channel_id")?)
                .map_err(|error| domain_error("渠道标识损坏", error))?,
            ChannelAccountId::new(column::<String>(row, "account_id")?)
                .map_err(|error| domain_error("渠道账号标识损坏", error))?,
            ExternalMessageId::new(column::<String>(row, "external_message_id")?)
                .map_err(|error| domain_error("外部消息标识损坏", error))?,
        ),
        AgentId::new(column::<String>(row, "agent_id")?)
            .map_err(|error| domain_error("Agent 标识损坏", error))?,
        AgentSessionId::new(column::<String>(row, "session_id")?)
            .map_err(|error| domain_error("Agent 会话标识损坏", error))?,
        timestamp_column(row, "created_at")?,
        timestamp_column(row, "expires_at")?,
    ))
}

pub(crate) fn claim_from_row(row: &Row<'_>) -> Result<InboundClaim, StoreError> {
    let state = ClaimState::parse(&column::<String>(row, "state")?)
        .map_err(|error| domain_error("入站 Claim 状态损坏", error))?;
    let mut claim = InboundClaim::new(
        ClaimKey::new(column::<String>(row, "claim_key")?)
            .map_err(|error| domain_error("入站 Claim 标识损坏", error))?,
        ChannelId::new(column::<String>(row, "channel_id")?)
            .map_err(|error| domain_error("渠道标识损坏", error))?,
        ChannelAccountId::new(column::<String>(row, "account_id")?)
            .map_err(|error| domain_error("渠道账号标识损坏", error))?,
        optional_column::<String>(row, "external_message_id")?
            .map(ExternalMessageId::new)
            .transpose()
            .map_err(|error| domain_error("外部消息标识损坏", error))?,
        timestamp_column(row, "received_at")?,
        timestamp_column(row, "expires_at")?,
    )
    .map_err(|error| domain_error("入站 Claim 损坏", error))?;

    match state {
        ClaimState::InProgress => {}
        ClaimState::Completed => claim
            .mark_completed(timestamp_column(row, "updated_at")?)
            .map_err(|error| domain_error("入站 Claim 状态损坏", error))?,
        ClaimState::Failed => claim
            .mark_failed(timestamp_column(row, "updated_at")?)
            .map_err(|error| domain_error("入站 Claim 状态损坏", error))?,
        ClaimState::Unknown => claim
            .mark_unknown(timestamp_column(row, "updated_at")?)
            .map_err(|error| domain_error("入站 Claim 状态损坏", error))?,
    }

    Ok(claim)
}

pub(crate) fn outbox_state_from_str(value: &str) -> Result<OutboxState, StoreError> {
    match value {
        "Pending" => Ok(OutboxState::Pending),
        "Leased" => Ok(OutboxState::Leased),
        "Done" => Ok(OutboxState::Done),
        "Unknown" => Ok(OutboxState::Unknown),
        "Dead" => Ok(OutboxState::Dead),
        _ => Err(StoreError::corrupted("Outbox 状态包含未知值")),
    }
}

pub(crate) fn outbox_item_from_row(row: &Row<'_>) -> Result<OutboxItem, StoreError> {
    let attempt_count = column::<i64>(row, "attempt_count")?
        .try_into()
        .map_err(|_| StoreError::corrupted("Outbox 尝试次数无效"))?;
    let last_error = optional_safe_error(
        optional_column(row, "last_error_code")?,
        optional_column(row, "last_error_message")?,
    )?;
    Ok(OutboxItem {
        id: column(row, "outbox_id")?,
        notification_id: NotificationId::new(column::<String>(row, "notification_id")?)
            .map_err(|error| domain_error("通知标识损坏", error))?,
        state: outbox_state_from_str(&column::<String>(row, "state")?)?,
        available_at: timestamp_column(row, "available_at")?,
        attempt_count,
        last_error,
    })
}

pub(crate) fn safe_error_parts(error: Option<&SafeError>) -> (Option<String>, Option<String>) {
    match error {
        Some(error) => (
            Some(error.code().to_owned()),
            Some(error.message().to_owned()),
        ),
        None => (None, None),
    }
}

pub(crate) fn domain_error(context: &str, error: DomainError) -> StoreError {
    StoreError::corrupted(format!("{context}: {}", error.message()))
}

pub(crate) fn column<T: rusqlite::types::FromSql>(
    row: &Row<'_>,
    name: &str,
) -> Result<T, StoreError> {
    row.get(name)
        .map_err(|_| StoreError::corrupted(format!("读取字段 {name} 失败")))
}

pub(crate) fn optional_column<T: rusqlite::types::FromSql>(
    row: &Row<'_>,
    name: &str,
) -> Result<Option<T>, StoreError> {
    row.get(name)
        .map_err(|_| StoreError::corrupted(format!("读取字段 {name} 失败")))
}

pub(crate) fn timestamp_column(row: &Row<'_>, name: &str) -> Result<Timestamp, StoreError> {
    Timestamp::parse_rfc3339(&column::<String>(row, name)?)
        .map_err(|error| domain_error("时间字段损坏", error))
}

fn optional_safe_error(
    code: Option<String>,
    message: Option<String>,
) -> Result<Option<SafeError>, StoreError> {
    match (code, message) {
        (Some(code), Some(message)) => SafeError::new(code, message)
            .map(Some)
            .map_err(|error| domain_error("错误信息损坏", error)),
        (None, None) => Ok(None),
        _ => Err(StoreError::corrupted("错误信息字段不完整")),
    }
}

fn safe_error_from_row(row: &Row<'_>) -> Result<SafeError, StoreError> {
    optional_safe_error(
        optional_column(row, "error_code")?,
        optional_column(row, "error_message")?,
    )?
    .ok_or_else(|| StoreError::corrupted("投递错误信息缺失"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn database_timestamp_uses_fixed_width_milliseconds() {
        let timestamp = Timestamp::parse_rfc3339("2026-09-19T09:00:00.8+08:00").unwrap();
        let formatted = timestamp_to_db(timestamp);

        assert_eq!(formatted, "2026-09-19T01:00:00.800Z");
        assert!(
            formatted
                < timestamp_to_db(Timestamp::parse_rfc3339("2026-09-19T01:00:00.801Z").unwrap())
        );
    }
}
