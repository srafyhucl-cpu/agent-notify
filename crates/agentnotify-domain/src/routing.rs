use crate::{
    AgentId, AgentSessionId, ChannelAccountId, ChannelId, DomainError, ExternalMessageId, Timestamp,
};

/// 精确定位一条已投递消息所使用的渠道、账号和外部消息标识。
#[derive(Clone, Debug, Eq, Hash, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct RouteKey {
    pub channel_id: ChannelId,
    pub account_id: ChannelAccountId,
    pub external_message_id: ExternalMessageId,
}

impl RouteKey {
    pub const fn new(
        channel_id: ChannelId,
        account_id: ChannelAccountId,
        external_message_id: ExternalMessageId,
    ) -> Self {
        Self {
            channel_id,
            account_id,
            external_message_id,
        }
    }
}

/// 将已投递消息精确映射回原 Agent 会话的回复路由。
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct ReplyRoute {
    pub key: RouteKey,
    pub agent_id: AgentId,
    pub session_id: AgentSessionId,
    pub created_at: Timestamp,
    pub expires_at: Timestamp,
}

impl ReplyRoute {
    pub const fn new(
        key: RouteKey,
        agent_id: AgentId,
        session_id: AgentSessionId,
        created_at: Timestamp,
        expires_at: Timestamp,
    ) -> Self {
        Self {
            key,
            agent_id,
            session_id,
            created_at,
            expires_at,
        }
    }

    /// 路由在过期时刻立即失效，避免边界时刻产生歧义。
    pub fn is_active(&self, now: Timestamp) -> Result<(), DomainError> {
        if now < self.expires_at {
            Ok(())
        } else {
            Err(DomainError::RouteExpired)
        }
    }
}
