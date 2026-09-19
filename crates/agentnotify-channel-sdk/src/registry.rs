use std::{
    collections::HashMap,
    fmt::Display,
    sync::{Arc, RwLock},
};

use agentnotify_domain::ChannelId;

use crate::ChannelAdapter;

#[derive(Default)]
pub struct ChannelRegistry {
    channels: RwLock<HashMap<ChannelId, Arc<dyn ChannelAdapter>>>,
}

impl ChannelRegistry {
    pub fn register(
        &mut self,
        adapter: Arc<dyn ChannelAdapter>,
    ) -> Result<(), ChannelRegistryError> {
        let channel_id = adapter.descriptor().id;
        let mut channels = self
            .channels
            .write()
            .map_err(|_| ChannelRegistryError::Unavailable)?;
        if channels.contains_key(&channel_id) {
            return Err(ChannelRegistryError::AlreadyRegistered { channel_id });
        }
        channels.insert(channel_id, adapter);
        Ok(())
    }

    pub fn get(&self, channel_id: &ChannelId) -> Option<Arc<dyn ChannelAdapter>> {
        self.channels
            .read()
            .ok()
            .and_then(|channels| channels.get(channel_id).cloned())
    }

    pub fn all(&self) -> Vec<Arc<dyn ChannelAdapter>> {
        let mut adapters = self
            .channels
            .read()
            .map(|channels| channels.values().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        adapters.sort_by(|left, right| left.descriptor().id.cmp(&right.descriptor().id));
        adapters
    }
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum ChannelRegistryError {
    AlreadyRegistered { channel_id: ChannelId },
    Unavailable,
}

impl ChannelRegistryError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::AlreadyRegistered { .. } => "channel_already_registered",
            Self::Unavailable => "channel_registry_unavailable",
        }
    }
}

impl Display for ChannelRegistryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyRegistered { channel_id } => {
                write!(formatter, "渠道已注册: {channel_id}")
            }
            Self::Unavailable => formatter.write_str("渠道注册表不可用"),
        }
    }
}

impl std::error::Error for ChannelRegistryError {}
