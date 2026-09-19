use agentnotify_application::{RouteStore, StoreError};
use agentnotify_domain::{ReplyRoute, RouteKey, Timestamp};
use rusqlite::{TransactionBehavior, params};

use crate::SqliteStore;
use crate::delivery_store::upsert_route;
use crate::migrations::storage_error;
use crate::row_codec::{route_from_row, timestamp_to_db};
use crate::sqlite_helpers::query_optional;

#[async_trait::async_trait]
impl RouteStore for SqliteStore {
    async fn find_route(
        &self,
        key: &RouteKey,
        now: Timestamp,
    ) -> Result<Option<ReplyRoute>, StoreError> {
        let key = key.clone();
        self.run(move |connection| {
            query_optional(
                connection,
                "SELECT channel_id, account_id, external_message_id, agent_id, session_id, \
                        created_at, expires_at FROM reply_routes \
                 WHERE channel_id = ?1 AND account_id = ?2 AND external_message_id = ?3 \
                   AND expires_at > ?4",
                params![
                    key.channel_id.as_str(),
                    key.account_id.as_str(),
                    key.external_message_id.as_str(),
                    timestamp_to_db(now)
                ],
                route_from_row,
            )
        })
        .await
    }

    async fn insert_route(&self, route: ReplyRoute) -> Result<(), StoreError> {
        self.run(move |connection| {
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(|error| storage_error("开启回复路由事务失败", error))?;
            upsert_route(&transaction, &route)?;
            transaction
                .commit()
                .map_err(|error| storage_error("提交回复路由事务失败", error))
        })
        .await
    }
}
