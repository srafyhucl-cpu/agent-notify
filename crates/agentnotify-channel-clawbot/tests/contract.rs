use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use agentnotify_application::{SecretError, SecretKind, SecretStore, SecretValue};
use agentnotify_channel_clawbot::{ClawBotChannel, capabilities, descriptor};
use agentnotify_channel_sdk::{ChannelAdapter, assert_channel_contract};
use agentnotify_domain::ChannelAccountId;
use async_trait::async_trait;

#[tokio::test]
async fn adapter_passes_channel_contract() {
    let channel = ClawBotChannel::new(Arc::new(EmptySecretStore::default()));
    assert_channel_contract(Arc::new(channel)).await;
}

#[test]
fn descriptor_and_capabilities_are_stable() {
    let channel = ClawBotChannel::new(Arc::new(EmptySecretStore::default()));

    assert_eq!(channel.descriptor(), descriptor());
    assert_eq!(channel.descriptor().id.as_str(), "clawbot");
    assert_eq!(channel.capabilities(), capabilities());
    assert!(channel.capabilities().send_text);
    assert!(channel.capabilities().receive);
    assert!(channel.capabilities().reply_routing);
    assert!(channel.capabilities().markdown);
    assert_eq!(channel.capabilities().max_text_bytes, Some(32 * 1024));
}

#[derive(Default)]
struct EmptySecretStore {
    values: Mutex<HashMap<(ChannelAccountId, SecretKind), SecretValue>>,
}

#[async_trait]
impl SecretStore for EmptySecretStore {
    async fn get(
        &self,
        _account_id: &ChannelAccountId,
        _kind: SecretKind,
    ) -> Result<SecretValue, SecretError> {
        Err(SecretError::new("secret_not_found", "找不到测试密钥"))
    }

    async fn set(
        &self,
        account_id: &ChannelAccountId,
        kind: SecretKind,
        value: SecretValue,
    ) -> Result<(), SecretError> {
        self.values
            .lock()
            .expect("密钥锁不应失败")
            .insert((account_id.clone(), kind), value);
        Ok(())
    }

    async fn delete(
        &self,
        _account_id: &ChannelAccountId,
        _kind: SecretKind,
    ) -> Result<(), SecretError> {
        Ok(())
    }
}
