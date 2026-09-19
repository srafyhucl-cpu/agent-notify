use agentnotify_domain::SafeError;

use crate::StoreError;

#[derive(Clone, Debug, Default, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct StatusSnapshot {
    pub notification_count: u64,
    pub delivery_count: u64,
    pub pending_outbox_count: u64,
    pub recent_error: Option<SafeError>,
}

#[async_trait::async_trait]
pub trait StatusStore: Send + Sync {
    async fn snapshot(&self) -> Result<StatusSnapshot, StoreError>;

    async fn record_error(&self, error: SafeError) -> Result<(), StoreError>;
}
