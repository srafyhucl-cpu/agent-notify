//! Agent 适配器协议、注册表与共享契约测试。

mod adapter;
mod contract;
mod descriptor;
mod registry;

pub use adapter::{
    AgentAdapter, AgentError, AgentEventEnvelope, NormalizedAgentEvent, ResumeReceipt,
};
pub use contract::assert_agent_contract;
pub use descriptor::{AgentCapabilities, AgentDescriptor, AgentHealth};
pub use registry::{AgentRegistry, AgentRegistryError};
