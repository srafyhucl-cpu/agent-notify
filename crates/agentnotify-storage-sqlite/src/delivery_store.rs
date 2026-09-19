use agentnotify_application::{
    DeliveryRecord, DeliveryStore, OutboxLease, OutboxState, StoreError,
};
use agentnotify_domain::{Delivery, DeliveryId, DeliveryState, Timestamp};
use rusqlite::{Transaction, TransactionBehavior, params};

use crate::SqliteStore;
use crate::migrations::storage_error;
use crate::row_codec::{
    column, delivery_from_row, optional_column, outbox_item_from_row, route_from_row,
    safe_error_parts, timestamp_to_db,
};
use crate::sqlite_helpers::{map_write_error, query_optional};

impl SqliteStore {
    /// 读取投递记录，供运行时状态页和集成测试使用。
    pub async fn delivery(&self, id: DeliveryId) -> Result<Option<DeliveryRecord>, StoreError> {
        self.run(move |connection| {
            let delivery = query_optional(
                connection,
                "SELECT delivery_id, notification_id, channel_id, account_id, state, \
                        external_message_id, error_code, error_message, retryable \
                 FROM deliveries WHERE delivery_id = ?1",
                params![id.as_str()],
                delivery_from_row,
            )?;
            Ok(delivery.map(|delivery| DeliveryRecord {
                external_message_id: delivery.external_message_id().cloned(),
                delivery,
            }))
        })
        .await
    }
}

#[async_trait::async_trait]
impl DeliveryStore for SqliteStore {
    async fn lease_next_outbox(
        &self,
        now: Timestamp,
        lease_until: Timestamp,
    ) -> Result<Option<OutboxLease>, StoreError> {
        if lease_until <= now {
            return Err(StoreError::conflict(
                "invalid_lease",
                "Outbox 租约到期时间必须晚于当前时间",
            ));
        }

        self.run(move |connection| {
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(|error| storage_error("开启 Outbox 租约事务失败", error))?;
            let now_db = timestamp_to_db(now);
            let candidate = query_optional(
                &transaction,
                "SELECT outbox_id FROM outbox \
                 WHERE state = 'Pending' AND available_at <= ?1 \
                 ORDER BY available_at, created_at, outbox_id LIMIT 1",
                params![now_db],
                |row| column::<String>(row, "outbox_id"),
            )?;
            let Some(candidate_id) = candidate else {
                transaction
                    .commit()
                    .map_err(|error| storage_error("提交空 Outbox 租约事务失败", error))?;
                return Ok(None);
            };

            let owner = format!("sqlite-worker-{}", uuid::Uuid::new_v4());
            let updated = transaction
                .execute(
                    "UPDATE outbox SET state = 'Leased', lease_owner = ?1, lease_until = ?2, \
                                        attempt_count = attempt_count + 1, updated_at = ?3 \
                     WHERE outbox_id = ?4 AND state = 'Pending' AND available_at <= ?3",
                    params![
                        owner,
                        timestamp_to_db(lease_until),
                        now_db,
                        candidate_id.as_str()
                    ],
                )
                .map_err(|error| storage_error("抢占 Outbox 租约失败", error))?;
            if updated != 1 {
                return Err(StoreError::conflict(
                    "outbox_not_available",
                    "Outbox 已被其他任务抢占",
                ));
            }

            let outbox = query_optional(
                &transaction,
                "SELECT outbox_id, notification_id, state, available_at, attempt_count, \
                        last_error_code, last_error_message FROM outbox WHERE outbox_id = ?1",
                params![candidate_id.as_str()],
                outbox_item_from_row,
            )?
            .ok_or_else(|| StoreError::not_found("outbox_missing", "找不到 Outbox 任务"))?;
            let notification = query_optional(
                &transaction,
                "SELECT notification_id, agent_id, ingest_key, session_id, session_title, \
                        title, body, occurred_at, metadata_json \
                 FROM notifications WHERE notification_id = ?1",
                params![outbox.notification_id.as_str()],
                crate::row_codec::notification_from_row,
            )?
            .ok_or_else(|| StoreError::not_found("notification_missing", "Outbox 找不到通知"))?;
            let lease = OutboxLease {
                outbox,
                notification,
                owner,
                lease_until,
            };
            transaction
                .commit()
                .map_err(|error| storage_error("提交 Outbox 租约事务失败", error))?;
            Ok(Some(lease))
        })
        .await
    }

