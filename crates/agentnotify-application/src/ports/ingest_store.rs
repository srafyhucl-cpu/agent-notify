use agentnotify_domain::{AgentId, AgentSessionId, Notification, Timestamp};

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

    async fn recent_notification_at(
        &self,
        agent_id: &AgentId,
        session_id: &AgentSessionId,
    ) -> Result<Option<Timestamp>, StoreError>;
}
