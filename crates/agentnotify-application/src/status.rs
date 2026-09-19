use std::{fmt::Display, sync::Arc};

use agentnotify_agent_sdk::{AgentHealth, AgentRegistry};
use agentnotify_channel_sdk::{ChannelHealth, ChannelRegistry};
use agentnotify_domain::{AgentId, ChannelAccountId, ChannelId, DomainArea};

use crate::{ChannelAccountStore, StatusSnapshot, StatusStore, StoreError, UseCase};

/// 单个 Agent 的脱敏状态。
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct AgentStatus {
    pub id: AgentId,
    pub display_name: String,
    pub health: AgentHealth,
}

/// 单个渠道账号的脱敏状态。
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct ChannelAccountStatus {
    pub account_id: ChannelAccountId,
    pub channel_id: ChannelId,
    pub display_name: String,
    pub enabled: bool,
    pub health: ChannelHealth,
}

/// 应用层状态模型，不包含密钥、正文或渠道原始响应。
#[derive(Clone, Debug, Default, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct StatusOverview {
    pub storage: StatusSnapshot,
    pub agents: Vec<AgentStatus>,
    pub channels: Vec<ChannelAccountStatus>,
}

impl StatusOverview {
    pub fn unhealthy_channel_count(&self) -> usize {
        self.channels
            .iter()
            .filter(|channel| !channel.enabled || !channel.health.available || channel.health.stale)
            .count()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StatusError {
    Store(StoreError),
}

impl StatusError {
    pub fn code(&self) -> &str {
        match self {
            Self::Store(error) => error.code(),
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Self::Store(error) => error.message(),
        }
    }
}

impl Display for StatusError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.message())
    }
}

impl std::error::Error for StatusError {}

impl From<StoreError> for StatusError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}

/// 汇总 Agent、渠道账号与持久化状态。
pub struct StatusService {
    agents: Arc<AgentRegistry>,
    channels: Arc<ChannelRegistry>,
    accounts: Arc<dyn ChannelAccountStore>,
    status_store: Arc<dyn StatusStore>,
}

impl StatusService {
    pub fn new(
        agents: Arc<AgentRegistry>,
        channels: Arc<ChannelRegistry>,
        accounts: Arc<dyn ChannelAccountStore>,
        status_store: Arc<dyn StatusStore>,
    ) -> Self {
        Self {
            agents,
            channels,
            accounts,
            status_store,
        }
    }

    pub async fn snapshot(&self) -> Result<StatusOverview, StatusError> {
        let storage = self.status_store.snapshot().await?;
        let mut agents = Vec::with_capacity(self.agents.all().len());
        for adapter in self.agents.all() {
            let descriptor = adapter.descriptor();
            let health = adapter.inspect().await;
            agents.push(AgentStatus {
                id: descriptor.id,
                display_name: descriptor.display_name,
                health,
            });
        }
        agents.sort_by(|left, right| left.id.cmp(&right.id));

        let mut channels = Vec::new();
        for adapter in self.channels.all() {
            let descriptor = adapter.descriptor();
            let mut accounts = self.accounts.list(&descriptor.id).await?;
            accounts.sort_by(|left, right| left.id.cmp(&right.id));
            for account in accounts {
                let health = adapter.inspect(account.clone()).await;
                channels.push(ChannelAccountStatus {
                    account_id: account.id,
                    channel_id: descriptor.id.clone(),
                    display_name: account.display_name,
                    enabled: account.enabled,
                    health,
                });
            }
        }
        channels.sort_by(|left, right| left.account_id.cmp(&right.account_id));

        Ok(StatusOverview {
            storage,
            agents,
            channels,
        })
    }
}

impl UseCase for StatusService {
    fn area(&self) -> DomainArea {
        DomainArea::Status
    }

    fn name(&self) -> &'static str {
        "status"
    }
}
