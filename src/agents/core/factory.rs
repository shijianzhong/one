use std::path::PathBuf;
use std::sync::Arc;
use anyhow::Result;

use super::orchestrator::Orchestrator;
use super::main_agent::MainAgent;
use crate::services::config::Config;

pub struct AgentFactory;

impl AgentFactory {
    pub fn create_orchestrator(config: &Config, workspace_name: &str, work_dir: PathBuf) -> Result<Orchestrator> {
        // MainAgent is the sole agent. It handles all conversation, memory, and
        // system/skill dispatch via its built-in tools. Specialized agents
        // (coding, system) have been removed — their capabilities are provided
        // through the Skill Market instead.
        let main_agent = Arc::new(MainAgent::with_workspace(
            config.model_name.clone(),
            config.model_base_url.clone(),
            config.model_api_key.clone(),
            workspace_name.to_string(),
        ));

        Ok(Orchestrator::new(main_agent, work_dir, None))
    }
}
