use agentnotify_domain::{ClaimKey, ClaimOutcome, InboundClaim};

use crate::StoreError;

#[async_trait::async_trait]
pub trait ClaimStore: Send + Sync {
    async fn claim(&self, claim: InboundClaim) -> Result<ClaimOutcome, StoreError>;

    async fn update_claim(&self, claim: InboundClaim) -> Result<(), StoreError>;

    async fn find_claim(&self, key: &ClaimKey) -> Result<Option<InboundClaim>, StoreError>;
}
