use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;

use super::{tool_registry, MainAgent, Orchestrator};
use crate::mcp::McpClientManager;
use crate::services::config::Config;

pub struct AgentFactory;

impl AgentFactory {
    pub fn create_orchestrator(
        config: &Config,
        workspace_name: &str,
        work_dir: PathBuf,
        mcp_manager: Option<Arc<std::sync::Mutex<McpClientManager>>>,
    ) -> Result<Orchestrator> {
        tool_registry::init_tool_registry(workspace_name);

        let main_agent = Arc::new(MainAgent::with_workspace(
            config.model_name.clone(),
            config.model_base_url.clone(),
            config.model_api_key.clone(),
            workspace_name.to_string(),
        ));

        Ok(Orchestrator::new(main_agent, work_dir, mcp_manager))
    }
}
