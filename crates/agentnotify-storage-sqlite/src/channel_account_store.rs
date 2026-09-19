use agentnotify_application::{ChannelAccountStore, StoreError};
use agentnotify_channel_sdk::{ChannelAccount, SecretRef};
use agentnotify_domain::{ChannelAccountId, ChannelId, Timestamp};
use rusqlite::{TransactionBehavior, params};

use crate::SqliteStore;
use crate::migrations::storage_error;
use crate::row_codec::{column, optional_column, timestamp_column, timestamp_to_db};
use crate::sqlite_helpers::{map_write_error, query_optional};

const ACCOUNT_COLUMNS: &str = "account_id, channel_id, display_name, enabled, config_json, \
                               secret_ref, cursor_json, created_at, updated_at";

#[async_trait::async_trait]
impl ChannelAccountStore for SqliteStore {
    async fn get(
        &self,
        account_id: &ChannelAccountId,
    ) -> Result<Option<ChannelAccount>, StoreError> {
        let account_id = account_id.clone();
        self.run(move |connection| {
            query_optional(
                connection,
                &format!("SELECT {ACCOUNT_COLUMNS} FROM channel_accounts WHERE account_id = ?1"),
                params![account_id.as_str()],
                account_from_row,
            )
        })
        .await
    }

    async fn list(&self, channel_id: &ChannelId) -> Result<Vec<ChannelAccount>, StoreError> {
        let channel_id = channel_id.clone();
        self.run(move |connection| {
            let mut statement = connection
                .prepare(&format!(
                    "SELECT {ACCOUNT_COLUMNS} FROM channel_accounts \
                     WHERE channel_id = ?1 ORDER BY account_id"
                ))
                .map_err(|error| storage_error("准备渠道账号查询失败", error))?;
            let mut rows = statement
                .query(params![channel_id.as_str()])
                .map_err(|error| storage_error("查询渠道账号失败", error))?;
            let mut accounts = Vec::new();
            while let Some(row) = rows
                .next()
                .map_err(|error| storage_error("读取渠道账号行失败", error))?
            {
                accounts.push(account_from_row(row)?);
            }
            Ok(accounts)
        })
        .await
    }

    async fn upsert(&self, account: ChannelAccount) -> Result<(), StoreError> {
        self.run(move |connection| {
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(|error| storage_error("开启渠道账号事务失败", error))?;
            transaction
                .execute(
                    "INSERT INTO channel_accounts(\
                        account_id, channel_id, display_name, enabled, config_json, secret_ref, \
                        cursor_json, created_at, updated_at\
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) \
                     ON CONFLICT(account_id) DO UPDATE SET \
                        channel_id = excluded.channel_id, \
                        display_name = excluded.display_name, \
                        enabled = excluded.enabled, \
                        config_json = excluded.config_json, \
                        secret_ref = excluded.secret_ref, \
                        cursor_json = excluded.cursor_json, \
                        updated_at = excluded.updated_at",
                    params![
                        account.id.as_str(),
                        account.channel_id.as_str(),
                        account.display_name,
                        account.enabled,
                        serde_json::to_string(&account.config)
                            .map_err(|_| StoreError::corrupted("序列化渠道账号配置失败"))?,
                        account.secret_ref.as_ref().map(SecretRef::as_str),
                        serde_json::to_string(&account.cursor)
                            .map_err(|_| StoreError::corrupted("序列化渠道账号游标失败"))?,
                        timestamp_to_db(account.created_at),
                        timestamp_to_db(account.updated_at),
                    ],
                )
                .map_err(|error| map_write_error("保存渠道账号失败", error))?;
            transaction
                .commit()
                .map_err(|error| storage_error("提交渠道账号事务失败", error))
        })
        .await
    }

    async fn set_enabled(
        &self,
        account_id: &ChannelAccountId,
        enabled: bool,
    ) -> Result<(), StoreError> {
        let account_id = account_id.clone();
        self.run(move |connection| {
            let updated = connection
                .execute(
                    "UPDATE channel_accounts SET enabled = ?1, updated_at = ?2 \
                     WHERE account_id = ?3",
                    params![
                        enabled,
                        timestamp_to_db(Timestamp::now_utc()),
                        account_id.as_str()
                    ],
                )
                .map_err(|error| storage_error("更新渠道账号状态失败", error))?;
            if updated == 0 {
                return Err(StoreError::not_found("account_missing", "找不到渠道账号"));
            }
            Ok(())
        })
        .await
    }
}

fn account_from_row(row: &rusqlite::Row<'_>) -> Result<ChannelAccount, StoreError> {
    let secret_ref = optional_column::<String>(row, "secret_ref")?
        .map(SecretRef::new)
        .transpose()
        .map_err(|_| StoreError::corrupted("渠道账号密钥引用损坏"))?;
    let config = parse_json(row, "config_json", "渠道账号配置损坏")?;
    let cursor = parse_json(row, "cursor_json", "渠道账号游标损坏")?;
    Ok(ChannelAccount {
        id: ChannelAccountId::new(column::<String>(row, "account_id")?)
            .map_err(|_| StoreError::corrupted("渠道账号标识损坏"))?,
        channel_id: ChannelId::new(column::<String>(row, "channel_id")?)
            .map_err(|_| StoreError::corrupted("渠道标识损坏"))?,
        display_name: column(row, "display_name")?,
        enabled: column::<i64>(row, "enabled")? != 0,
        config,
        secret_ref,
        cursor,
        created_at: timestamp_column(row, "created_at")?,
        updated_at: timestamp_column(row, "updated_at")?,
    })
}

fn parse_json(
    row: &rusqlite::Row<'_>,
    name: &str,
    message: &str,
) -> Result<serde_json::Value, StoreError> {
    serde_json::from_str(&column::<String>(row, name)?).map_err(|_| StoreError::corrupted(message))
}
