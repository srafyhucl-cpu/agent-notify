//! 假适配器与契约测试工具，只依赖核心边界，不访问真实网络或用户目录。

/// 可复用的测试场景描述，供后续假 Agent 与假渠道夹具扩展。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TestScenario {
    name: String,
}

impl TestScenario {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}
