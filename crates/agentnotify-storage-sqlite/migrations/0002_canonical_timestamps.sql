-- 统一数据库时间的 RFC3339 精度，避免 SQLite 字符串比较把 .8Z 误判为晚于 .801Z。
UPDATE schema_migrations
SET applied_at = strftime('%Y-%m-%dT%H:%M:%fZ', applied_at);

UPDATE settings
SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', updated_at);

UPDATE agent_configs
SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', updated_at);

UPDATE channel_accounts
SET created_at = strftime('%Y-%m-%dT%H:%M:%fZ', created_at),
    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', updated_at);

UPDATE notifications
SET occurred_at = strftime('%Y-%m-%dT%H:%M:%fZ', occurred_at),
    created_at = strftime('%Y-%m-%dT%H:%M:%fZ', created_at);

UPDATE deliveries
SET created_at = strftime('%Y-%m-%dT%H:%M:%fZ', created_at),
    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', updated_at);

UPDATE reply_routes
SET created_at = strftime('%Y-%m-%dT%H:%M:%fZ', created_at),
    expires_at = strftime('%Y-%m-%dT%H:%M:%fZ', expires_at);

UPDATE inbound_claims
SET received_at = strftime('%Y-%m-%dT%H:%M:%fZ', received_at),
    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', updated_at),
    expires_at = strftime('%Y-%m-%dT%H:%M:%fZ', expires_at);

UPDATE outbox
SET available_at = strftime('%Y-%m-%dT%H:%M:%fZ', available_at),
    lease_until = CASE
        WHEN lease_until IS NULL THEN NULL
        ELSE strftime('%Y-%m-%dT%H:%M:%fZ', lease_until)
    END,
    created_at = strftime('%Y-%m-%dT%H:%M:%fZ', created_at),
    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', updated_at);

UPDATE adapter_manifests
SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', updated_at);
