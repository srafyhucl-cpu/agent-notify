use std::collections::BTreeMap;

use agentnotify_application::StoreError;
use agentnotify_domain::{
    Delivery, DeliveryId, DeliveryState, Notification, NotificationId, Timestamp,
};
use rusqlite::{
    OptionalExtension, TransactionBehavior, params, params_from_iter, types::Value as SqlValue,
};

use crate::{
    SqliteStore,
    migrations::storage_error,
    row_codec::{delivery_from_row, notification_from_row, timestamp_column, timestamp_to_db},
    sqlite_helpers::{map_write_error, query_optional},
};

const DEFAULT_NOTIFICATION_LIMIT: u32 = 50;
const MAX_NOTIFICATION_LIMIT: u32 = 200;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentConfigRecord {
    pub enabled: bool,
    pub config: serde_json::Value,
    pub updated_at: Timestamp,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeliveryViewRecord {
    pub delivery: Delivery,
    pub updated_at: Timestamp,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NotificationRecord {
    pub notification: Notification,
    pub created_at: Timestamp,
    pub delivery_states: Vec<DeliveryState>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NotificationDetailRecord {
    pub notification: Notification,
    pub created_at: Timestamp,
    pub deliveries: Vec<DeliveryViewRecord>,
    pub route_exists: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NotificationPage {
    pub items: Vec<NotificationRecord>,
    pub total: u32,
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct NotificationQuery {
    pub agent_id: Option<String>,
    pub channel_id: Option<String>,
    pub account_id: Option<String>,
    pub delivery_state: Option<DeliveryState>,
    pub from: Option<Timestamp>,
    pub to: Option<Timestamp>,
    pub search: Option<String>,
    pub cursor: Option<String>,
    pub limit: u32,
}

impl NotificationQuery {
    fn effective_limit(&self) -> u32 {
        if self.limit == 0 {
            DEFAULT_NOTIFICATION_LIMIT
        } else {
            self.limit.min(MAX_NOTIFICATION_LIMIT)
        }
    }
}

impl SqliteStore {
    pub async fn agent_configs(&self) -> Result<BTreeMap<String, AgentConfigRecord>, StoreError> {
        self.run(|connection| {
            let mut statement = connection
                .prepare(
                    "SELECT agent_id, enabled, config_json, updated_at \
                     FROM agent_configs ORDER BY agent_id",
                )
                .map_err(|error| storage_error("准备 Agent 配置查询失败", error))?;
            let mut rows = statement
                .query([])
                .map_err(|error| storage_error("查询 Agent 配置失败", error))?;
            let mut result = BTreeMap::new();
            while let Some(row) = rows
                .next()
                .map_err(|error| storage_error("读取 Agent 配置失败", error))?
            {
                let agent_id: String = row
                    .get("agent_id")
                    .map_err(|_| StoreError::corrupted("读取 Agent 标识失败"))?;
                let config_json: String = row
                    .get("config_json")
                    .map_err(|_| StoreError::corrupted("读取 Agent 配置失败"))?;
                let config = serde_json::from_str(&config_json)
                    .map_err(|_| StoreError::corrupted("Agent 配置 JSON 损坏"))?;
                let updated_at = Timestamp::parse_rfc3339(
                    &row.get::<_, String>("updated_at")
                        .map_err(|_| StoreError::corrupted("读取 Agent 配置时间失败"))?,
                )
                .map_err(|_| StoreError::corrupted("Agent 配置时间损坏"))?;
                result.insert(
                    agent_id,
                    AgentConfigRecord {
                        enabled: row
                            .get::<_, i64>("enabled")
                            .map_err(|_| StoreError::corrupted("读取 Agent 启用状态失败"))?
                            != 0,
                        config,
                        updated_at,
                    },
                );
            }
            Ok(result)
        })
        .await
    }

    pub async fn upsert_agent_config(
        &self,
        agent_id: &str,
        enabled: bool,
        config: &serde_json::Value,
    ) -> Result<AgentConfigRecord, StoreError> {
        if agent_id.trim().is_empty() {
            return Err(StoreError::conflict("agent_id_empty", "Agent 标识不能为空"));
        }
        let agent_id = agent_id.to_owned();
        let config = config.clone();
        let updated_at = Timestamp::now_utc();
        let config_json = serde_json::to_string(&config)
            .map_err(|_| StoreError::conflict("agent_config_invalid", "Agent 配置无法序列化"))?;
        let record = AgentConfigRecord {
            enabled,
            config: config.clone(),
            updated_at,
        };
        let record_for_write = record.clone();
        self.run(move |connection| {
            connection
                .execute(
                    "INSERT INTO agent_configs(agent_id, enabled, config_json, updated_at) \
                     VALUES (?1, ?2, ?3, ?4) \
                     ON CONFLICT(agent_id) DO UPDATE SET \
                        enabled = excluded.enabled, \
                        config_json = excluded.config_json, \
                        updated_at = excluded.updated_at",
                    params![
                        agent_id,
                        record_for_write.enabled,
                        config_json,
                        timestamp_to_db(record_for_write.updated_at)
                    ],
                )
                .map_err(|error| map_write_error("保存 Agent 配置失败", error))?;
            Ok(())
        })
        .await?;
        Ok(record)
    }

    pub async fn recent_deliveries(
        &self,
        limit: u32,
    ) -> Result<Vec<DeliveryViewRecord>, StoreError> {
        let limit = i64::from(limit.clamp(1, MAX_NOTIFICATION_LIMIT));
        self.run(move |connection| {
            let mut statement = connection
                .prepare(
                    "SELECT delivery_id, notification_id, channel_id, account_id, state, \
                            external_message_id, error_code, error_message, retryable, updated_at \
                     FROM deliveries ORDER BY updated_at DESC, delivery_id DESC LIMIT ?1",
                )
                .map_err(|error| storage_error("准备最近投递查询失败", error))?;
            let mut rows = statement
                .query(params![limit])
                .map_err(|error| storage_error("查询最近投递失败", error))?;
            let mut result = Vec::new();
            while let Some(row) = rows
                .next()
                .map_err(|error| storage_error("读取最近投递失败", error))?
            {
                result.push(delivery_view_from_row(row)?);
            }
            Ok(result)
        })
        .await
    }

    pub async fn notification_page(
        &self,
        query: NotificationQuery,
    ) -> Result<NotificationPage, StoreError> {
        let limit = query.effective_limit();
        self.run(move |connection| {
            let mut filters = Vec::new();
            let mut values = Vec::<SqlValue>::new();
            push_notification_filters(&mut filters, &mut values, &query);
            let where_clause = if filters.is_empty() {
                String::new()
            } else {
                format!(" WHERE {}", filters.join(" AND "))
            };

            let total = connection
                .query_row(
                    &format!("SELECT COUNT(*) FROM notifications n{where_clause}"),
                    params_from_iter(values.iter().cloned()),
                    |row| row.get::<_, i64>(0),
                )
                .map_err(|error| storage_error("统计通知历史失败", error))?;

            if let Some(cursor) = query.cursor.as_deref().filter(|value| !value.is_empty()) {
                let exists = connection
                    .query_row(
                        "SELECT 1 FROM notifications WHERE notification_id = ?1",
                        params![cursor],
                        |_| Ok(()),
                    )
                    .optional()
                    .map_err(|error| storage_error("校验通知游标失败", error))?;
                if exists.is_none() {
                    return Err(StoreError::not_found(
                        "notification_cursor_missing",
                        "通知历史游标已失效，请刷新列表",
                    ));
                }
                filters.push(
                    "(n.occurred_at < (SELECT occurred_at FROM notifications WHERE notification_id = ?) \
                     OR (n.occurred_at = (SELECT occurred_at FROM notifications WHERE notification_id = ?) \
                         AND n.notification_id < ?))"
                        .into(),
                );
                values.push(SqlValue::Text(cursor.into()));
                values.push(SqlValue::Text(cursor.into()));
                values.push(SqlValue::Text(cursor.into()));
            }

            let page_where = filters.join(" AND ");
            let page_clause = if page_where.is_empty() {
                String::new()
            } else {
                format!(" WHERE {page_where}")
            };
            let sql = format!(
                "SELECT n.notification_id, n.agent_id, n.ingest_key, n.session_id, \
                        n.session_title, n.title, n.body, n.occurred_at, n.metadata_json, \
                        n.created_at \
                 FROM notifications n{page_clause} \
                 ORDER BY n.occurred_at DESC, n.notification_id DESC LIMIT ?"
            );
            let mut page_values = values;
            page_values.push(SqlValue::Integer(i64::from(limit) + 1));
            let mut statement = connection
                .prepare(&sql)
                .map_err(|error| storage_error("准备通知历史查询失败", error))?;
            let mut rows = statement
                .query(params_from_iter(page_values))
                .map_err(|error| storage_error("查询通知历史失败", error))?;
            let mut records = Vec::new();
            while let Some(row) = rows
                .next()
                .map_err(|error| storage_error("读取通知历史失败", error))?
            {
                let notification = notification_from_row(row)?;
                let created_at = timestamp_column(row, "created_at")?;
                let mut states = Vec::new();
                let mut delivery_statement = connection
                    .prepare("SELECT state FROM deliveries WHERE notification_id = ?1 ORDER BY updated_at")
                    .map_err(|error| storage_error("准备通知投递状态查询失败", error))?;
                let mut delivery_rows = delivery_statement
                    .query(params![notification.id.as_str()])
                    .map_err(|error| storage_error("查询通知投递状态失败", error))?;
                while let Some(delivery_row) = delivery_rows
                    .next()
                    .map_err(|error| storage_error("读取通知投递状态失败", error))?
                {
                    states.push(parse_delivery_state(
                        &delivery_row
                            .get::<_, String>(0)
                            .map_err(|_| StoreError::corrupted("投递状态损坏"))?,
                    )?);
                }
                records.push(NotificationRecord {
                    notification,
                    created_at,
                    delivery_states: states,
                });
            }

            let has_more = records.len() > limit as usize;
            if has_more {
                records.truncate(limit as usize);
            }
            let next_cursor = has_more.then(|| {
                records
                    .last()
                    .expect("存在下一页时至少保留一条记录")
                    .notification
                    .id
                    .to_string()
            });
            Ok(NotificationPage {
                items: records,
                total: total
                    .try_into()
                    .map_err(|_| StoreError::corrupted("通知数量超出可显示范围"))?,
                next_cursor,
            })
        })
        .await
    }

    pub async fn notification_detail(
        &self,
        notification_id: NotificationId,
    ) -> Result<Option<NotificationDetailRecord>, StoreError> {
        self.run(move |connection| {
            let notification = query_optional(
                connection,
                "SELECT notification_id, agent_id, ingest_key, session_id, session_title, \
                        title, body, occurred_at, metadata_json, created_at \
                 FROM notifications WHERE notification_id = ?1",
                params![notification_id.as_str()],
                |row| {
                    Ok((
                        notification_from_row(row)?,
                        timestamp_column(row, "created_at")?,
                    ))
                },
            )?;
            let Some((notification, created_at)) = notification else {
                return Ok(None);
            };

            let mut deliveries = Vec::new();
            let mut statement = connection
                .prepare(
                    "SELECT delivery_id, notification_id, channel_id, account_id, state, \
                            external_message_id, error_code, error_message, retryable, updated_at \
                     FROM deliveries WHERE notification_id = ?1 \
                     ORDER BY updated_at DESC, delivery_id DESC",
                )
                .map_err(|error| storage_error("准备通知投递详情查询失败", error))?;
            let mut rows = statement
                .query(params![notification.id.as_str()])
                .map_err(|error| storage_error("查询通知投递详情失败", error))?;
            while let Some(row) = rows
                .next()
                .map_err(|error| storage_error("读取通知投递详情失败", error))?
            {
                deliveries.push(delivery_view_from_row(row)?);
            }

            let route_exists = match notification.session_id.as_ref() {
                Some(session_id) => {
                    connection
                        .query_row(
                            "SELECT EXISTS(SELECT 1 FROM reply_routes \
                         WHERE agent_id = ?1 AND session_id = ?2 AND expires_at > ?3)",
                            params![
                                notification.agent_id.as_str(),
                                session_id.as_str(),
                                timestamp_to_db(Timestamp::now_utc())
                            ],
                            |row| row.get::<_, i64>(0),
                        )
                        .map_err(|error| storage_error("查询回复路由状态失败", error))?
                        != 0
                }
                None => false,
            };

            Ok(Some(NotificationDetailRecord {
                notification,
                created_at,
                deliveries,
                route_exists,
            }))
        })
        .await
    }

    pub async fn requeue_delivery(
        &self,
        delivery_id: DeliveryId,
    ) -> Result<DeliveryViewRecord, StoreError> {
        let now = Timestamp::now_utc();
        self.run(move |connection| {
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(|error| storage_error("开启投递重排事务失败", error))?;
            let existing = query_optional(
                &transaction,
                "SELECT delivery_id, notification_id, channel_id, account_id, state, \
                        external_message_id, error_code, error_message, retryable, updated_at \
                 FROM deliveries WHERE delivery_id = ?1",
                params![delivery_id.as_str()],
                delivery_view_from_row,
            )?
            .ok_or_else(|| StoreError::not_found("delivery_missing", "找不到投递记录"))?;
            if existing.delivery.state() != DeliveryState::Failed || !existing.delivery.can_retry()
            {
                return Err(StoreError::conflict(
                    "delivery_not_retryable",
                    "只有可重试的失败投递才能重新排队",
                ));
            }

            let updated = transaction
                .execute(
                    "UPDATE outbox SET state = 'Pending', available_at = ?1, \
                                        lease_owner = NULL, lease_until = NULL, \
                                        last_error_code = NULL, last_error_message = NULL, \
                                        updated_at = ?1 \
                     WHERE notification_id = ?2 AND state IN ('Dead', 'Unknown')",
                    params![
                        timestamp_to_db(now),
                        existing.delivery.notification_id().as_str()
                    ],
                )
                .map_err(|error| storage_error("重新排队投递失败", error))?;
            if updated != 1 {
                return Err(StoreError::conflict(
                    "outbox_not_retryable",
                    "投递任务当前不可重新排队",
                ));
            }
            transaction
                .commit()
                .map_err(|error| storage_error("提交投递重排事务失败", error))?;
            Ok(existing)
        })
        .await
    }

    pub async fn settings_entries(
        &self,
    ) -> Result<BTreeMap<String, serde_json::Value>, StoreError> {
        self.run(|connection| {
            let mut statement = connection
                .prepare("SELECT key, value_json FROM settings ORDER BY key")
                .map_err(|error| storage_error("准备设置查询失败", error))?;
            let mut rows = statement
                .query([])
                .map_err(|error| storage_error("查询设置失败", error))?;
            let mut result = BTreeMap::new();
            while let Some(row) = rows
                .next()
                .map_err(|error| storage_error("读取设置失败", error))?
            {
                let key: String = row
                    .get(0)
                    .map_err(|_| StoreError::corrupted("读取设置键失败"))?;
                let value_json: String = row
                    .get(1)
                    .map_err(|_| StoreError::corrupted("读取设置值失败"))?;
                let value = serde_json::from_str(&value_json)
                    .map_err(|_| StoreError::corrupted("设置值 JSON 损坏"))?;
                result.insert(key, value);
            }
            Ok(result)
        })
        .await
    }

    pub async fn write_settings_entries(
        &self,
        entries: BTreeMap<String, serde_json::Value>,
    ) -> Result<(), StoreError> {
        let now = Timestamp::now_utc();
        self.run(move |connection| {
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(|error| storage_error("开启设置更新事务失败", error))?;
            for (key, value) in entries {
                if key.trim().is_empty() {
                    return Err(StoreError::conflict(
                        "settings_key_invalid",
                        "设置键不能为空",
                    ));
                }
                let value_json = serde_json::to_string(&value).map_err(|_| {
                    StoreError::conflict("settings_value_invalid", "设置值无法序列化")
                })?;
                transaction
                    .execute(
                        "INSERT INTO settings(key, value_json, updated_at) VALUES (?1, ?2, ?3) \
                         ON CONFLICT(key) DO UPDATE SET \
                            value_json = excluded.value_json, updated_at = excluded.updated_at",
                        params![key, value_json, timestamp_to_db(now)],
                    )
                    .map_err(|error| map_write_error("保存设置失败", error))?;
            }
            transaction
                .commit()
                .map_err(|error| storage_error("提交设置更新事务失败", error))
        })
        .await
    }
}

fn push_notification_filters(
    filters: &mut Vec<String>,
    values: &mut Vec<SqlValue>,
    query: &NotificationQuery,
) {
    if let Some(agent_id) = query.agent_id.as_deref().filter(|value| !value.is_empty()) {
        filters.push("n.agent_id = ?".into());
        values.push(SqlValue::Text(agent_id.into()));
    }
    if let Some(channel_id) = query
        .channel_id
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        filters.push(
            "EXISTS(SELECT 1 FROM deliveries d WHERE d.notification_id = n.notification_id \
             AND d.channel_id = ?)"
                .into(),
        );
        values.push(SqlValue::Text(channel_id.into()));
    }
    if let Some(account_id) = query
        .account_id
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        filters.push(
            "EXISTS(SELECT 1 FROM deliveries d WHERE d.notification_id = n.notification_id \
             AND d.account_id = ?)"
                .into(),
        );
        values.push(SqlValue::Text(account_id.into()));
    }
    if let Some(state) = query.delivery_state {
        filters.push(
            "EXISTS(SELECT 1 FROM deliveries d WHERE d.notification_id = n.notification_id \
             AND d.state = ?)"
                .into(),
        );
        values.push(SqlValue::Text(state.as_str().into()));
    }
    if let Some(from) = query.from {
        filters.push("n.occurred_at >= ?".into());
        values.push(SqlValue::Text(timestamp_to_db(from)));
    }
    if let Some(to) = query.to {
        filters.push("n.occurred_at <= ?".into());
        values.push(SqlValue::Text(timestamp_to_db(to)));
    }
    if let Some(search) = query
        .search
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        filters.push("(n.title LIKE ? OR n.body LIKE ?)".into());
        let pattern = format!("%{search}%");
        values.push(SqlValue::Text(pattern.clone()));
        values.push(SqlValue::Text(pattern));
    }
}

fn delivery_view_from_row(row: &rusqlite::Row<'_>) -> Result<DeliveryViewRecord, StoreError> {
    Ok(DeliveryViewRecord {
        delivery: delivery_from_row(row)?,
        updated_at: timestamp_column(row, "updated_at")?,
    })
}

fn parse_delivery_state(value: &str) -> Result<DeliveryState, StoreError> {
    match value {
        "Pending" => Ok(DeliveryState::Pending),
        "Sent" => Ok(DeliveryState::Sent),
        "Failed" => Ok(DeliveryState::Failed),
        "Unknown" => Ok(DeliveryState::Unknown),
        "Skipped" => Ok(DeliveryState::Skipped),
        _ => Err(StoreError::corrupted("投递状态包含未知值")),
    }
}
