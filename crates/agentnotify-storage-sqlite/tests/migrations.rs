use agentnotify_storage_sqlite::SqliteStore;
use rusqlite::{Connection, params};
use sha2::{Digest, Sha256};

const MIGRATION_0001: &str = include_str!("../migrations/0001_init.sql");
const MIGRATION_0002: &str = include_str!("../migrations/0002_canonical_timestamps.sql");

fn sha256_hex(value: &[u8]) -> String {
    let digest = Sha256::digest(value);
    let mut text = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write;
        let _ = write!(text, "{byte:02x}");
    }
    text
}

/// 归一化行尾后的校验和：与程序内部 `migration_checksum` 的语义一致。
fn lf_checksum(sql: &str) -> String {
    sha256_hex(sql.replace("\r\n", "\n").as_bytes())
}

/// 旧版本在 CRLF 检出下按原始字节记录的校验和。
fn crlf_checksum(sql: &str) -> String {
    let crlf = sql.replace("\r\n", "\n").replace('\n', "\r\n");
    sha256_hex(crlf.as_bytes())
}

/// 建一个已应用全部迁移的库，迁移记录的校验和由调用方决定。
fn seed_fully_migrated_database(
    path: &std::path::Path,
    checksum: impl Fn(&str) -> String,
) -> Connection {
    let connection = Connection::open(path).unwrap();
    connection.execute_batch(MIGRATION_0001).unwrap();
    connection.execute_batch(MIGRATION_0002).unwrap();
    for (version, sql) in [(1, MIGRATION_0001), (2, MIGRATION_0002)] {
        connection
            .execute(
                "INSERT INTO schema_migrations(version, checksum, applied_at) VALUES (?1, ?2, ?3)",
                params![version, checksum(sql), "2026-09-19T09:00:00.800Z"],
            )
            .unwrap();
    }
    connection
}

fn recorded_checksum(connection: &Connection, version: i64) -> String {
    connection
        .query_row(
            "SELECT checksum FROM schema_migrations WHERE version = ?1",
            [version],
            |row| row.get(0),
        )
        .unwrap()
}

const CORE_TABLES: [&str; 9] = [
    "settings",
    "agent_configs",
    "channel_accounts",
    "notifications",
    "deliveries",
    "reply_routes",
    "inbound_claims",
    "outbox",
    "adapter_manifests",
];

#[tokio::test]
async fn open_creates_required_tables_and_enables_wal() {
    let temp = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(temp.path().join("state.db")).unwrap();

    assert_eq!(store.schema_version().await.unwrap(), 2);
    assert_eq!(
        store.journal_mode().await.unwrap().to_ascii_lowercase(),
        "wal"
    );
    assert!(store.table_exists("schema_migrations").await.unwrap());

    for table in CORE_TABLES {
        assert!(store.table_exists(table).await.unwrap(), "缺少表 {table}");
    }
}

#[tokio::test]
async fn reopening_is_idempotent() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("state.db");

    {
        let store = SqliteStore::open(&path).unwrap();
        assert_eq!(store.schema_version().await.unwrap(), 2);
    }
    {
        let store = SqliteStore::open(&path).unwrap();
        assert_eq!(store.schema_version().await.unwrap(), 2);
    }
}

#[tokio::test]
async fn migration_checksum_mismatch_is_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("state.db");

    {
        let store = SqliteStore::open(&path).unwrap();
        assert_eq!(store.schema_version().await.unwrap(), 2);
    }

    {
        let connection = Connection::open(&path).unwrap();
        connection
            .execute(
                "UPDATE schema_migrations SET checksum = 'tampered' WHERE version = 1",
                [],
            )
            .unwrap();
    }

    let error = match SqliteStore::open(&path) {
        Ok(_) => panic!("篡改 checksum 后仍成功打开数据库"),
        Err(error) => error,
    };
    assert_eq!(error.code(), "migration_checksum_mismatch");
}

#[tokio::test]
async fn heals_legacy_crlf_checksums_recorded_by_older_release() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("state.db");
    drop(seed_fully_migrated_database(&path, crlf_checksum));

    {
        let store = SqliteStore::open(&path).unwrap();
        assert_eq!(store.schema_version().await.unwrap(), 2);
    }

    {
        let connection = Connection::open(&path).unwrap();
        for (version, sql) in [(1, MIGRATION_0001), (2, MIGRATION_0002)] {
            assert_eq!(
                recorded_checksum(&connection, version),
                lf_checksum(sql),
                "版本 {version} 的校验和未自愈为归一化值"
            );
        }
    }

    // 自愈必须幂等：再次打开仍成功，且不再需要改写。
    let store = SqliteStore::open(&path).unwrap();
    assert_eq!(store.schema_version().await.unwrap(), 2);
    drop(store);

    let connection = Connection::open(&path).unwrap();
    assert_eq!(
        recorded_checksum(&connection, 1),
        lf_checksum(MIGRATION_0001)
    );
    assert_eq!(
        recorded_checksum(&connection, 2),
        lf_checksum(MIGRATION_0002)
    );
}

