//! 纯领域规则层，不依赖 I/O、数据库或操作系统 API。

/// 领域规则覆盖的稳定业务区域。
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum DomainArea {
    Ingest,
    Delivery,
    Routing,
    Reply,
}

/// 领域规则只暴露业务标识，具体执行由上层用例编排。
pub trait DomainRule: Send + Sync {
    fn area(&self) -> DomainArea;

    fn code(&self) -> &'static str;
}
