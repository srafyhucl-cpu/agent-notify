use std::{fmt::Display, sync::Arc};

use agentnotify_channel_sdk::{
    ChannelAccount, ChannelError, ChannelRegistry, DeliveryReceipt, OutboundMessage,
};
use agentnotify_domain::{
    Delivery, DeliveryErrorKind, DeliveryId, DeliveryState, DomainArea, Notification, ReplyRoute,
    RouteKey, SafeError, Timestamp,
};
use time::Duration;

use crate::retry::RetryPolicy;
use crate::{Clock, DeliveryStore, EventSink, IdGenerator, OutboxLease, StoreError, UseCase};

const DEFAULT_REPLY_ROUTE_TTL_SECONDS: i64 = 24 * 60 * 60;
const DEFAULT_LEASE_SECONDS: i64 = 30;

/// 首个生产闭环显式配置的渠道目标。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeliveryTarget {
    pub account: ChannelAccount,
    pub conversation_id: String,
}

impl DeliveryTarget {
    pub fn new(account: ChannelAccount, conversation_id: impl Into<String>) -> Self {
        Self {
            account,
            conversation_id: conversation_id.into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProcessOutcome {
    Idle,
    Completed {
        delivery_id: DeliveryId,
    },
    Rescheduled {
        delivery_id: DeliveryId,
        next_attempt_at: Timestamp,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DeliveryError {
    Store(StoreError),
    Channel(ChannelError),
    NoTarget,
    InvalidIdentifier,
    InvalidRouteWindow,
}

impl DeliveryError {
    pub fn code(&self) -> &str {
        match self {
            Self::Store(error) => error.code(),
            Self::Channel(error) => error.code(),
            Self::NoTarget => "delivery_no_target",
            Self::InvalidIdentifier => "delivery_invalid_identifier",
            Self::InvalidRouteWindow => "delivery_invalid_route_window",
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Self::Store(error) => error.message(),
            Self::Channel(error) => error.message(),
            Self::NoTarget => "没有可用的渠道账号，无法投递通知",
            Self::InvalidIdentifier => "投递标识生成失败",
            Self::InvalidRouteWindow => "回复路由时间窗口无效",
        }
    }
}

impl Display for DeliveryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.message())
    }
}

impl std::error::Error for DeliveryError {}

impl From<StoreError> for DeliveryError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}

impl From<ChannelError> for DeliveryError {
    fn from(value: ChannelError) -> Self {
        Self::Channel(value)
    }
}

/// 单次 Outbox 投递服务。
pub struct DeliveryService {
    delivery_store: Arc<dyn DeliveryStore>,
    channels: Arc<ChannelRegistry>,
    targets: Vec<DeliveryTarget>,
    clock: Arc<dyn Clock>,
    id_generator: Arc<dyn IdGenerator>,
    event_sink: Arc<dyn EventSink>,
    retry_policy: RetryPolicy,
    reply_route_ttl: Duration,
    lease_duration: Duration,
}

impl DeliveryService {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        delivery_store: Arc<dyn DeliveryStore>,
        channels: Arc<ChannelRegistry>,
        targets: Vec<DeliveryTarget>,
        clock: Arc<dyn Clock>,
        id_generator: Arc<dyn IdGenerator>,
        event_sink: Arc<dyn EventSink>,
        retry_policy: RetryPolicy,
    ) -> Self {
        Self {
            delivery_store,
            channels,
            targets,
            clock,
            id_generator,
            event_sink,
            retry_policy,
            reply_route_ttl: Duration::seconds(DEFAULT_REPLY_ROUTE_TTL_SECONDS),
            lease_duration: Duration::seconds(DEFAULT_LEASE_SECONDS),
        }
    }

    pub fn with_route_ttl(mut self, reply_route_ttl: Duration) -> Self {
        self.reply_route_ttl = reply_route_ttl;
        self
    }

