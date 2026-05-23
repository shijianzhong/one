//! ACP Agent Adapter
//!
//! Adapter that wraps acpx::Agent to work with the project's Agent trait.

#![allow(dead_code)]

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;

use crate::agents::{Agent, AgentConfig, AgentInstance, AgentStatus};
use acpx;
use acpx::protocol::{ContentBlock, SessionNewParams, SessionPromptParams};

/// Adapter that wraps acpx::Agent to implement the project's Agent trait
pub struct AcpAgentAdapter {
    acp_agent: Arc<dyn acpx::Agent>,
}

impl AcpAgentAdapter {
    pub fn new(acp_agent: Arc<dyn acpx::Agent>) -> Self {
        Self { acp_agent }
    }

    pub fn into_dyn(self) -> Arc<dyn Agent>
    where
        Self: Agent + 'static,
    {
        Arc::new(self)
    }
}

#[async_trait]
impl Agent for AcpAgentAdapter {
    fn agent_type(&self) -> &str {
        "acp_agent"
    }

    fn agent_name(&self) -> &str {
        "ACP Agent"
    }

    async fn spawn(&self, config: AgentConfig) -> Result<AgentInstance> {
        let params = SessionNewParams {
            cwd: config.name.clone(),
            additional_directories: vec![],
            mcp_servers: vec![],
        };

        let result = self
            .acp_agent
            .session_new(params)
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        Ok(AgentInstance {
            id: 0,
            agent_id: 0,
            task_id: None,
            status: AgentStatus::Idle,
            session_state: serde_json::json!({
                "session_id": result.session_id,
                "agent_session_id": result.agent_session_id,
            }),
        })
    }

    async fn send_message(&self, instance: &mut AgentInstance, msg: &str) -> Result<String> {
        let session_id = instance
            .session_state
            .get("session_id")
            .and_then(|v| v.as_str())
            .unwrap_or("default");

        let params = SessionPromptParams {
            session_id: session_id.to_string(),
            content: vec![ContentBlock::Text {
                text: msg.to_string(),
            }],
            system_prompt: None,
            mode: None,
        };

        let result = self
            .acp_agent
            .session_prompt(params)
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        instance.status = AgentStatus::Idle;
        Ok(format!(
            "Session {} completed with reason: {:?}",
            result.session_id, result.stop_reason
        ))
    }

    async fn get_status(&self, instance: &AgentInstance) -> AgentStatus {
        instance.status
    }

    async fn pause(&self, _instance: &mut AgentInstance) -> Result<()> {
        Ok(())
    }

    async fn resume(&self, _instance: &mut AgentInstance) -> Result<()> {
        Ok(())
    }

    async fn destroy(&self, _instance: &mut AgentInstance) -> Result<()> {
        Ok(())
    }
}

/// Routing decision adapter - converts ACP routing to project routing
pub fn classify_with_acp(
    _message: &str,
    _acp_router: Option<&acpx::registry::Registry>,
) -> Option<crate::agents::RoutingDecision> {
    // If we have an ACP registry, we could use it to route
    // For now, return None to fall back to default routing
    None
}
