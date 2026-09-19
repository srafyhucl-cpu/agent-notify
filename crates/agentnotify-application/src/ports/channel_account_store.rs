use agentnotify_channel_sdk::ChannelAccount;
use agentnotify_domain::{ChannelAccountId, ChannelId};

use crate::StoreError;

#[async_trait::async_trait]
pub trait ChannelAccountStore: Send + Sync {
    async fn get(
        &self,
        account_id: &ChannelAccountId,
    ) -> Result<Option<ChannelAccount>, StoreError>;

    async fn list(&self, channel_id: &ChannelId) -> Result<Vec<ChannelAccount>, StoreError>;

    async fn upsert(&self, account: ChannelAccount) -> Result<(), StoreError>;

    async fn set_enabled(
        &self,
        account_id: &ChannelAccountId,
        enabled: bool,
    ) -> Result<(), StoreError>;
}
