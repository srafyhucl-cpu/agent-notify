use agentnotify_application::EventSink;
use agentnotify_domain::{ClaimKey, DeliveryId, NotificationId};
use tokio::sync::broadcast;

const EVENT_CHANNEL_CAPACITY: usize = 256;

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum RuntimeEvent {
    NotificationChanged { notification_id: NotificationId },
    DeliveryChanged { delivery_id: DeliveryId },
    ReplyChanged { claim_key: ClaimKey },
    RuntimeStopped,
}

#[derive(Clone)]
pub struct EventBus {
    sender: broadcast::Sender<RuntimeEvent>,
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

impl EventBus {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        Self { sender }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<RuntimeEvent> {
        self.sender.subscribe()
    }

    fn publish(&self, event: RuntimeEvent) {
        let _ = self.sender.send(event);
    }
}

#[async_trait::async_trait]
impl EventSink for EventBus {
    async fn notification_changed(&self, notification_id: &NotificationId) {
        self.publish(RuntimeEvent::NotificationChanged {
            notification_id: notification_id.clone(),
        });
    }

    async fn delivery_changed(&self, delivery_id: &DeliveryId) {
        self.publish(RuntimeEvent::DeliveryChanged {
            delivery_id: delivery_id.clone(),
        });
    }

    async fn reply_changed(&self, claim_key: &ClaimKey) {
        self.publish(RuntimeEvent::ReplyChanged {
            claim_key: claim_key.clone(),
        });
    }

    async fn runtime_stopped(&self) {
        self.publish(RuntimeEvent::RuntimeStopped);
    }
}
