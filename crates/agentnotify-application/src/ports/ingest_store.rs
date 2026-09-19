use agentnotify_domain::{AgentId, Notification};

use crate::StoreError;

#[async_trait::async_trait]
pub trait IngestStore: Send + Sync {
    async fn commit_ingest(
        &self,
        notification: Notification,
        outbox: Vec<super::OutboxItem>,
    ) -> Result<(), StoreError>;

    async fn notification_by_ingest_key(
        &self,
        agent_id: &AgentId,
        ingest_key: &str,
    ) -> Result<Option<Notification>, StoreError>;
}
