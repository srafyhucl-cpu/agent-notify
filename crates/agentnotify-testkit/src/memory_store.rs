use std::collections::{BTreeMap, HashMap};

use agentnotify_application::{
    ChannelAccountStore, ClaimStore, DeliveryRecord, DeliveryStore, EventSink, IngestStore,
    OutboxItem, OutboxLease, OutboxState, RouteStore, SecretError, SecretKind, SecretStore,
    SecretValue, StatusSnapshot, StatusStore, StoreError,
};
use agentnotify_channel_sdk::{ChannelAccount, DeliveryReceipt};
use agentnotify_domain::{
    AgentId, AgentSessionId, ChannelAccountId, ChannelId, ClaimKey, ClaimOutcome, Delivery,
    DeliveryId, InboundClaim, Notification, NotificationId, ReplyRoute, RouteKey, Timestamp,
};
use tokio::sync::RwLock;

#[derive(Default)]
pub struct MemoryStore {
    notifications: RwLock<HashMap<NotificationId, Notification>>,
    outbox: RwLock<BTreeMap<String, OutboxItem>>,
    deliveries: RwLock<HashMap<DeliveryId, DeliveryRecord>>,
    routes: RwLock<HashMap<RouteKey, ReplyRoute>>,
    claims: RwLock<HashMap<ClaimKey, InboundClaim>>,
    accounts: RwLock<HashMap<ChannelAccountId, ChannelAccount>>,
    secrets: RwLock<HashMap<(ChannelAccountId, SecretKind), SecretValue>>,
    status: RwLock<StatusSnapshot>,
    events: RwLock<Vec<String>>,
    channel_receipts: RwLock<HashMap<ChannelAccountId, DeliveryReceipt>>,
}

impl MemoryStore {
    pub async fn notification_count(&self) -> usize {
        self.notifications.read().await.len()
    }

    pub async fn outbox_count(&self) -> usize {
        self.outbox.read().await.len()
    }

    pub async fn notification(&self, id: NotificationId) -> Option<Notification> {
        self.notifications.read().await.get(&id).cloned()
    }

    pub async fn delivery(&self, id: &DeliveryId) -> Option<DeliveryRecord> {
        self.deliveries.read().await.get(id).cloned()
    }

    pub async fn route(&self, key: &RouteKey) -> Option<ReplyRoute> {
        self.routes.read().await.get(key).cloned()
    }

    pub async fn set_channel_receipt(
        &self,
        account_id: ChannelAccountId,
        receipt: DeliveryReceipt,
    ) {
        self.channel_receipts
            .write()
            .await
            .insert(account_id, receipt);
    }

    pub async fn channel_receipt(&self, account_id: &ChannelAccountId) -> Option<DeliveryReceipt> {
        self.channel_receipts.read().await.get(account_id).cloned()
    }

    pub async fn secret_value(
        &self,
        account_id: &ChannelAccountId,
        kind: SecretKind,
    ) -> Option<SecretValue> {
        self.secrets
            .read()
            .await
            .get(&(account_id.clone(), kind))
            .cloned()
    }

    pub async fn events(&self) -> Vec<String> {
        self.events.read().await.clone()
    }
}

#[async_trait::async_trait]
impl IngestStore for MemoryStore {
    async fn commit_ingest(
        &self,
        notification: Notification,
        outbox: Vec<OutboxItem>,
    ) -> Result<(), StoreError> {
        let mut notifications = self.notifications.write().await;
        let mut outbox_store = self.outbox.write().await;
        if notifications.contains_key(&notification.id) {
            return Err(StoreError::conflict("notification_exists", "通知已存在"));
        }
        for item in &outbox {
            if outbox_store.contains_key(&item.id) {
                return Err(StoreError::conflict("outbox_exists", "投递任务已存在"));
            }
        }
        notifications.insert(notification.id.clone(), notification);
        for item in outbox {
            outbox_store.insert(item.id.clone(), item);
        }
        Ok(())
    }

    async fn notification_by_ingest_key(
        &self,
        agent_id: &AgentId,
        ingest_key: &str,
    ) -> Result<Option<Notification>, StoreError> {
        Ok(self
            .notifications
            .read()
            .await
            .values()
            .find(|notification| {
                &notification.agent_id == agent_id && notification.ingest_key == ingest_key
            })
            .cloned())
    }

    async fn recent_notification_at(
        &self,
        agent_id: &AgentId,
        session_id: &AgentSessionId,
    ) -> Result<Option<Timestamp>, StoreError> {
        Ok(self
            .notifications
            .read()
            .await
            .values()
            .filter(|notification| {
                &notification.agent_id == agent_id
                    && notification.session_id.as_ref() == Some(session_id)
            })
            .map(|notification| notification.occurred_at)
            .max())
    }
}

