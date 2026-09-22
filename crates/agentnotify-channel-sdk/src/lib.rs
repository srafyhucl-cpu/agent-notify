//! 渠道适配器协议、账号隔离和共享契约测试。

mod account;
mod adapter;
mod contract;
mod descriptor;
mod login;
mod registry;

pub use account::{ChannelAccount, ChannelHealth, SecretRef};
pub use adapter::{
    ChannelAdapter, ChannelError, ChannelTask, DeliveryReceipt, InboundEmitter, MessagePurpose,
    NotificationPresentation, OutboundMessage,
};
pub use contract::{assert_channel_contract, assert_channel_login_contract};
pub use descriptor::{ChannelCapabilities, ChannelDescriptor, InboundMode};
pub use login::{
    BeginLoginRequest, ChannelLoginAdapter, LoginSession, LoginSessionId, LoginSessionState,
};
pub use registry::{ChannelRegistry, ChannelRegistryError};
