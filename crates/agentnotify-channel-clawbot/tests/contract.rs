use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use agentnotify_application::{SecretError, SecretKind, SecretStore, SecretValue};
use agentnotify_channel_clawbot::{
    ClawBotAccount, ClawBotAccountState, ClawBotChannel, ClawBotContext, ClawBotCredentials,
    capabilities, descriptor,
};
use agentnotify_channel_sdk::{ChannelAccount, ChannelAdapter, assert_channel_contract};
use agentnotify_domain::{ChannelAccountId, Timestamp};
use async_trait::async_trait;

#[tokio::test]
async fn adapter_passes_channel_contract() {
    let channel = ClawBotChannel::new(Arc::new(MemorySecretStore::default()));
    assert_channel_contract(Arc::new(channel)).await;
}

#[test]
fn descriptor_and_capabilities_are_stable() {
    let channel = ClawBotChannel::new(Arc::new(MemorySecretStore::default()));

    assert_eq!(channel.descriptor(), descriptor());
    assert_eq!(channel.descriptor().id.as_str(), "clawbot");
    assert_eq!(channel.capabilities(), capabilities());
    assert!(channel.capabilities().send_text);
    assert!(channel.capabilities().receive);
    assert!(channel.capabilities().reply_routing);
    assert!(channel.capabilities().markdown);
    assert_eq!(channel.capabilities().max_text_bytes, Some(32 * 1024));
}

/// 刚登录、还没收到过消息（从未就绪）：属正常等待，不能报「推送已断」。
#[tokio::test]
async fn inspect_stays_healthy_before_first_push_session() {
    let (account, channel_account) = account_with_state(false, false);
    let channel = ClawBotChannel::new(Arc::new(MemorySecretStore::default()));
    channel
        .save_credentials(account.id(), &test_credentials())
        .await
        .unwrap();

    let health = channel.inspect(channel_account).await;

    assert!(health.available, "登录有效必须 available：{health:?}");
    assert!(!health.stale, "从未就绪不得报推送已断：{health:?}");
}

/// 推送上下文就绪：healthy（无告警）。
#[tokio::test]
async fn inspect_stays_healthy_with_live_push_session() {
    let (account, channel_account) = account_with_state(true, false);
    let channel = ClawBotChannel::new(Arc::new(MemorySecretStore::default()));
    channel
        .save_credentials(account.id(), &test_credentials())
        .await
        .unwrap();
    channel
        .save_context(
            account.id(),
            &ClawBotContext::new("ctx-1", "user-1").unwrap(),
        )
        .await
        .unwrap();

    let health = channel.inspect(channel_account).await;

    assert!(health.available && !health.stale, "{health:?}");
}

/// 曾就绪过但推送上下文已被平台回收（发送会 ret=-2 prepare failed）：必须标「推送已断」并给恢复指引。
#[tokio::test]
async fn inspect_marks_stale_when_push_session_missing() {
    let (account, channel_account) = account_with_state(true, false);
    let channel = ClawBotChannel::new(Arc::new(MemorySecretStore::default()));
    channel
        .save_credentials(account.id(), &test_credentials())
        .await
        .unwrap();

    let health = channel.inspect(channel_account).await;

    assert!(health.available, "登录仍有效：{health:?}");
    assert!(health.stale, "曾就绪 + 上下文缺失必须标 stale：{health:?}");
    let detail = health.detail.expect("必须给出恢复指引");
    assert_eq!(detail.code(), "clawbot_push_session_missing");
    assert!(
        detail.message().contains("发任意一条消息"),
        "{}",
        detail.message()
    );
}

/// 上下文存在但绑定的是别的用户：同样算推送会话失效（不猜测、不误发）。
#[tokio::test]
async fn inspect_marks_stale_when_context_user_mismatches() {
    let (account, channel_account) = account_with_state(true, false);
    let channel = ClawBotChannel::new(Arc::new(MemorySecretStore::default()));
    channel
        .save_credentials(account.id(), &test_credentials())
        .await
        .unwrap();
    channel
        .save_context(
            account.id(),
            &ClawBotContext::new("ctx-1", "user-2").unwrap(),
        )
        .await
        .unwrap();

    let health = channel.inspect(channel_account).await;

    assert!(health.stale, "{health:?}");
    assert_eq!(
        health.detail.expect("必须带原因").code(),
        "clawbot_push_session_missing"
    );
}

/// 登录失效（stale_at 已标记）：提示重新扫码，优先于推送会话判定。
#[tokio::test]
async fn inspect_marks_stale_for_invalid_login() {
    let (account, channel_account) = account_with_state(true, true);
    let channel = ClawBotChannel::new(Arc::new(MemorySecretStore::default()));
    channel
        .save_credentials(account.id(), &test_credentials())
        .await
        .unwrap();

    let health = channel.inspect(channel_account).await;

    assert!(health.stale, "{health:?}");
    assert_eq!(
        health.detail.expect("必须带原因").code(),
        "clawbot_session_stale"
    );
}

fn account_with_state(established: bool, stale: bool) -> (ClawBotAccount, ChannelAccount) {
    let account = ClawBotAccount::from_platform_ids("bot-1", "user-1").expect("账号必须有效");
    let mut channel_account = account.channel_account().clone();
    let mut state: ClawBotAccountState =
        serde_json::from_value(channel_account.config.clone()).expect("状态必须可解析");
    state.session_established_at = established.then(Timestamp::now_utc);
    state.stale_at = stale.then(Timestamp::now_utc);
    channel_account.config = serde_json::to_value(state).expect("状态必须可编码");
    (account, channel_account)
}

fn test_credentials() -> ClawBotCredentials {
    ClawBotCredentials::new(
        "bot-secret",
        "bot-1",
        "user-1",
        "https://business.example.test",
    )
    .expect("凭据必须有效")
}

#[derive(Default)]
struct MemorySecretStore {
    values: Mutex<HashMap<(ChannelAccountId, SecretKind), SecretValue>>,
}

#[async_trait]
impl SecretStore for MemorySecretStore {
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