#[async_trait::async_trait]
impl DeliveryStore for MemoryStore {
    async fn lease_next_outbox(
        &self,
        now: Timestamp,
        lease_until: Timestamp,
    ) -> Result<Option<OutboxLease>, StoreError> {
        let mut outbox = self.outbox.write().await;
        let candidate_id = outbox
            .values()
            .filter(|item| item.state == OutboxState::Pending && item.available_at <= now)
            .min_by(|left, right| {
                (left.available_at, left.id.as_str()).cmp(&(right.available_at, right.id.as_str()))
            })
            .map(|item| item.id.clone());
        let Some(candidate_id) = candidate_id else {
            return Ok(None);
        };
        let item = outbox
            .get_mut(&candidate_id)
            .ok_or_else(|| StoreError::unavailable("内存 Outbox 状态不一致"))?;
        item.state = OutboxState::Leased;
        item.attempt_count = item.attempt_count.saturating_add(1);
        let notification = self
            .notifications
            .read()
            .await
            .get(&item.notification_id)
            .cloned()
            .ok_or_else(|| StoreError::not_found("notification_missing", "找不到通知"))?;
        Ok(Some(OutboxLease {
            outbox: item.clone(),
            notification,
            owner: "memory-worker".into(),
            lease_until,
        }))
    }

    async fn commit_delivery(
        &self,
        lease: OutboxLease,
        delivery: Delivery,
        route: Option<ReplyRoute>,
    ) -> Result<(), StoreError> {
        if lease.outbox.state != OutboxState::Leased {
            return Err(StoreError::conflict(
                "outbox_not_leased",
                "Outbox 未被当前任务租用",
            ));
        }
        if let Some(route) = route.as_ref() {
            let mut routes = self.routes.write().await;
            if let Some(existing) = routes.get(&route.key) {
                if existing != route {
                    return Err(StoreError::conflict(
                        "route_conflict",
                        "回复路由已存在且内容不同",
                    ));
                }
            }
            routes.insert(route.key.clone(), route.clone());
        }
        let state = match delivery.state() {
            agentnotify_domain::DeliveryState::Sent
            | agentnotify_domain::DeliveryState::Skipped => OutboxState::Done,
            agentnotify_domain::DeliveryState::Failed => OutboxState::Dead,
            agentnotify_domain::DeliveryState::Unknown => OutboxState::Unknown,
            agentnotify_domain::DeliveryState::Pending => {
                return Err(StoreError::conflict("delivery_pending", "投递尚未完成"));
            }
        };
        if let Some(item) = self.outbox.write().await.get_mut(&lease.outbox.id) {
            item.state = state;
            item.last_error = delivery.error().cloned();
        }
        let external_message_id = delivery.external_message_id().cloned();
        self.deliveries.write().await.insert(
            delivery.id().clone(),
            DeliveryRecord {
                delivery,
                external_message_id,
            },
        );
        Ok(())
    }

    async fn reschedule_outbox(
        &self,
        lease: OutboxLease,
        delivery: Delivery,
        next_attempt_at: Timestamp,
    ) -> Result<(), StoreError> {
        if delivery.state() != agentnotify_domain::DeliveryState::Failed {
            return Err(StoreError::conflict(
                "delivery_not_failed",
                "只有失败投递可以重试",
            ));
        }
        let mut outbox = self.outbox.write().await;
        let item = outbox
            .get_mut(&lease.outbox.id)
            .ok_or_else(|| StoreError::not_found("outbox_missing", "找不到 Outbox"))?;
        item.state = OutboxState::Pending;
        item.available_at = next_attempt_at;
        item.last_error = delivery.error().cloned();
        Ok(())
    }
}

#[async_trait::async_trait]
impl RouteStore for MemoryStore {
    async fn find_route(
        &self,
        key: &RouteKey,
        now: Timestamp,
    ) -> Result<Option<ReplyRoute>, StoreError> {
        Ok(self
            .routes
            .read()
            .await
            .get(key)
            .filter(|route| route.is_active(now).is_ok())
            .cloned())
    }

    async fn insert_route(&self, route: ReplyRoute) -> Result<(), StoreError> {
        let mut routes = self.routes.write().await;
        if let Some(existing) = routes.get(&route.key) {
            if existing != &route {
                return Err(StoreError::conflict("route_conflict", "回复路由冲突"));
            }
        }
        routes.insert(route.key.clone(), route);
        Ok(())
    }
}

