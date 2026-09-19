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

#[test]
fn migration_creates_required_tables_and_enables_wal() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = SqliteStore::open(temp.path().join("state.db")).unwrap();
    store.migrate().unwrap();

    assert_eq!(store.schema_version().unwrap(), 1);
    assert_eq!(store.journal_mode().unwrap().to_ascii_lowercase(), "wal");
    assert!(store.table_exists("schema_migrations").unwrap());

    for table in CORE_TABLES {
        assert!(store.table_exists(table).unwrap(), "缺少表 {table}");
    }
}

#[test]
fn running_migrations_twice_is_idempotent() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = SqliteStore::open(temp.path().join("state.db")).unwrap();

    store.migrate().unwrap();
    store.migrate().unwrap();

    assert_eq!(store.schema_version().unwrap(), 1);
}

#[test]
fn migration_checksum_mismatch_is_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("state.db");

    {
        let mut store = SqliteStore::open(&path).unwrap();
        store.migrate().unwrap();
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

    let mut store = SqliteStore::open(&path).unwrap();
    let error = store.migrate().unwrap_err();

    assert_eq!(error.code(), "migration_checksum_mismatch");
}
