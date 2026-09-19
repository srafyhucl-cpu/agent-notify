//! 纯领域规则层，不依赖 I/O、数据库或操作系统 API。

mod delivery;
mod error;
mod identifier;
mod inbound;
mod notification;
mod routing;
mod timestamp;

pub use delivery::{Delivery, DeliveryErrorKind, DeliveryReceiptState, DeliveryState, SafeError};
pub use error::DomainError;
pub use identifier::{
    AgentId, AgentSessionId, ChannelAccountId, ChannelId, DeliveryId, ExternalMessageId,
    InboundMessageId, NotificationId, RequestId,
};
pub use inbound::InboundMessageInput;
pub use inbound::{ClaimKey, ClaimOutcome, ClaimState, InboundClaim, InboundMessage};
pub use notification::{Notification, NotificationMetadata};
pub use routing::{ReplyRoute, RouteKey};
pub use timestamp::Timestamp;

/// 领域规则覆盖的稳定业务区域。
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum DomainArea {
    Ingest,
    Delivery,
    Routing,
    Reply,
    Status,
}

/// 领域规则只暴露业务标识，具体执行由上层用例编排。
pub trait DomainRule: Send + Sync {
    fn area(&self) -> DomainArea;

    fn code(&self) -> &'static str;
}
