use agentnotify_application::{ClaimStore, StoreError};
use agentnotify_domain::{ClaimKey, ClaimOutcome, InboundClaim};
use rusqlite::{TransactionBehavior, params};

use crate::SqliteStore;
use crate::migrations::storage_error;
use crate::row_codec::{claim_from_row, timestamp_to_db};
use crate::sqlite_helpers::{map_write_error, query_optional};

const CLAIM_COLUMNS: &str = "claim_key, channel_id, account_id, external_message_id, state, \
                             received_at, updated_at, expires_at";

#[async_trait::async_trait]
impl ClaimStore for SqliteStore {
    async fn claim(&self, claim: InboundClaim) -> Result<ClaimOutcome, StoreError> {
        self.run(move |connection| {
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(|error| storage_error("开启 Claim 事务失败", error))?;
            let inserted = transaction
                .execute(
                    "INSERT OR IGNORE INTO inbound_claims(\
                        claim_key, channel_id, account_id, external_message_id, state, \
                        error_code, error_message, received_at, updated_at, expires_at\
                    ) VALUES (?1, ?2, ?3, ?4, ?5, NULL, NULL, ?6, ?7, ?8)",
                    params![
                        claim.key.as_str(),
                        claim.channel_id.as_str(),
                        claim.account_id.as_str(),
                        claim
                            .external_message_id
                            .as_ref()
                            .map(|value| value.as_str()),
                        claim.state.as_str(),
                        timestamp_to_db(claim.received_at),
                        timestamp_to_db(claim.updated_at),
                        timestamp_to_db(claim.expires_at),
                    ],
                )
                .map_err(|error| map_write_error("写入入站 Claim 失败", error))?;

            let outcome = if inserted == 1 {
                ClaimOutcome::Acquired(claim)
            } else {
                let existing = query_optional(
                    &transaction,
                    &format!("SELECT {CLAIM_COLUMNS} FROM inbound_claims WHERE claim_key = ?1"),
                    params![claim.key.as_str()],
                    claim_from_row,
                )?
                .ok_or_else(|| StoreError::unavailable("入站 Claim 写入冲突但找不到已有记录"))?;
                ClaimOutcome::AlreadyClaimed {
                    state: existing.state,
                    updated_at: existing.updated_at,
                }
            };

            transaction
                .commit()
                .map_err(|error| storage_error("提交 Claim 事务失败", error))?;
            Ok(outcome)
        })
        .await
    }

    async fn update_claim(&self, claim: InboundClaim) -> Result<(), StoreError> {
        self.run(move |connection| {
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(|error| storage_error("开启 Claim 更新事务失败", error))?;
            let updated = transaction
                .execute(
                    "UPDATE inbound_claims SET state = ?1, error_code = NULL, \
                                               error_message = NULL, updated_at = ?2 \
                     WHERE claim_key = ?3",
                    params![
                        claim.state.as_str(),
                        timestamp_to_db(claim.updated_at),
                        claim.key.as_str(),
                    ],
                )
                .map_err(|error| storage_error("更新入站 Claim 失败", error))?;
            if updated != 1 {
                return Err(StoreError::not_found("claim_missing", "找不到入站 Claim"));
            }
            transaction
                .commit()
                .map_err(|error| storage_error("提交 Claim 更新事务失败", error))
        })
        .await
    }

    async fn find_claim(&self, key: &ClaimKey) -> Result<Option<InboundClaim>, StoreError> {
        let key = key.clone();
        self.run(move |connection| {
            query_optional(
                connection,
                &format!("SELECT {CLAIM_COLUMNS} FROM inbound_claims WHERE claim_key = ?1"),
                params![key.as_str()],
                claim_from_row,
            )
        })
        .await
    }
}
