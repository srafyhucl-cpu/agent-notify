use std::{
    collections::HashMap,
    fmt::Display,
    sync::{Arc, RwLock},
};

use agentnotify_domain::AgentId;

use crate::AgentAdapter;

#[derive(Default)]
pub struct AgentRegistry {
    agents: RwLock<HashMap<AgentId, Arc<dyn AgentAdapter>>>,
}

impl AgentRegistry {
    pub fn register(&mut self, adapter: Arc<dyn AgentAdapter>) -> Result<(), AgentRegistryError> {
        let agent_id = adapter.descriptor().id;
        let mut agents = self
            .agents
            .write()
            .map_err(|_| AgentRegistryError::Unavailable)?;
        if agents.contains_key(&agent_id) {
            return Err(AgentRegistryError::AlreadyRegistered { agent_id });
        }
        agents.insert(agent_id, adapter);
        Ok(())
    }

    pub fn get(&self, agent_id: &AgentId) -> Option<Arc<dyn AgentAdapter>> {
        self.agents
            .read()
            .ok()
            .and_then(|agents| agents.get(agent_id).cloned())
    }

    pub fn all(&self) -> Vec<Arc<dyn AgentAdapter>> {
        let mut adapters = self
            .agents
            .read()
            .map(|agents| agents.values().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        adapters.sort_by(|left, right| left.descriptor().id.cmp(&right.descriptor().id));
        adapters
    }
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum AgentRegistryError {
    AlreadyRegistered { agent_id: AgentId },
    Unavailable,
}

impl AgentRegistryError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::AlreadyRegistered { .. } => "agent_already_registered",
            Self::Unavailable => "agent_registry_unavailable",
        }
    }
}

impl Display for AgentRegistryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyRegistered { agent_id } => {
                write!(formatter, "Agent 已注册: {agent_id}")
            }
            Self::Unavailable => formatter.write_str("Agent 注册表不可用"),
        }
    }
}

impl std::error::Error for AgentRegistryError {}
