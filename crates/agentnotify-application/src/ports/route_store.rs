use agentnotify_domain::{ReplyRoute, RouteKey, Timestamp};

use crate::StoreError;

#[async_trait::async_trait]
pub trait RouteStore: Send + Sync {
    async fn find_route(
        &self,
        key: &RouteKey,
        now: Timestamp,
    ) -> Result<Option<ReplyRoute>, StoreError>;

    async fn insert_route(&self, route: ReplyRoute) -> Result<(), StoreError>;
}
