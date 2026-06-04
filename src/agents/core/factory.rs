use std::collections::HashMap;
use std::sync::Arc;
use anyhow::Result;

use super::{Agent, Orchestrator, SystemAgent, CodingAgent, MemoryAgent, MainAgent};
use crate::services::config::Config;

pub struct AgentFactory;

impl AgentFactory {
    pub fn create_orchestrator(config: &Config, _workspace_name: &str) -> Result<Orchestrator> {
        let mut sub_agents: HashMap<String, Arc<dyn Agent>> = HashMap::new();

        // Sub-agents are now used as "specialized tools" called by MainAgent
        let system_agent = Arc::new(SystemAgent::new(
            config.system_model.clone().unwrap_or_else(|| config.model_name.clone()),
            config.model_base_url.clone(),
            config.model_api_key.clone(),
        ));

        let coding_agent = Arc::new(CodingAgent::new(
            config.coding_model.clone().unwrap_or_else(|| config.model_name.clone()),
            config.model_base_url.clone(),
            config.model_api_key.clone(),
        ));

        // Note: Memory logic is now partly built into MainAgent, but we keep
        // MemoryAgent if we want standalone memory background tasks.
        let memory_agent = Arc::new(MemoryAgent::new(
            config.model_name.clone(),
            config.model_base_url.clone(),
            config.model_api_key.clone(),
            ".".to_string(), // Use root for global profile or adjust as needed
        ));

        sub_agents.insert(system_agent.id().to_string(), system_agent);
        sub_agents.insert(coding_agent.id().to_string(), coding_agent);
        sub_agents.insert(memory_agent.id().to_string(), memory_agent);

        // MainAgent owns the primary conversation and calls specialized tools/sub-agents.
        let main_agent = Arc::new(MainAgent::new(
            config.model_name.clone(),
            config.model_base_url.clone(),
            config.model_api_key.clone(),
        ));

        Ok(Orchestrator::new(main_agent, sub_agents))
    }
}