#[async_trait::async_trait]
impl ClaimStore for MemoryStore {
    async fn claim(&self, claim: InboundClaim) -> Result<ClaimOutcome, StoreError> {
        let mut claims = self.claims.write().await;
        if let Some(existing) = claims.get(&claim.key) {
            return Ok(ClaimOutcome::AlreadyClaimed {
                state: existing.state,
                updated_at: existing.updated_at,
            });
        }
        let key = claim.key.clone();
        claims.insert(key, claim.clone());
        Ok(ClaimOutcome::Acquired(claim))
    }

    async fn update_claim(&self, claim: InboundClaim) -> Result<(), StoreError> {
        let mut claims = self.claims.write().await;
        if !claims.contains_key(&claim.key) {
            return Err(StoreError::not_found("claim_missing", "找不到入站 Claim"));
        }
        claims.insert(claim.key.clone(), claim);
        Ok(())
    }

    async fn find_claim(&self, key: &ClaimKey) -> Result<Option<InboundClaim>, StoreError> {
        Ok(self.claims.read().await.get(key).cloned())
    }
}

#[async_trait::async_trait]
impl ChannelAccountStore for MemoryStore {
    async fn get(
        &self,
        account_id: &ChannelAccountId,
    ) -> Result<Option<ChannelAccount>, StoreError> {
        Ok(self.accounts.read().await.get(account_id).cloned())
    }

    async fn list(&self, channel_id: &ChannelId) -> Result<Vec<ChannelAccount>, StoreError> {
        let mut accounts = self
            .accounts
            .read()
            .await
            .values()
            .filter(|account| &account.channel_id == channel_id)
            .cloned()
            .collect::<Vec<_>>();
        accounts.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(accounts)
    }

    async fn upsert(&self, account: ChannelAccount) -> Result<(), StoreError> {
        self.accounts
            .write()
            .await
            .insert(account.id.clone(), account);
        Ok(())
    }

    async fn set_enabled(
        &self,
        account_id: &ChannelAccountId,
        enabled: bool,
    ) -> Result<(), StoreError> {
        let mut accounts = self.accounts.write().await;
        let account = accounts
            .get_mut(account_id)
            .ok_or_else(|| StoreError::not_found("account_missing", "找不到渠道账号"))?;
        account.enabled = enabled;
        Ok(())
    }
}

#[async_trait::async_trait]
impl SecretStore for MemoryStore {
    async fn get(
        &self,
        account_id: &ChannelAccountId,
        kind: SecretKind,
    ) -> Result<SecretValue, SecretError> {
        self.secrets
            .read()
            .await
            .get(&(account_id.clone(), kind))
            .cloned()
            .ok_or_else(|| SecretError::new("secret_not_found", "找不到密钥"))
    }

    async fn set(
        &self,
        account_id: &ChannelAccountId,
        kind: SecretKind,
        value: SecretValue,
    ) -> Result<(), SecretError> {
        self.secrets
            .write()
            .await
            .insert((account_id.clone(), kind), value);
        Ok(())
    }

    async fn delete(
        &self,
        account_id: &ChannelAccountId,
        kind: SecretKind,
    ) -> Result<(), SecretError> {
        self.secrets
            .write()
            .await
            .remove(&(account_id.clone(), kind));
        Ok(())
    }
}

#[async_trait::async_trait]
impl StatusStore for MemoryStore {
    async fn snapshot(&self) -> Result<StatusSnapshot, StoreError> {
        let mut snapshot = self.status.read().await.clone();
        snapshot.notification_count = self.notification_count().await as u64;
        snapshot.delivery_count = self.deliveries.read().await.len() as u64;
        snapshot.pending_outbox_count = self
            .outbox
            .read()
            .await
            .values()
            .filter(|item| item.state == OutboxState::Pending)
            .count() as u64;
        Ok(snapshot)
    }

    async fn record_error(&self, error: agentnotify_domain::SafeError) -> Result<(), StoreError> {
        self.status.write().await.recent_error = Some(error);
        Ok(())
    }
}

#[async_trait::async_trait]
impl EventSink for MemoryStore {
    async fn notification_changed(&self, notification_id: &NotificationId) {
        self.events
            .write()
            .await
            .push(format!("notification:{notification_id}"));
    }

    async fn delivery_changed(&self, delivery_id: &DeliveryId) {
        self.events
            .write()
            .await
            .push(format!("delivery:{delivery_id}"));
    }

    async fn reply_changed(&self, claim_key: &ClaimKey) {
        self.events.write().await.push(format!("reply:{claim_key}"));
    }

    async fn runtime_stopped(&self) {
        self.events.write().await.push("runtime.stopped".into());
    }
}