    pub async fn process_next(&self) -> Result<ProcessOutcome, DeliveryError> {
        let now = self.clock.now();
        let lease_until = now
            .checked_add(self.lease_duration)
            .ok_or(DeliveryError::InvalidRouteWindow)?;
        let Some(lease) = self
            .delivery_store
            .lease_next_outbox(now, lease_until)
            .await?
        else {
            return Ok(ProcessOutcome::Idle);
        };
        let target = self
            .targets
            .iter()
            .find(|target| target.account.enabled)
            .cloned()
            .ok_or(DeliveryError::NoTarget)?;
        let channel = self
            .channels
            .get(&target.account.channel_id)
            .ok_or(DeliveryError::NoTarget)?;
        let capabilities = channel.capabilities();
        let delivery_id = DeliveryId::new(self.id_generator.next_id())
            .map_err(|_| DeliveryError::InvalidIdentifier)?;
        let mut delivery = Delivery::pending(
            delivery_id.clone(),
            lease.notification.id.clone(),
            target.account.channel_id.clone(),
            target.account.id.clone(),
        );
        let text = format!(
            "{}\n\n{}",
            lease.notification.title, lease.notification.body
        );
        let message = OutboundMessage::notification(
            target.conversation_id.clone(),
            text,
            format!("delivery-{delivery_id}"),
        )?;

        let mut next_attempt_at = None;
        match channel.send(target.account.clone(), message).await {
            Ok(receipt) => {
                self.apply_receipt(&lease, &target, &mut delivery, receipt, &capabilities)
                    .await?;
            }
            Err(error) => {
                next_attempt_at = self
                    .apply_channel_error(&lease, &mut delivery, error)
                    .await?;
            }
        }

        self.event_sink.delivery_changed(&delivery_id).await;
        if let Some(next_attempt_at) = next_attempt_at {
            Ok(ProcessOutcome::Rescheduled {
                delivery_id,
                next_attempt_at,
            })
        } else {
            Ok(ProcessOutcome::Completed { delivery_id })
        }
    }

    async fn apply_receipt(
        &self,
        lease: &OutboxLease,
        target: &DeliveryTarget,
        delivery: &mut Delivery,
        receipt: DeliveryReceipt,
        capabilities: &agentnotify_channel_sdk::ChannelCapabilities,
    ) -> Result<(), DeliveryError> {
        if !receipt.is_valid_for(capabilities) {
            delivery
                .mark_unknown(safe_error(
                    "channel_invalid_receipt",
                    "渠道返回了无法确认的投递回执",
                )?)
                .map_err(|_| {
                    DeliveryError::Channel(ChannelError::unknown(
                        "channel_invalid_receipt",
                        "渠道返回了无法确认的投递回执",
                    ))
                })?;
            self.delivery_store
                .commit_delivery(lease.clone(), delivery.clone(), None)
                .await?;
            return Ok(());
        }

        match receipt.state {
            DeliveryState::Sent => {
                if let Some(external_message_id) = receipt.external_message_id {
                    delivery
                        .mark_sent(external_message_id)
                        .map_err(|_| DeliveryError::InvalidIdentifier)?;
                    let route = self.build_route(lease.notification.clone(), target, delivery)?;
                    self.delivery_store
                        .commit_delivery(lease.clone(), delivery.clone(), route)
                        .await?;
                } else {
                    delivery
                        .mark_unknown(safe_error(
                            "channel_missing_message_id",
                            "渠道未返回消息标识，无法建立精确回复路由",
                        )?)
                        .map_err(|_| DeliveryError::InvalidIdentifier)?;
                    self.delivery_store
                        .commit_delivery(lease.clone(), delivery.clone(), None)
                        .await?;
                }
            }
            DeliveryState::Unknown => {
                delivery
                    .mark_unknown(receipt.error.unwrap_or_else(|| {
                        safe_error("channel_unknown", "渠道结果无法确认")
                            .expect("内置安全错误必须有效")
                    }))
                    .map_err(|_| DeliveryError::InvalidIdentifier)?;
                self.delivery_store
                    .commit_delivery(lease.clone(), delivery.clone(), None)
                    .await?;
            }
            DeliveryState::Skipped => {
                delivery
                    .mark_skipped(receipt.error.unwrap_or_else(|| {
                        safe_error("channel_skipped", "渠道跳过了本次投递")
                            .expect("内置安全错误必须有效")
                    }))
                    .map_err(|_| DeliveryError::InvalidIdentifier)?;
                self.delivery_store
                    .commit_delivery(lease.clone(), delivery.clone(), None)
                    .await?;
            }
            DeliveryState::Pending | DeliveryState::Failed => {
                return Err(DeliveryError::Channel(ChannelError::unknown(
                    "channel_invalid_receipt",
                    "渠道返回了非终态回执",
                )));
            }
        }
        Ok(())
    }

