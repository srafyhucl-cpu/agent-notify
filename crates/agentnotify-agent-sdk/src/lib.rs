//! Agent 适配器协议与注册边界。

/// Agent 适配器只通过稳定名称和能力信息暴露自身。
pub trait AgentAdapterBoundary: Send + Sync {
    fn adapter_name(&self) -> &'static str;

    fn supports_resume(&self) -> bool;
}
