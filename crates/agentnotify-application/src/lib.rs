//! 应用用例与端口层，只编排领域规则，不直接访问具体基础设施。

mod clock;
mod delivery;
mod error;
mod ingest;
mod observability;
mod policy;
pub mod ports;
mod reply;
mod retry;
mod status;

use agentnotify_domain::DomainArea;

pub use clock::{Clock, IdGenerator};
pub use delivery::{DeliveryError, DeliveryService, DeliveryTarget, ProcessOutcome};
pub use error::{ApplicationError, StoreError};
pub use ingest::{IngestError, IngestResult, IngestService};
pub use policy::{
    AgentNotificationConfig, NotificationPolicy, PolicyDecision, PolicyInput, QuietHours,
    SkipReason,
};
pub use ports::{
    ChannelAccountStore, ClaimStore, DeliveryRecord, DeliveryStore, EventSink, IngestStore,
    OutboxItem, OutboxLease, OutboxState, RouteStore, SecretError, SecretKind, SecretStore,
    SecretValue, StatusSnapshot, StatusStore,
};
pub use reply::{ReplyConfig, ReplyError, ReplyOutcome, ReplyRejection, ReplyService, ReplyTarget};
pub use retry::RetryPolicy;
pub use status::{AgentStatus, ChannelAccountStatus, StatusError, StatusOverview, StatusService};

/// 每个应用用例都声明所属领域区域和稳定名称，便于监督与诊断。
pub trait UseCase: Send + Sync {
    fn area(&self) -> DomainArea;

    fn name(&self) -> &'static str;
}