    async fn apply_channel_error(
        &self,
        lease: &OutboxLease,
        delivery: &mut Delivery,
        error: ChannelError,
    ) -> Result<Option<Timestamp>, DeliveryError> {
        let (safe_error, retry_after) = channel_error_parts(&error);
        if error.is_retryable() {
            delivery
                .mark_retryable(safe_error)
                .map_err(|_| DeliveryError::InvalidIdentifier)?;
            if let Some(next_attempt_at) = self.retry_policy.next_attempt_with_retry_after(
                lease.outbox.attempt_count,
                DeliveryErrorKind::Retryable,
                self.clock.now(),
                retry_after,
            ) {
                self.delivery_store
                    .reschedule_outbox(lease.clone(), delivery.clone(), next_attempt_at)
                    .await?;
                return Ok(Some(next_attempt_at));
            }
            delivery
                .mark_permanent_failure(safe_error_from_channel_error(&error))
                .map_err(|_| DeliveryError::InvalidIdentifier)?;
            self.delivery_store
                .commit_delivery(lease.clone(), delivery.clone(), None)
                .await?;
            return Ok(None);
        }

        let safe = safe_error_from_channel_error(&error);
        if matches!(error, ChannelError::Unknown(_)) {
            delivery
                .mark_unknown(safe)
                .map_err(|_| DeliveryError::InvalidIdentifier)?;
        } else {
            delivery
                .mark_permanent_failure(safe)
                .map_err(|_| DeliveryError::InvalidIdentifier)?;
        }
        self.delivery_store
            .commit_delivery(lease.clone(), delivery.clone(), None)
            .await?;
        Ok(None)
    }

    fn build_route(
        &self,
        notification: Notification,
        target: &DeliveryTarget,
        delivery: &Delivery,
    ) -> Result<Option<ReplyRoute>, DeliveryError> {
        let Some(session_id) = notification.session_id else {
            return Ok(None);
        };
        let Some(external_message_id) = delivery.external_message_id().cloned() else {
            return Ok(None);
        };
        let created_at = self.clock.now();
        let expires_at = created_at
            .checked_add(self.reply_route_ttl)
            .ok_or(DeliveryError::InvalidRouteWindow)?;
        Ok(Some(ReplyRoute::new(
            RouteKey::new(
                target.account.channel_id.clone(),
                target.account.id.clone(),
                external_message_id,
            ),
            notification.agent_id,
            session_id,
            created_at,
            expires_at,
        )))
    }
}

impl UseCase for DeliveryService {
    fn area(&self) -> DomainArea {
        DomainArea::Delivery
    }

    fn name(&self) -> &'static str {
        "delivery"
    }
}

fn channel_error_parts(error: &ChannelError) -> (SafeError, Option<Duration>) {
    match error {
        ChannelError::Permanent(error)
        | ChannelError::Unknown(error)
        | ChannelError::InvalidAccount(error)
        | ChannelError::UnsupportedCapability(error) => (error.clone(), None),
        ChannelError::Retryable { error, retry_after } => (error.clone(), *retry_after),
    }
}

fn safe_error_from_channel_error(error: &ChannelError) -> SafeError {
    channel_error_parts(error).0
}

fn safe_error(code: &str, message: &str) -> Result<SafeError, DeliveryError> {
    SafeError::new(code, message).map_err(|_| DeliveryError::InvalidIdentifier)
}
