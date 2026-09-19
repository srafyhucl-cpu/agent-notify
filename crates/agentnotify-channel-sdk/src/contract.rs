use std::sync::Arc;

use agentnotify_domain::{ChannelAccountId, ChannelId, Timestamp};

use crate::{
    BeginLoginRequest, ChannelAccount, ChannelAdapter, ChannelError, ChannelLoginAdapter,
    MessagePurpose, OutboundMessage,
};

/// 在无网络条件下验证渠道适配器共有的稳定约束。
pub async fn assert_channel_contract(adapter: Arc<dyn ChannelAdapter>) {
    let descriptor = adapter.descriptor();
    assert!(
        !descriptor.display_name.trim().is_empty(),
        "渠道 descriptor display_name 不能为空"
    );
    assert_eq!(
        descriptor,
        adapter.descriptor(),
        "渠道 descriptor 必须保持稳定"
    );

    let capabilities = adapter.capabilities();
    assert!(capabilities.is_consistent(), "渠道能力声明自相矛盾");
    let account = contract_account(descriptor.id.clone());
    if !capabilities.send_text {
        let message = valid_message();
        let error = adapter.send(account, message).await.unwrap_err();
        assert!(
            matches!(error, ChannelError::UnsupportedCapability(_)),
            "不支持文本发送的渠道必须返回 UnsupportedCapability"
        );
        return;
    }

    let empty_message = OutboundMessage {
        purpose: MessagePurpose::Notification,
        conversation_id: "contract-conversation".into(),
        text: String::new(),
        client_id: "contract-client".into(),
        reply_to: None,
        safe_metadata: Default::default(),
    };
    let error = adapter
        .send(account.clone(), empty_message)
        .await
        .unwrap_err();
    assert!(
        matches!(error, ChannelError::Permanent(_)),
        "渠道必须在访问网络前拒绝空正文"
    );

    if let Some(max_text_bytes) = capabilities.max_text_bytes {
        let oversized = OutboundMessage {
            purpose: MessagePurpose::Notification,
            conversation_id: "contract-conversation".into(),
            text: "x".repeat(max_text_bytes + 1),
            client_id: "contract-client".into(),
            reply_to: None,
            safe_metadata: Default::default(),
        };
        let error = adapter.send(account, oversized).await.unwrap_err();
        assert!(
            matches!(error, ChannelError::Permanent(_)),
            "渠道必须在访问网络前拒绝超过上限的正文"
        );
    }
}

/// 登录契约只验证验证码与取消语义，不访问真实渠道。
pub async fn assert_channel_login_contract(adapter: Arc<dyn ChannelLoginAdapter>) {
    let session = adapter
        .begin_login(BeginLoginRequest::new("contract-account"))
        .await
        .expect("登录适配器必须能创建内存会话");
    let error = adapter
        .submit_login_code(session.id(), " ")
        .await
        .unwrap_err();
    assert!(
        matches!(error, ChannelError::Permanent(_)),
        "登录适配器必须拒绝空验证码"
    );
    adapter
        .cancel_login(session.id())
        .await
        .expect("登录会话必须可以取消");
    assert!(
        adapter
            .submit_login_code(session.id(), "123456")
            .await
            .is_err(),
        "取消后的登录会话不能继续使用"
    );
}

fn contract_account(channel_id: ChannelId) -> ChannelAccount {
    ChannelAccount::new(
        ChannelAccountId::new("contract-account").unwrap(),
        channel_id,
        "契约账号",
        Timestamp::parse_rfc3339("2026-09-19T10:00:00Z").unwrap(),
    )
}

fn valid_message() -> OutboundMessage {
    OutboundMessage::notification("contract-conversation", "contract", "contract-client")
        .expect("契约消息必须有效")
}