    async fn commit_delivery(
        &self,
        lease: OutboxLease,
        delivery: Delivery,
        route: Option<agentnotify_domain::ReplyRoute>,
    ) -> Result<(), StoreError> {
        let outbox_state = outbox_state_for_delivery(&delivery)?;
        if route.is_some() && delivery.state() != DeliveryState::Sent {
            return Err(StoreError::conflict(
                "route_requires_sent",
                "只有成功投递才能建立回复路由",
            ));
        }

        self.run(move |connection| {
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(|error| storage_error("开启投递提交事务失败", error))?;
            ensure_lease(&transaction, &lease)?;
            write_delivery(&transaction, &lease, &delivery)?;
            if let Some(route) = route {
                upsert_route(&transaction, &route)?;
            }
            update_outbox_after_delivery(&transaction, &lease, &delivery, outbox_state)?;
            transaction
                .commit()
                .map_err(|error| storage_error("提交投递事务失败", error))
        })
        .await
    }

    async fn reschedule_outbox(
        &self,
        lease: OutboxLease,
        delivery: Delivery,
        next_attempt_at: Timestamp,
    ) -> Result<(), StoreError> {
        if delivery.state() != DeliveryState::Failed || !delivery.can_retry() {
            return Err(StoreError::conflict(
                "delivery_not_retryable",
                "只有可重试失败才能重新排队",
            ));
        }

        self.run(move |connection| {
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(|error| storage_error("开启 Outbox 重排事务失败", error))?;
            ensure_lease(&transaction, &lease)?;
            write_delivery(&transaction, &lease, &delivery)?;
            let (error_code, error_message) = safe_error_parts(delivery.error());
            let updated = transaction
                .execute(
                    "UPDATE outbox SET state = 'Pending', available_at = ?1, \
                                        lease_owner = NULL, lease_until = NULL, \
                                        last_error_code = ?2, last_error_message = ?3, \
                                        updated_at = ?4 \
                     WHERE outbox_id = ?5 AND state = 'Leased' AND lease_owner = ?6",
                    params![
                        timestamp_to_db(next_attempt_at),
                        error_code,
                        error_message,
                        timestamp_to_db(Timestamp::now_utc()),
                        lease.outbox.id,
                        lease.owner,
                    ],
                )
                .map_err(|error| storage_error("重新排队 Outbox 失败", error))?;
            if updated != 1 {
                return Err(StoreError::conflict(
                    "outbox_not_leased",
                    "Outbox 租约已失效，无法重新排队",
                ));
            }
            transaction
                .commit()
                .map_err(|error| storage_error("提交 Outbox 重排事务失败", error))
        })
        .await
    }
}

fn outbox_state_for_delivery(delivery: &Delivery) -> Result<OutboxState, StoreError> {
    match delivery.state() {
        DeliveryState::Sent | DeliveryState::Skipped => Ok(OutboxState::Done),
        DeliveryState::Failed => Ok(OutboxState::Dead),
        DeliveryState::Unknown => Ok(OutboxState::Unknown),
        DeliveryState::Pending => Err(StoreError::conflict(
            "delivery_pending",
            "投递尚未完成，不能提交 Outbox",
        )),
    }
}

fn ensure_lease(transaction: &Transaction<'_>, lease: &OutboxLease) -> Result<(), StoreError> {
    let current = query_optional(
        transaction,
        "SELECT state, lease_owner, notification_id FROM outbox WHERE outbox_id = ?1",
        params![lease.outbox.id],
        |row| {
            Ok((
                column::<String>(row, "state")?,
                optional_column::<String>(row, "lease_owner")?,
                column::<String>(row, "notification_id")?,
            ))
        },
    )?;
    let Some((state, owner, notification_id)) = current else {
        return Err(StoreError::not_found(
            "outbox_missing",
            "找不到 Outbox 任务",
        ));
    };
    if state != OutboxState::Leased.as_str()
        || owner.as_deref() != Some(lease.owner.as_str())
        || notification_id != lease.outbox.notification_id.as_str()
    {
        return Err(StoreError::conflict(
            "outbox_not_leased",
            "Outbox 租约已失效",
        ));
    }
    Ok(())
}

