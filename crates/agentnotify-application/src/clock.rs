use agentnotify_domain::Timestamp;

/// 应用服务使用的可注入时钟。
pub trait Clock: Send + Sync {
    fn now(&self) -> Timestamp;
}

/// 应用服务使用的稳定 ID 生成器。
pub trait IdGenerator: Send + Sync {
    fn next_id(&self) -> String;
}
