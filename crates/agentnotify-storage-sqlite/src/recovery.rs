use agentnotify_application::StoreError;
use agentnotify_domain::Timestamp;
use rusqlite::{TransactionBehavior, params};

use crate::SqliteStore;
use crate::migrations::storage_error;
use crate::row_codec::timestamp_to_db;

const INTERRUPTED_DELIVERY_CODE: &str = "delivery_unknown_after_restart";
const INTERRUPTED_DELIVERY_MESSAGE: &str = "上次运行中断，投递结果无法确认，未自动重试";
const INTERRUPTED_REPLY_CODE: &str = "reply_unknown_after_restart";
const INTERRUPTED_REPLY_MESSAGE: &str = "上次运行中断，回复结果无法确认，未自动重试";

/// 启动恢复处理过的中断记录数量。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RecoverySummary {
    pub interrupted_outbox: usize,
    pub interrupted_claims: usize,
}

impl SqliteStore {
    /// 将上次进程中断遗留的租约和 Claim 收敛到 Unknown 终态。
    ///
    /// 单实例宿主启动时不存在仍由当前进程持有的工作；不能把这些记录
    /// 重新放回可投递队列，否则会破坏推送和引用的至多一次语义。
    pub async fn recover_interrupted_work(
        &self,
        recovered_at: Timestamp,
    ) -> Result<RecoverySummary, StoreError> {
        self.run(move |connection| {
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(|error| storage_error("开启启动恢复事务失败", error))?;
            let recovered_at = timestamp_to_db(recovered_at);

            let interrupted_outbox = transaction
                .execute(
                    "UPDATE outbox SET state = 'Unknown', lease_owner = NULL, \
                                        lease_until = NULL, last_error_code = ?1, \
                                        last_error_message = ?2, \
                                        updated_at = CASE WHEN updated_at > ?3 THEN updated_at ELSE ?3 END \
                     WHERE state = 'Leased'",
                    params![
                        INTERRUPTED_DELIVERY_CODE,
                        INTERRUPTED_DELIVERY_MESSAGE,
                        recovered_at
                    ],
                )
                .map_err(|error| storage_error("恢复中断投递失败", error))?;

            let interrupted_claims = transaction
                .execute(
                    "UPDATE inbound_claims SET state = 'Unknown', error_code = ?1, \
                                               error_message = ?2, \
                                               updated_at = CASE WHEN updated_at > ?3 THEN updated_at ELSE ?3 END \
                     WHERE state = 'InProgress'",
                    params![
                        INTERRUPTED_REPLY_CODE,
                        INTERRUPTED_REPLY_MESSAGE,
                        recovered_at
                    ],
                )
                .map_err(|error| storage_error("恢复中断回复失败", error))?;

            transaction
                .commit()
                .map_err(|error| storage_error("提交启动恢复事务失败", error))?;

            Ok(RecoverySummary {
                interrupted_outbox,
                interrupted_claims,
            })
        })
        .await
    }
}
