use std::sync::Arc;

use agentnotify_channel_sdk::{
    ChannelAccount, ChannelAdapter, ChannelCapabilities, ChannelDescriptor, ChannelError,
    ChannelHealth, ChannelTask, DeliveryReceipt, InboundEmitter, InboundMode, OutboundMessage,
    assert_channel_contract,
};
use agentnotify_domain::{
    ChannelAccountId, ChannelId, DeliveryState, ExternalMessageId, SafeError, Timestamp,
};

struct FakeChannel {
    unknown_on_send: bool,
}

impl FakeChannel {
    fn new() -> Self {
        Self {
            unknown_on_send: false,
        }
    }

    fn unknown_on_send() -> Self {
        Self {
            unknown_on_send: true,
        }
    }
}

#[async_trait::async_trait]
impl ChannelAdapter for FakeChannel {
    fn descriptor(&self) -> ChannelDescriptor {
        ChannelDescriptor {
            id: ChannelId::new("fake").unwrap(),
            display_name: "Fake Channel".into(),
            config_schema: serde_json::json!({"type": "object"}),
        }
    }

    fn capabilities(&self) -> ChannelCapabilities {
        ChannelCapabilities {
            send_text: true,
            receive: true,
            reply_routing: true,
            edit_message: false,
            attachments: false,
            markdown: true,
            max_text_bytes: Some(1024),
            inbound_modes: vec![InboundMode::LongPolling],
        }
    }

    async fn start(
        &self,
        _account: ChannelAccount,
        _emit: InboundEmitter,
    ) -> Result<ChannelTask, ChannelError> {
        Ok(ChannelTask::completed())
    }

    async fn send(
        &self,
        account: ChannelAccount,
        message: OutboundMessage,
    ) -> Result<DeliveryReceipt, ChannelError> {
        if message.text.trim().is_empty() {
            return Err(ChannelError::permanent("empty_text", "消息正文不能为空"));
        }
        if message.text.len() > self.capabilities().max_text_bytes.unwrap_or(usize::MAX) {
            return Err(ChannelError::permanent(
                "text_too_large",
                "消息正文超过渠道上限",
            ));
        }
        if self.unknown_on_send {
            return Err(ChannelError::unknown("channel_timeout", "渠道结果无法确认"));
        }
        Ok(DeliveryReceipt::sent(
            ExternalMessageId::new(format!("message-{}", account.id.as_str())).unwrap(),
        ))
    }

    async fn inspect(&self, _account: ChannelAccount) -> ChannelHealth {
        ChannelHealth::healthy()
    }

    async fn logout(&self, _account: ChannelAccount) -> Result<(), ChannelError> {
        Ok(())
    }
}

fn account(id: &str) -> ChannelAccount {
    ChannelAccount::new(
        ChannelAccountId::new(id).unwrap(),
        ChannelId::new("fake").unwrap(),
        format!("账号 {id}"),
        Timestamp::parse_rfc3339("2026-09-19T10:00:00Z").unwrap(),
    )
}

fn text_message(text: &str) -> OutboundMessage {
    OutboundMessage::notification("conversation-1", text, "client-1").unwrap()
}

#[tokio::test]
async fn contract_proves_account_isolation() {
    let adapter: Arc<dyn ChannelAdapter> = Arc::new(FakeChannel::new());
    assert_channel_contract(adapter).await;
}

#[tokio::test]
async fn timeout_maps_to_unknown_not_retryable() {
    let adapter = FakeChannel::unknown_on_send();
    let error = adapter
        .send(account("account-a"), text_message("hello"))
        .await
        .unwrap_err();
    assert!(matches!(error, ChannelError::Unknown(_)));
    assert!(!error.is_retryable());
}

#[test]
fn sent_receipt_requires_external_id_when_reply_routing_is_declared() {
    let receipt = DeliveryReceipt {
        external_message_id: None,
        external_thread_id: None,
        state: DeliveryState::Sent,
        error: Some(SafeError::new("missing_id", "缺少消息标识").unwrap()),
        raw_safe_metadata: Default::default(),
    };
    assert!(!receipt.is_valid_for(&FakeChannel::new().capabilities()));
}
