use agentnotify_domain::{ClaimKey, DeliveryId, NotificationId};

/// 状态变化只发布脱敏 ID，不携带正文、密钥或渠道原始响应。
#[async_trait::async_trait]
pub trait EventSink: Send + Sync {
    async fn notification_changed(&self, notification_id: &NotificationId);

    async fn delivery_changed(&self, delivery_id: &DeliveryId);

    async fn reply_changed(&self, claim_key: &ClaimKey);

    async fn runtime_stopped(&self);
}
