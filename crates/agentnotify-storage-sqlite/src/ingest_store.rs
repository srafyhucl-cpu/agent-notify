use agentnotify_application::{IngestStore, OutboxItem, StoreError};
use agentnotify_domain::{AgentId, Notification, Timestamp};
use rusqlite::{TransactionBehavior, params};

use crate::SqliteStore;
use crate::migrations::storage_error;
use crate::row_codec::{notification_from_row, safe_error_parts, timestamp_to_db};
use crate::sqlite_helpers::{map_write_error, query_optional};

#[async_trait::async_trait]
impl IngestStore for SqliteStore {
    async fn commit_ingest(
        &self,
        notification: Notification,
        outbox: Vec<OutboxItem>,
    ) -> Result<(), StoreError> {
        self.run(move |connection| {
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(|error| storage_error("开启入站事务失败", error))?;
            let metadata_json = serde_json::to_string(&notification.metadata)
                .map_err(|_| StoreError::corrupted("序列化通知元数据失败"))?;
            let now = timestamp_to_db(Timestamp::now_utc());
            transaction
                .execute(
                    "INSERT INTO notifications(\
                        notification_id, agent_id, ingest_key, session_id, session_title, \
                        title, body, occurred_at, metadata_json, created_at\
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                    params![
                        notification.id.as_str(),
                        notification.agent_id.as_str(),
                        notification.ingest_key,
                        notification.session_id.as_ref().map(|value| value.as_str()),
                        notification.session_title,
                        notification.title,
                        notification.body,
                        timestamp_to_db(notification.occurred_at),
                        metadata_json,
                        now,
                    ],
                )
                .map_err(|error| map_write_error("通知已存在", error))?;

            for item in outbox {
                let (error_code, error_message) = safe_error_parts(item.last_error.as_ref());
                transaction
                    .execute(
                        "INSERT INTO outbox(\
                            outbox_id, notification_id, state, available_at, lease_owner, \
                            lease_until, attempt_count, last_error_code, last_error_message, \
                            created_at, updated_at\
                        ) VALUES (?1, ?2, ?3, ?4, NULL, NULL, ?5, ?6, ?7, ?8, ?8)",
                        params![
                            item.id,
                            item.notification_id.as_str(),
                            item.state.as_str(),
                            timestamp_to_db(item.available_at),
                            i64::from(item.attempt_count),
                            error_code,
                            error_message,
                            now,
                        ],
                    )
                    .map_err(|error| map_write_error("投递任务已存在", error))?;
            }

            transaction
                .commit()
                .map_err(|error| storage_error("提交入站事务失败", error))
        })
        .await
    }

    async fn notification_by_ingest_key(
        &self,
        agent_id: &AgentId,
        ingest_key: &str,
    ) -> Result<Option<Notification>, StoreError> {
        let agent_id = agent_id.clone();
        let ingest_key = ingest_key.to_owned();
        self.run(move |connection| {
            query_optional(
                connection,
                "SELECT notification_id, agent_id, ingest_key, session_id, session_title, \
                        title, body, occurred_at, metadata_json \
                 FROM notifications WHERE agent_id = ?1 AND ingest_key = ?2",
                params![agent_id.as_str(), ingest_key],
                notification_from_row,
            )
        })
        .await
    }
}
