//! 应用用例与端口层，只编排领域规则，不直接访问具体基础设施。

use agentnotify_domain::DomainArea;

/// 每个应用用例都声明所属领域区域和稳定名称，便于监督与诊断。
pub trait UseCase: Send + Sync {
    fn area(&self) -> DomainArea;

    fn name(&self) -> &'static str;
}
