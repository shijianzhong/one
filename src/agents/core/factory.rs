use std::collections::HashMap;
use std::sync::Arc;
use anyhow::Result;

use super::{Agent, Orchestrator, SystemAgent, CodingAgent, MainAgent};
use crate::services::config::Config;

pub struct AgentFactory;

impl AgentFactory {
    pub fn create_orchestrator(config: &Config, workspace_name: &str) -> Result<Orchestrator> {
        let mut sub_agents: HashMap<String, Arc<dyn Agent>> = HashMap::new();

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

        sub_agents.insert(system_agent.id().to_string(), system_agent);
        sub_agents.insert(coding_agent.id().to_string(), coding_agent);

        // MainAgent owns the primary conversation and calls specialized tools/sub-agents.
        // Memory is fully integrated into MainAgent via RememberTool and RecallTool.
        let main_agent = Arc::new(MainAgent::with_workspace(
            config.model_name.clone(),
            config.model_base_url.clone(),
            config.model_api_key.clone(),
            workspace_name.to_string(),
        ));

        Ok(Orchestrator::new(main_agent, sub_agents))
    }
}
