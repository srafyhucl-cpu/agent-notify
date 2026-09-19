use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use agentnotify_channel_sdk::{
    ChannelAccount, ChannelAdapter, ChannelCapabilities, ChannelDescriptor, ChannelError,
    ChannelHealth, ChannelTask, DeliveryReceipt, InboundEmitter, InboundMode, OutboundMessage,
};
use agentnotify_domain::{ChannelId, DeliveryState, ExternalMessageId, SafeError};

#[derive(Clone)]
pub struct FakeChannel {
    id: ChannelId,
    capabilities: ChannelCapabilities,
    unknown_on_send: bool,
    receipts: Arc<Mutex<HashMap<String, DeliveryReceipt>>>,
}

impl FakeChannel {
    pub fn new() -> Self {
        Self {
            id: ChannelId::new("fake").unwrap(),
            capabilities: ChannelCapabilities {
                send_text: true,
                receive: true,
                reply_routing: true,
                edit_message: false,
                attachments: false,
                markdown: true,
                max_text_bytes: Some(1024),
                inbound_modes: vec![InboundMode::LongPolling],
            },
            unknown_on_send: false,
            receipts: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn unknown_on_send() -> Self {
        let mut channel = Self::new();
        channel.unknown_on_send = true;
        channel
    }

    pub fn set_receipt(&self, account_id: &str, receipt: DeliveryReceipt) {
        self.receipts
            .lock()
            .expect("FakeChannel 回执锁不应失败")
            .insert(account_id.into(), receipt);
    }

    pub fn last_receipt(&self, account_id: &str) -> Option<DeliveryReceipt> {
        self.receipts
            .lock()
            .expect("FakeChannel 回执锁不应失败")
            .get(account_id)
            .cloned()
    }
}

impl Default for FakeChannel {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl ChannelAdapter for FakeChannel {
    fn descriptor(&self) -> ChannelDescriptor {
        ChannelDescriptor {
            id: self.id.clone(),
            display_name: "Fake Channel".into(),
            config_schema: serde_json::json!({"type": "object"}),
        }
    }

    fn capabilities(&self) -> ChannelCapabilities {
        self.capabilities.clone()
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
        if let Some(max_text_bytes) = self.capabilities.max_text_bytes {
            if message.text.len() > max_text_bytes {
                return Err(ChannelError::permanent(
                    "text_too_large",
                    "消息正文超过渠道上限",
                ));
            }
        }
        if self.unknown_on_send {
            return Err(ChannelError::unknown("channel_timeout", "渠道结果无法确认"));
        }
        if let Some(receipt) = self.last_receipt(account.id.as_str()) {
            return Ok(receipt);
        }
        let receipt = DeliveryReceipt {
            external_message_id: Some(
                ExternalMessageId::new(format!("message-{}", account.id.as_str())).unwrap(),
            ),
            external_thread_id: None,
            state: DeliveryState::Sent,
            error: None,
            raw_safe_metadata: Default::default(),
        };
        self.receipts
            .lock()
            .expect("FakeChannel 回执锁不应失败")
            .insert(account.id.as_str().into(), receipt.clone());
        Ok(receipt)
    }

    async fn inspect(&self, _account: ChannelAccount) -> ChannelHealth {
        ChannelHealth::healthy()
    }

    async fn logout(&self, _account: ChannelAccount) -> Result<(), ChannelError> {
        Ok(())
    }
}

pub fn fake_skip_reason(code: &str, message: &str) -> SafeError {
    SafeError::new(code, message).expect("FakeChannel 安全错误必须有效")
}
