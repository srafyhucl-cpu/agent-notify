//! SQLite 持久化适配器层，对外只实现应用层定义的仓储端口。

mod migrations;

pub use migrations::{SqliteStore, run_migrations};

use agentnotify_domain::DomainArea;

/// 存储适配器声明它能够持久化的领域区域。
pub trait StorageCapability: Send + Sync {
    fn supports(&self, area: DomainArea) -> bool;
}