fn write_delivery(
    transaction: &Transaction<'_>,
    lease: &OutboxLease,
    delivery: &Delivery,
) -> Result<(), StoreError> {
    let external_message_id = delivery.external_message_id().map(|value| value.as_str());
    let (error_code, error_message) = safe_error_parts(delivery.error());
    let retryable = delivery
        .error_kind()
        .is_some_and(|kind| kind.is_retryable()) as i64;
    let now = timestamp_to_db(Timestamp::now_utc());

    transaction
        .execute(
            "INSERT INTO deliveries(\
                delivery_id, notification_id, channel_id, account_id, state, \
                external_message_id, error_code, error_message, retryable, attempt_count, \
                created_at, updated_at\
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11) \
            ON CONFLICT(notification_id, channel_id, account_id) DO UPDATE SET \
                delivery_id = excluded.delivery_id, state = excluded.state, \
                external_message_id = excluded.external_message_id, \
                error_code = excluded.error_code, error_message = excluded.error_message, \
                retryable = excluded.retryable, attempt_count = excluded.attempt_count, \
                updated_at = excluded.updated_at",
            params![
                delivery.id().as_str(),
                delivery.notification_id().as_str(),
                delivery.channel_id().as_str(),
                delivery.account_id().as_str(),
                delivery.state().as_str(),
                external_message_id,
                error_code,
                error_message,
                retryable,
                i64::from(lease.outbox.attempt_count),
                now,
            ],
        )
        .map_err(|error| map_write_error("写入投递记录失败", error))?;
    Ok(())
}

fn update_outbox_after_delivery(
    transaction: &Transaction<'_>,
    lease: &OutboxLease,
    delivery: &Delivery,
    state: OutboxState,
) -> Result<(), StoreError> {
    let (error_code, error_message) = safe_error_parts(delivery.error());
    let updated = transaction
        .execute(
            "UPDATE outbox SET state = ?1, lease_owner = NULL, lease_until = NULL, \
                                last_error_code = ?2, last_error_message = ?3, updated_at = ?4 \
             WHERE outbox_id = ?5 AND state = 'Leased' AND lease_owner = ?6",
            params![
                state.as_str(),
                error_code,
                error_message,
                timestamp_to_db(Timestamp::now_utc()),
                lease.outbox.id,
                lease.owner,
            ],
        )
        .map_err(|error| storage_error("更新 Outbox 状态失败", error))?;
    if updated != 1 {
        return Err(StoreError::conflict(
            "outbox_not_leased",
            "Outbox 租约已失效，无法提交投递",
        ));
    }
    Ok(())
}

pub(crate) fn upsert_route(
    transaction: &Transaction<'_>,
    route: &agentnotify_domain::ReplyRoute,
) -> Result<(), StoreError> {
    if let Some(existing) = query_optional(
        transaction,
        "SELECT channel_id, account_id, external_message_id, agent_id, session_id, \
                created_at, expires_at FROM reply_routes \
         WHERE channel_id = ?1 AND account_id = ?2 AND external_message_id = ?3",
        params![
            route.key.channel_id.as_str(),
            route.key.account_id.as_str(),
            route.key.external_message_id.as_str()
        ],
        route_from_row,
    )? {
        if &existing != route {
            return Err(StoreError::conflict(
                "route_conflict",
                "回复路由已存在且内容不同",
            ));
        }
        return Ok(());
    }

    transaction
        .execute(
            "INSERT INTO reply_routes(\
                channel_id, account_id, external_message_id, agent_id, session_id, \
                created_at, expires_at\
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                route.key.channel_id.as_str(),
                route.key.account_id.as_str(),
                route.key.external_message_id.as_str(),
                route.agent_id.as_str(),
                route.session_id.as_str(),
                timestamp_to_db(route.created_at),
                timestamp_to_db(route.expires_at),
            ],
        )
        .map_err(|error| map_write_error("写入回复路由失败", error))?;
    Ok(())
}
