//! 通知与投递命令域（`HostCommandService` 的 `notifications` 片段，2026-09 从 service.rs 拆分）。

use super::*;
use crate::bridge::commands::NotificationCommands;

#[async_trait::async_trait]
impl NotificationCommands for ProductionHostCommandService {
    async fn list_notifications(
        &self,
        payload: NotificationFilterPayload,
    ) -> Result<NotificationListDto, CommandError> {
        let from_ts = payload
            .from
            .as_deref()
            .and_then(|s| Timestamp::parse_rfc3339(s).ok());
        let to_ts = payload
            .to
            .as_deref()
            .and_then(|s| Timestamp::parse_rfc3339(s).ok());

        let query = NotificationQuery {
            agent_id: payload.agent_id,
            channel_id: payload.channel_id,
            account_id: payload.account_id,
            delivery_state: payload.delivery_state.map(|s| match s {
                DeliveryStateDto::Pending => DeliveryState::Pending,
                DeliveryStateDto::Sent => DeliveryState::Sent,
                DeliveryStateDto::Failed => DeliveryState::Failed,
                DeliveryStateDto::Unknown => DeliveryState::Unknown,
                DeliveryStateDto::Skipped => DeliveryState::Skipped,
            }),
            from: from_ts,
            to: to_ts,
            search: payload.query,
            cursor: payload.cursor,
            limit: payload.limit,
        };

        let page = self
            .store
            .notification_page(query)
            .await
            .map_err(|e| CommandError::new("notification_query_failed", e.to_string()))?;

        let items = page
            .items
            .into_iter()
            .map(map_notification_record)
            .collect();

        Ok(NotificationListDto {
            items,
            total: page.total,
            next_cursor: page.next_cursor,
        })
    }

    async fn get_notification_detail(
        &self,
        payload: NotificationIdPayload,
    ) -> Result<NotificationDetailDto, CommandError> {
        let id = NotificationId::new(&payload.notification_id)
            .map_err(|e| CommandError::new("invalid_notification_id", e.to_string()))?;

        let record = self
            .store
            .notification_detail(id)
            .await
            .map_err(|e| CommandError::new("notification_detail_query_failed", e.to_string()))?
            .ok_or_else(|| CommandError::new("notification_not_found", "找不到指定通知记录"))?;

        let summary = NotificationSummaryDto {
            id: record.notification.id.to_string(),
            agent_id: record.notification.agent_id.to_string(),
            session_id: record
                .notification
                .session_id
                .as_ref()
                .map(|s| s.to_string()),
            session_title: record.notification.session_title.clone(),
            title: record.notification.title.clone(),
            preview: record.notification.body.chars().take(100).collect(),
            occurred_at: record.notification.occurred_at.to_rfc3339(),
            delivery_states: record
                .deliveries
                .iter()
                .map(|d| map_delivery_state(d.delivery.state()))
                .collect(),
        };

        let deliveries = record
            .deliveries
            .into_iter()
            .map(map_delivery_view_record)
            .collect();

        Ok(NotificationDetailDto {
            notification: summary,
            body: record.notification.body,
            metadata: record.notification.metadata.clone().into_inner(),
            deliveries,
            route_exists: record.route_exists,
        })
    }

    async fn retry_delivery(
        &self,
        payload: DeliveryIdPayload,
    ) -> Result<DeliveryDto, CommandError> {
        let id = DeliveryId::new(&payload.delivery_id)
            .map_err(|e| CommandError::new("invalid_delivery_id", e.to_string()))?;

        // 重新排队 outbox 并获取更新前记录
        let record = self
            .store
            .requeue_delivery(id)
            .await
            .map_err(|e| CommandError::new(e.code(), format!("重试投递失败：{e}")))?;

        // 重新查询该通知对应的最新投递记录
        let latest = self
            .store
            .notification_detail(record.delivery.notification_id().clone())
            .await
            .map_err(|e| CommandError::new(e.code(), format!("查询最新投递状态失败：{e}")))?
            .and_then(|detail| {
                detail.deliveries.into_iter().find(|item| {
                    item.delivery.channel_id() == record.delivery.channel_id()
                        && item.delivery.account_id() == record.delivery.account_id()
                })
            })
            .unwrap_or(record);

        Ok(map_delivery_view_record(latest))
    }
}
