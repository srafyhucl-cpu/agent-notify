use std::path::Path;

use agentnotify_application::StoreError;
use agentnotify_domain::Timestamp;
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};

use crate::database::Database;

const MIGRATION_0001: &str = include_str!("../migrations/0001_init.sql");
const MIGRATION_0001_VERSION: i64 = 1;

/// SQLite 存储适配器。数据库连接由专用调度线程独占。
#[derive(Clone)]
pub struct SqliteStore {
    database: Database,
}

impl SqliteStore {
    /// 打开数据库并在返回前完成全部待应用迁移。
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        Ok(Self {
            database: Database::open(path)?,
        })
    }

    pub(crate) async fn run<T, F>(&self, operation: F) -> Result<T, StoreError>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T, StoreError> + Send + 'static,
    {
        self.database.run(operation).await
    }

    pub async fn schema_version(&self) -> Result<i64, StoreError> {
        self.run(|connection| {
            connection
                .query_row(
                    "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
                    [],
                    |row| row.get(0),
                )
                .map_err(|error| storage_error("读取数据库版本失败", error))
        })
        .await
    }

    pub async fn journal_mode(&self) -> Result<String, StoreError> {
        self.run(|connection| {
            connection
                .query_row("PRAGMA journal_mode", [], |row| row.get(0))
                .map_err(|error| storage_error("读取 SQLite journal_mode 失败", error))
        })
        .await
    }

    pub async fn table_exists(&self, table: &str) -> Result<bool, StoreError> {
        let table = table.to_owned();
        self.run(move |connection| {
            connection
                .query_row(
                    "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
                    [table],
                    |_| Ok(()),
                )
                .optional()
                .map(|value| value.is_some())
                .map_err(|error| storage_error("检查数据库表失败", error))
        })
        .await
    }
}

/// 按顺序执行尚未应用的迁移。已记录版本的 SQL 内容不可修改。
pub fn run_migrations(connection: &mut Connection) -> Result<(), StoreError> {
    let migration_table_exists = connection
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'schema_migrations'",
            [],
            |_| Ok(()),
        )
        .optional()
        .map_err(|error| storage_error("检查迁移表失败", error))?
        .is_some();

    let checksum = sha256_hex(MIGRATION_0001.as_bytes());
    let transaction = connection
        .transaction()
        .map_err(|error| storage_error("开启迁移事务失败", error))?;

    if migration_table_exists {
        let existing = transaction
            .query_row(
                "SELECT checksum FROM schema_migrations WHERE version = ?1",
                params![MIGRATION_0001_VERSION],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| storage_error("读取迁移 checksum 失败", error))?;

        if let Some(existing) = existing {
            if existing != checksum {
                return Err(StoreError::new(
                    "migration_checksum_mismatch",
                    "数据库迁移校验失败，现有版本与程序内置迁移不一致",
                ));
            }
            return Ok(());
        }
    }

    transaction
        .execute_batch(MIGRATION_0001)
        .map_err(|error| storage_error("执行初始迁移失败", error))?;
    transaction
        .execute(
            "INSERT INTO schema_migrations(version, checksum, applied_at) VALUES (?1, ?2, ?3)",
            params![
                MIGRATION_0001_VERSION,
                checksum,
                Timestamp::now_utc().to_rfc3339()
            ],
        )
        .map_err(|error| storage_error("记录迁移版本失败", error))?;
    transaction
        .commit()
        .map_err(|error| storage_error("提交迁移事务失败", error))?;
    Ok(())
}

pub(crate) fn configure_connection(connection: &Connection) -> Result<(), StoreError> {
    for pragma in [
        "PRAGMA journal_mode = WAL;",
        "PRAGMA foreign_keys = ON;",
        "PRAGMA busy_timeout = 5000;",
        "PRAGMA synchronous = NORMAL;",
    ] {
        connection
            .execute_batch(pragma)
            .map_err(|error| storage_error("配置 SQLite 连接失败", error))?;
    }
    Ok(())
}

pub(crate) fn storage_error(context: &str, error: rusqlite::Error) -> StoreError {
    let _ = error;
    StoreError::new("sqlite_error", context)
}

fn sha256_hex(value: &[u8]) -> String {
    let digest = Sha256::digest(value);
    let mut text = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write;
        let _ = write!(text, "{byte:02x}");
    }
    text
}
