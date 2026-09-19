//! SQLite 持久化适配器层，对外只实现应用层定义的仓储端口。

mod channel_account_store;
mod claim_store;
mod database;
mod delivery_store;
mod ingest_store;
mod migrations;
mod recovery;
mod route_store;
mod row_codec;
mod sqlite_helpers;
mod status_store;

pub use migrations::{SqliteStore, run_migrations};
pub use recovery::RecoverySummary;

use agentnotify_domain::DomainArea;

/// 存储适配器声明它能够持久化的领域区域。
pub trait StorageCapability: Send + Sync {
    fn supports(&self, area: DomainArea) -> bool;
}
