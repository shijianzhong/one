#![allow(dead_code)]

pub mod acp_adapter;
pub mod business;
pub mod claude_code;
pub mod intent;
pub mod types;

pub use types::*;

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use anyhow::Result;

#[async_trait::async_trait]
pub trait Agent: Send + Sync {
    fn agent_type(&self) -> &str;
    fn agent_name(&self) -> &str;

    async fn spawn(&self, config: AgentConfig) -> Result<AgentInstance>;
    async fn send_message(&self, instance: &mut AgentInstance, msg: &str) -> Result<String>;
    async fn get_status(&self, instance: &AgentInstance) -> AgentStatus;
    async fn pause(&self, instance: &mut AgentInstance) -> Result<()>;
    async fn resume(&self, instance: &mut AgentInstance) -> Result<()>;
    async fn destroy(&self, instance: &mut AgentInstance) -> Result<()>;
}

pub struct AgentManager {
    agents: Arc<Mutex<HashMap<usize, Arc<dyn Agent>>>>,
    instances: Arc<Mutex<HashMap<usize, AgentInstance>>>,
}

impl AgentManager {
    pub fn new() -> Self {
        Self {
            agents: Arc::new(Mutex::new(HashMap::new())),
            instances: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn register_agent(&self, agent_id: usize, agent: Arc<dyn Agent>) {
        let mut agents = self.agents.lock().await;
        agents.insert(agent_id, agent);
    }

    pub async fn unregister_agent(&self, agent_id: usize) {
        let mut agents = self.agents.lock().await;
        agents.remove(&agent_id);
    }

    pub async fn get_agent(&self, agent_id: usize) -> Option<Arc<dyn Agent>> {
        let agents = self.agents.lock().await;
        agents.get(&agent_id).cloned()
    }

    pub async fn add_instance(&self, instance: AgentInstance) {
        let mut instances = self.instances.lock().await;
        instances.insert(instance.id, instance);
    }

    pub async fn remove_instance(&self, instance_id: usize) {
        let mut instances = self.instances.lock().await;
        instances.remove(&instance_id);
    }

    pub async fn get_instance(&self, instance_id: usize) -> Option<AgentInstance> {
        let instances = self.instances.lock().await;
        instances.get(&instance_id).cloned()
    }

    pub async fn get_instance_by_task(&self, task_id: usize) -> Option<AgentInstance> {
        let instances = self.instances.lock().await;
        instances
            .values()
            .find(|i| i.task_id == Some(task_id))
            .cloned()
    }
}

impl Default for AgentManager {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for AgentManager {
    fn clone(&self) -> Self {
        Self {
            agents: self.agents.clone(),
            instances: self.instances.clone(),
        }
    }
}

pub struct AgentRegistry {
    agents: HashMap<String, AgentRow>,
}

impl AgentRegistry {
    pub fn new() -> Self {
        Self {
            agents: HashMap::new(),
        }
    }

    pub fn register(&mut self, agent_type: &str, agent: AgentRow) {
        self.agents.insert(agent_type.to_string(), agent);
    }

    pub fn get(&self, agent_type: &str) -> Option<&AgentRow> {
        self.agents.get(agent_type)
    }

    pub fn list(&self) -> Vec<&AgentRow> {
        self.agents.values().collect()
    }
}

impl Default for AgentRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// Helper struct for database agent row
#[derive(Debug, Clone)]
pub struct AgentRow {
    pub id: usize,
    pub name: String,
    pub agent_type: String,
    pub description: Option<String>,
    pub capabilities_json: Option<String>,
    pub config_json: Option<String>,
    pub memory_threshold: i64,
}
