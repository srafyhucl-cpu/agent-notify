use std::collections::BTreeMap;
use std::path::Path;

use agentnotify_application::StoreError;
use agentnotify_domain::Timestamp;
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};

use crate::database::Database;
use crate::row_codec::timestamp_to_db;

struct Migration {
    version: i64,
    sql: &'static str,
}

const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        sql: include_str!("../migrations/0001_init.sql"),
    },
    Migration {
        version: 2,
        sql: include_str!("../migrations/0002_canonical_timestamps.sql"),
    },
];

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

    pub async fn integrity_check(&self) -> Result<bool, StoreError> {
        self.run(|connection| {
            connection
                .query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0))
                .map(|result| result == "ok")
                .map_err(|error| storage_error("检查 SQLite 完整性失败", error))
        })
        .await
    }

    pub async fn wal_checkpoint_truncate(&self) -> Result<(), StoreError> {
        self.run(|connection| {
            connection
                .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
                .map_err(|error| storage_error("执行 SQLite WAL checkpoint 失败", error))
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

    let transaction = connection
        .transaction()
        .map_err(|error| storage_error("开启迁移事务失败", error))?;

    let mut applied = BTreeMap::new();
    if migration_table_exists {
        let mut statement = transaction
            .prepare("SELECT version, checksum FROM schema_migrations ORDER BY version")
            .map_err(|error| storage_error("读取迁移历史失败", error))?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|error| storage_error("读取迁移历史失败", error))?;
        for row in rows {
            let (version, checksum) =
                row.map_err(|error| storage_error("读取迁移历史失败", error))?;
            applied.insert(version, checksum);
        }
    }

    if migration_table_exists && applied.is_empty() {
        return Err(StoreError::new(
            "migration_history_missing",
            "数据库迁移历史为空，拒绝在未知状态上继续迁移",
        ));
    }

    for (version, checksum) in &applied {
        let Some(migration) = MIGRATIONS.iter().find(|item| item.version == *version) else {
            return Err(StoreError::new(
                "migration_version_unsupported",
                "数据库包含当前程序无法识别的迁移版本，请先核对数据库完整性",
            ));
        };
        let expected = migration_checksum(migration.sql);
        if *checksum == expected {
            continue;
        }
        // 旧版本按原始字节记录校验和，同一份迁移在 CRLF 检出与 LF 检出下结果不同。
        // 只有内容等价、仅行尾不同的记录才自愈；其它差异继续报错。
        if legacy_byte_checksums(migration.sql).contains(checksum) {
            tracing::warn!(
                version = migration.version,
                "数据库迁移校验和仅行尾不同，已自愈为归一化校验和"
            );
            transaction
                .execute(
                    "UPDATE schema_migrations SET checksum = ?1 WHERE version = ?2",
                    params![expected, migration.version],
                )
                .map_err(|error| storage_error("自愈迁移校验和失败", error))?;
            continue;
        }
        return Err(StoreError::new(
            "migration_checksum_mismatch",
            "数据库迁移校验失败，现有版本与程序内置迁移不一致",
        ));
    }

    for migration in MIGRATIONS {
        if applied.contains_key(&migration.version) {
            continue;
        }
        if applied.keys().any(|version| *version > migration.version) {
            return Err(StoreError::new(
                "migration_history_gap",
                "数据库迁移历史不连续，拒绝自动修复",
            ));
        }

        transaction.execute_batch(migration.sql).map_err(|error| {
            storage_error(&format!("执行数据库迁移 {} 失败", migration.version), error)
        })?;
        transaction
            .execute(
                "INSERT INTO schema_migrations(version, checksum, applied_at) VALUES (?1, ?2, ?3)",
                params![
                    migration.version,
                    migration_checksum(migration.sql),
                    timestamp_to_db(Timestamp::now_utc())
                ],
            )
            .map_err(|error| storage_error("记录迁移版本失败", error))?;
    }
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

/// 迁移 SQL 的行尾归一化：CRLF 统一为 LF，其它字节保持不变。
///
/// 迁移 SQL 由 `include_str!` 嵌入二进制，字节内容取决于构建时的检出：`.gitattributes`
/// 的 `text=auto` 会让 Windows 检出得到 CRLF，Linux/CI 得到 LF。若按原始字节计算校验和，
/// 同一份迁移在不同检出的程序之间会得到不同结果，用户升级后会被误判为
/// `migration_checksum_mismatch` 而无法启动。因此写入与比较统一使用归一化后的校验和。
fn normalize_line_endings(sql: &str) -> String {
    sql.replace("\r\n", "\n")
}

/// 按归一化行尾计算的迁移校验和，所有新写入与比较都使用它。
fn migration_checksum(sql: &str) -> String {
    sha256_hex(normalize_line_endings(sql).as_bytes())
}

/// 2.0.0 及更早版本按原始字节记录校验和时可能出现的两种变体（CRLF 检出 / LF 检出）。
///
/// 自愈只允许命中这两种变体，绝不接受其它不匹配。
fn legacy_byte_checksums(sql: &str) -> [String; 2] {
    let lf = normalize_line_endings(sql);
    let crlf = lf.replace('\n', "\r\n");
    [sha256_hex(lf.as_bytes()), sha256_hex(crlf.as_bytes())]
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_SQL: &str = "CREATE TABLE demo(id INTEGER);\nINSERT INTO demo VALUES (1);\n";

    #[test]
    fn normalized_checksum_ignores_line_endings() {
        let crlf = SAMPLE_SQL.replace('\n', "\r\n");
        assert!(crlf.contains("\r\n"));

        assert_eq!(migration_checksum(SAMPLE_SQL), migration_checksum(&crlf));
    }

    #[test]
    fn normalized_checksum_matches_lf_plain_bytes() {
        assert_eq!(
            migration_checksum(SAMPLE_SQL),
            sha256_hex(SAMPLE_SQL.as_bytes())
        );
    }

    #[test]
    fn legacy_byte_checksums_cover_both_checkouts() {
        let crlf = SAMPLE_SQL.replace('\n', "\r\n");
        let variants = legacy_byte_checksums(SAMPLE_SQL);

        assert!(variants.contains(&sha256_hex(SAMPLE_SQL.as_bytes())));
        assert!(variants.contains(&sha256_hex(crlf.as_bytes())));
        assert_eq!(variants.len(), 2);
    }
}
