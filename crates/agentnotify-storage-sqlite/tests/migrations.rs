use agentnotify_storage_sqlite::SqliteStore;
use rusqlite::Connection;

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

    assert_eq!(store.schema_version().await.unwrap(), 1);
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
        assert_eq!(store.schema_version().await.unwrap(), 1);
    }
    {
        let store = SqliteStore::open(&path).unwrap();
        assert_eq!(store.schema_version().await.unwrap(), 1);
    }
}

#[tokio::test]
async fn migration_checksum_mismatch_is_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("state.db");

    {
        let store = SqliteStore::open(&path).unwrap();
        assert_eq!(store.schema_version().await.unwrap(), 1);
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
