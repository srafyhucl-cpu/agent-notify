use agentnotify_application::{StatusSnapshot, StatusStore, StoreError};
use agentnotify_domain::{SafeError, Timestamp};
use rusqlite::params;

use crate::SqliteStore;
use crate::migrations::storage_error;
use crate::row_codec::timestamp_to_db;
use crate::sqlite_helpers::query_optional;

const RECENT_ERROR_KEY: &str = "status.recent_error";

#[async_trait::async_trait]
impl StatusStore for SqliteStore {
    async fn snapshot(&self) -> Result<StatusSnapshot, StoreError> {
        self.run(|connection| {
            let (notification_count, delivery_count, pending_outbox_count) = connection
                .query_row(
                    "SELECT \
                        (SELECT COUNT(*) FROM notifications), \
                        (SELECT COUNT(*) FROM deliveries), \
                        (SELECT COUNT(*) FROM outbox WHERE state = 'Pending')",
                    [],
                    |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, i64>(1)?,
                            row.get::<_, i64>(2)?,
                        ))
                    },
                )
                .map_err(|error| storage_error("读取运行状态失败", error))?;
            let recent_error = query_optional(
                connection,
                "SELECT value_json FROM settings WHERE key = ?1",
                params![RECENT_ERROR_KEY],
                |row| {
                    let value = row
                        .get::<_, String>(0)
                        .map_err(|_| StoreError::corrupted("读取最近错误失败"))?;
                    serde_json::from_str(&value)
                        .map_err(|_| StoreError::corrupted("最近错误记录损坏"))
                },
            )?;

            Ok(StatusSnapshot {
                notification_count: notification_count
                    .try_into()
                    .map_err(|_| StoreError::corrupted("通知数量超出可显示范围"))?,
                delivery_count: delivery_count
                    .try_into()
                    .map_err(|_| StoreError::corrupted("投递数量超出可显示范围"))?,
                pending_outbox_count: pending_outbox_count
                    .try_into()
                    .map_err(|_| StoreError::corrupted("Outbox 数量超出可显示范围"))?,
                recent_error,
            })
        })
        .await
    }

    async fn record_error(&self, error: SafeError) -> Result<(), StoreError> {
        self.run(move |connection| {
            let value_json = serde_json::to_string(&error)
                .map_err(|_| StoreError::corrupted("序列化最近错误失败"))?;
            connection
                .execute(
                    "INSERT INTO settings(key, value_json, updated_at) VALUES (?1, ?2, ?3) \
                     ON CONFLICT(key) DO UPDATE SET \
                        value_json = excluded.value_json, updated_at = excluded.updated_at",
                    params![
                        RECENT_ERROR_KEY,
                        value_json,
                        timestamp_to_db(Timestamp::now_utc())
                    ],
                )
                .map_err(|error| storage_error("保存最近错误失败", error))?;
            Ok(())
        })
        .await
    }
}
