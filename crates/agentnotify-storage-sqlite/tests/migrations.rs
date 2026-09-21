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
