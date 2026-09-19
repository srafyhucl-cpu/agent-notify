//! 渠道适配器协议与账号隔离边界。

/// 渠道适配器通过稳定名称声明其发送与回复路由能力。
pub trait ChannelAdapterBoundary: Send + Sync {
    fn adapter_name(&self) -> &'static str;

    fn supports_reply_routing(&self) -> bool;
}
