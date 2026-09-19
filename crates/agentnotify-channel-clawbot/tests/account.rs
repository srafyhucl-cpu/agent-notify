use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use agentnotify_application::{SecretError, SecretKind, SecretStore, SecretValue};
use agentnotify_channel_clawbot::{
    ClawBotAccount, ClawBotChannel, ClawBotContext, ClawBotCredentials, DEFAULT_BASE_URL,
};
use agentnotify_channel_sdk::ChannelAdapter;
use agentnotify_domain::ChannelAccountId;
use async_trait::async_trait;

#[test]
fn account_scope_is_stable_and_does_not_expose_platform_ids() {
    let first = ClawBotAccount::from_platform_ids("bot-1", "user-1").unwrap();
    let second = ClawBotAccount::from_platform_ids("bot-1", "user-1").unwrap();

    assert_eq!(first.id(), second.id());
    assert_eq!(first.id().as_str(), "clawbot-c9a076b7fea8ae7f");
    assert!(!first.id().as_str().contains("bot-1"));
    assert!(!first.id().as_str().contains("user-1"));
}

#[test]
fn account_keeps_only_tail_hints_and_default_base_url() {
    let account = ClawBotAccount::from_platform_ids("bot-1234567890", "user-abcdef123456").unwrap();

    assert_eq!(account.bot_id_hint(), "567890");
    assert_eq!(account.user_id_hint(), "123456");
    assert_eq!(account.base_url(), DEFAULT_BASE_URL);
    assert_eq!(account.channel_account().channel_id.as_str(), "clawbot");
    assert!(account.channel_account().secret_ref.is_some());
}

#[test]
fn account_conversion_never_contains_secret_material() {
    let account = ClawBotAccount::from_platform_ids(
        "bot-identifier-1234567890",
        "user-identifier-abcdef123456",
    )
    .unwrap();
    let encoded = serde_json::to_string(account.channel_account()).unwrap();

    assert!(!encoded.contains("bot-token-value"));
    assert!(!encoded.contains("context-token-value"));
    assert!(!encoded.contains("bot-identifier-1234567890"));
    assert!(!encoded.contains("user-identifier-abcdef123456"));
}

#[test]
fn credential_debug_does_not_expose_tokens_or_platform_ids() {
    let credentials = ClawBotCredentials::new(
        "bot-token-value",
        "bot-identifier",
        "user-identifier",
        DEFAULT_BASE_URL,
    )
    .unwrap();

    let debug = format!("{credentials:?}");
    assert!(!debug.contains("bot-token-value"));
    assert!(!debug.contains("bot-identifier"));
    assert!(!debug.contains("user-identifier"));
}
#[test]
fn different_platform_bindings_get_different_account_ids() {
    let first = ClawBotAccount::from_platform_ids("bot-1", "user-1").unwrap();
    let second = ClawBotAccount::from_platform_ids("bot-1", "user-2").unwrap();
    let third = ClawBotAccount::from_platform_ids("bot-2", "user-1").unwrap();

    assert_ne!(first.id(), second.id());
    assert_ne!(first.id(), third.id());
    assert_ne!(second.id(), third.id());
}

#[test]
fn empty_platform_ids_are_rejected() {
    assert!(ClawBotAccount::from_platform_ids("", "user-1").is_err());
    assert!(ClawBotAccount::from_platform_ids("bot-1", " ").is_err());
}

#[tokio::test]
async fn credentials_round_trip_through_secret_store_without_touching_history() {
    let store = Arc::new(TestSecretStore::default());
    let channel = ClawBotChannel::new(store.clone());
    let account = ClawBotAccount::from_platform_ids("bot-1", "user-1").unwrap();
    let credentials =
        ClawBotCredentials::new("bot-token-value", "bot-1", "user-1", DEFAULT_BASE_URL).unwrap();
    let context = ClawBotContext::new("context-token-value", "user-1").unwrap();

    channel
        .save_credentials(account.id(), &credentials)
        .await
        .unwrap();
    channel.save_context(account.id(), &context).await.unwrap();
    store.record_history();

    assert_eq!(
        channel.load_credentials(account.id()).await.unwrap(),
        credentials
    );
    assert_eq!(channel.load_context(account.id()).await.unwrap(), context);

    channel
        .logout(account.channel_account().clone())
        .await
        .unwrap();

    assert!(matches!(
        store.get(account.id(), SecretKind::BotToken).await,
        Err(SecretError { .. })
    ));
    assert!(matches!(
        store.get(account.id(), SecretKind::ContextToken).await,
        Err(SecretError { .. })
    ));
    assert_eq!(store.history_count(), 1);
}

#[derive(Default)]
struct TestSecretStore {
    values: Mutex<HashMap<(ChannelAccountId, SecretKind), SecretValue>>,
    history_count: Mutex<usize>,
}

impl TestSecretStore {
    fn record_history(&self) {
        *self.history_count.lock().expect("历史计数锁不应失败") += 1;
    }

    fn history_count(&self) -> usize {
        *self.history_count.lock().expect("历史计数锁不应失败")
    }
}

#[async_trait]
impl SecretStore for TestSecretStore {
    async fn get(
        &self,
        account_id: &ChannelAccountId,
        kind: SecretKind,
    ) -> Result<SecretValue, SecretError> {
        self.values
            .lock()
            .expect("密钥锁不应失败")
            .get(&(account_id.clone(), kind))
            .cloned()
            .ok_or_else(|| SecretError::new("secret_not_found", "找不到测试密钥"))
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
        account_id: &ChannelAccountId,
        kind: SecretKind,
    ) -> Result<(), SecretError> {
        self.values
            .lock()
            .expect("密钥锁不应失败")
            .remove(&(account_id.clone(), kind));
        Ok(())
    }
}