#[tokio::test]
async fn accepts_legacy_lf_checksums_recorded_by_older_release() {
    // 现场证据：用户库由 LF 检出的预览版创建，记录的是 LF 原始字节校验和。
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("state.db");
    drop(seed_fully_migrated_database(&path, lf_checksum));

    let store = SqliteStore::open(&path).unwrap();
    assert_eq!(store.schema_version().await.unwrap(), 2);
    drop(store);

    let connection = Connection::open(&path).unwrap();
    assert_eq!(
        recorded_checksum(&connection, 1),
        lf_checksum(MIGRATION_0001)
    );
    assert_eq!(
        recorded_checksum(&connection, 2),
        lf_checksum(MIGRATION_0002)
    );
}

#[tokio::test]
async fn tampered_migration_content_is_still_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("state.db");

    // 篡改后的内容即使按 CRLF 变体计算校验和，也不得被自愈放过。
    let tampered = format!("{MIGRATION_0001}\n-- 被篡改的迁移内容");
    let connection = seed_fully_migrated_database(&path, crlf_checksum);
    connection
        .execute(
            "UPDATE schema_migrations SET checksum = ?1 WHERE version = 1",
            params![crlf_checksum(&tampered)],
        )
        .unwrap();
    drop(connection);

    let error = match SqliteStore::open(&path) {
        Ok(_) => panic!("内容被篡改的迁移仍成功打开数据库"),
        Err(error) => error,
    };
    assert_eq!(error.code(), "migration_checksum_mismatch");
}

#[tokio::test]
async fn upgrades_v1_database_and_canonicalizes_timestamps() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("state.db");

    {
        let connection = Connection::open(&path).unwrap();
        connection.execute_batch(MIGRATION_0001).unwrap();
        connection
            .execute(
                "INSERT INTO schema_migrations(version, checksum, applied_at) VALUES (?1, ?2, ?3)",
                params![
                    1,
                    sha256_hex(MIGRATION_0001.as_bytes()),
                    "2026-09-19T09:00:00.8Z"
                ],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO settings(key, value_json, updated_at) VALUES (?1, ?2, ?3)",
                params!["legacy", "{}", "2026-09-19T09:00:00.8Z"],
            )
            .unwrap();
    }

    let store = SqliteStore::open(&path).unwrap();
    assert_eq!(store.schema_version().await.unwrap(), 2);
    drop(store);

    let connection = Connection::open(&path).unwrap();
    let setting_time: String = connection
        .query_row(
            "SELECT updated_at FROM settings WHERE key = 'legacy'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let applied_time: String = connection
        .query_row(
            "SELECT applied_at FROM schema_migrations WHERE version = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let versions: i64 = connection
        .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
            row.get(0)
        })
        .unwrap();

    assert_eq!(setting_time, "2026-09-19T09:00:00.800Z");
    assert_eq!(applied_time, "2026-09-19T09:00:00.800Z");
    assert_eq!(versions, 2);
}

#[tokio::test]
async fn future_migration_version_is_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("state.db");

    let store = SqliteStore::open(&path).unwrap();
    assert_eq!(store.schema_version().await.unwrap(), 2);
    drop(store);

    let connection = Connection::open(&path).unwrap();
    connection
        .execute(
            "INSERT INTO schema_migrations(version, checksum, applied_at) VALUES (3, 'future', '2026-09-19T09:00:00.800Z')",
            [],
        )
        .unwrap();
    drop(connection);

    let error = match SqliteStore::open(&path) {
        Ok(_) => panic!("未来迁移版本仍成功打开数据库"),
        Err(error) => error,
    };
    assert_eq!(error.code(), "migration_version_unsupported");
}

#[tokio::test]
async fn migration_history_gap_is_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("state.db");

    let connection = Connection::open(&path).unwrap();
    connection.execute_batch(MIGRATION_0001).unwrap();
    connection
        .execute(
            "INSERT INTO schema_migrations(version, checksum, applied_at) VALUES (?1, ?2, ?3)",
            params![
                2,
                sha256_hex(MIGRATION_0002.as_bytes()),
                "2026-09-19T09:00:00.800Z"
            ],
        )
        .unwrap();
    drop(connection);

    let error = match SqliteStore::open(&path) {
        Ok(_) => panic!("迁移历史缺口仍成功打开数据库"),
        Err(error) => error,
    };
    assert_eq!(error.code(), "migration_history_gap");
}
