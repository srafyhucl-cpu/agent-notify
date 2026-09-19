CREATE TABLE schema_migrations (
    version INTEGER PRIMARY KEY,
    checksum TEXT NOT NULL,
    applied_at TEXT NOT NULL
);

CREATE TABLE settings (
    key TEXT PRIMARY KEY,
    value_json TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE agent_configs (
    agent_id TEXT PRIMARY KEY,
    enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
    config_json TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE channel_accounts (
    account_id TEXT PRIMARY KEY,
    channel_id TEXT NOT NULL,
    display_name TEXT NOT NULL,
    enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
    config_json TEXT NOT NULL,
    secret_ref TEXT,
    cursor_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE notifications (
    notification_id TEXT PRIMARY KEY,
    agent_id TEXT NOT NULL,
    ingest_key TEXT NOT NULL,
    session_id TEXT,
    session_title TEXT,
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    occurred_at TEXT NOT NULL,
    metadata_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE (agent_id, ingest_key)
);

CREATE TABLE deliveries (
    delivery_id TEXT PRIMARY KEY,
    notification_id TEXT NOT NULL REFERENCES notifications(notification_id),
    channel_id TEXT NOT NULL,
    account_id TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('Pending','Sent','Failed','Unknown','Skipped')),
    external_message_id TEXT,
    error_code TEXT,
    error_message TEXT,
    retryable INTEGER NOT NULL DEFAULT 0 CHECK (retryable IN (0, 1)),
    attempt_count INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE (notification_id, channel_id, account_id)
);

CREATE TABLE reply_routes (
    channel_id TEXT NOT NULL,
    account_id TEXT NOT NULL,
    external_message_id TEXT NOT NULL,
    agent_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    created_at TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    PRIMARY KEY (channel_id, account_id, external_message_id)
);

CREATE TABLE inbound_claims (
    claim_key TEXT PRIMARY KEY,
    channel_id TEXT NOT NULL,
    account_id TEXT NOT NULL,
    external_message_id TEXT,
    state TEXT NOT NULL CHECK (state IN ('InProgress','Completed','Failed','Unknown')),
    error_code TEXT,
    error_message TEXT,
    received_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    expires_at TEXT NOT NULL
);

CREATE TABLE outbox (
    outbox_id TEXT PRIMARY KEY,
    notification_id TEXT NOT NULL REFERENCES notifications(notification_id),
    state TEXT NOT NULL CHECK (state IN ('Pending','Leased','Done','Unknown','Dead')),
    available_at TEXT NOT NULL,
    lease_owner TEXT,
    lease_until TEXT,
    attempt_count INTEGER NOT NULL DEFAULT 0,
    last_error_code TEXT,
    last_error_message TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE adapter_manifests (
    adapter_id TEXT PRIMARY KEY,
    adapter_kind TEXT NOT NULL CHECK (adapter_kind IN ('agent','channel')),
    version TEXT NOT NULL,
    manifest_json TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 0 CHECK (enabled IN (0, 1)),
    updated_at TEXT NOT NULL
);

CREATE INDEX idx_notifications_occurred_at ON notifications(occurred_at DESC);
CREATE INDEX idx_deliveries_notification ON deliveries(notification_id);
CREATE INDEX idx_reply_routes_expires_at ON reply_routes(expires_at);
CREATE INDEX idx_inbound_claims_expires_at ON inbound_claims(expires_at);
CREATE INDEX idx_outbox_ready ON outbox(state, available_at, created_at);
