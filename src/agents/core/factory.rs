use std::collections::HashMap;
use std::sync::Arc;
use anyhow::Result;

use super::{Agent, Coordinator, Orchestrator, SystemAgent, CodingAgent, MemoryAgent, GeneralAgent};
use crate::services::config::Config;

pub struct AgentFactory;

impl AgentFactory {
    pub fn create_orchestrator(config: &Config, workspace_name: &str) -> Result<Orchestrator> {
        let mut sub_agents: HashMap<String, Arc<dyn Agent>> = HashMap::new();

        // Each agent can use a different model for specialization
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

        let memory_agent = Arc::new(MemoryAgent::new(
            config.model_name.clone(),
            config.model_base_url.clone(),
            config.model_api_key.clone(),
            workspace_name.to_string(),
        ));

        let general_agent = Arc::new(GeneralAgent::new(
            config.model_name.clone(),
            config.model_base_url.clone(),
            config.model_api_key.clone(),
        ));

        sub_agents.insert(system_agent.id().to_string(), system_agent);
        sub_agents.insert(coding_agent.id().to_string(), coding_agent);
        sub_agents.insert(memory_agent.id().to_string(), memory_agent);
        sub_agents.insert(general_agent.id().to_string(), general_agent);

        // Coordinator uses the full-power model for planning
        let coordinator = Arc::new(Coordinator::new(
            config.model_name.clone(),
            config.model_base_url.clone(),
            config.model_api_key.clone(),
            sub_agents.clone(),
        ));

        Ok(Orchestrator::new(coordinator, sub_agents))
    }

    /// Create a lightweight General Agent for simple/quick questions
    pub fn create_general_agent(config: &Config) -> GeneralAgent {
        GeneralAgent::new(
            config.light_model.clone().unwrap_or_else(|| config.model_name.clone()),
            config.model_base_url.clone(),
            config.model_api_key.clone(),
        )
    }
}
