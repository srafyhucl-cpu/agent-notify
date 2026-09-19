use agentnotify_domain::{
    Delivery, ExternalMessageId, Notification, NotificationId, ReplyRoute, SafeError, Timestamp,
};

use crate::StoreError;

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum OutboxState {
    Pending,
    Leased,
    Done,
    Unknown,
    Dead,
}

impl OutboxState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "Pending",
            Self::Leased => "Leased",
            Self::Done => "Done",
            Self::Unknown => "Unknown",
            Self::Dead => "Dead",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct OutboxItem {
    pub id: String,
    pub notification_id: NotificationId,
    pub state: OutboxState,
    pub available_at: Timestamp,
    pub attempt_count: u32,
    pub last_error: Option<SafeError>,
}

impl OutboxItem {
    pub fn pending(
        id: impl Into<String>,
        notification_id: NotificationId,
        available_at: Timestamp,
    ) -> Self {
        Self {
            id: id.into(),
            notification_id,
            state: OutboxState::Pending,
            available_at,
            attempt_count: 0,
            last_error: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutboxLease {
    pub outbox: OutboxItem,
    pub notification: Notification,
    pub owner: String,
    pub lease_until: Timestamp,
}

#[async_trait::async_trait]
pub trait DeliveryStore: Send + Sync {
    async fn lease_next_outbox(
        &self,
        now: Timestamp,
        lease_until: Timestamp,
    ) -> Result<Option<OutboxLease>, StoreError>;

    async fn commit_delivery(
        &self,
        lease: OutboxLease,
        delivery: Delivery,
        route: Option<ReplyRoute>,
    ) -> Result<(), StoreError>;

    async fn reschedule_outbox(
        &self,
        lease: OutboxLease,
        delivery: Delivery,
        next_attempt_at: Timestamp,
    ) -> Result<(), StoreError>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeliveryRecord {
    pub delivery: Delivery,
    pub external_message_id: Option<ExternalMessageId>,
}
