//! ACP Agent Registry
//!
//! Central registry for managing multiple ACP agents.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::agent::Agent;
use crate::protocol::AgentCapabilities;
use crate::error::AcpError;
use crate::protocol::{
    SessionCancelParams, SessionCancelResult, SessionCloseParams, SessionCloseResult,
    SessionListResult, SessionLoadParams, SessionNewParams, SessionNewResult,
    SessionPromptParams, SessionPromptResult, SessionResumeParams, SessionSetModeParams,
    SessionSetModeResult,
};
use crate::session::SessionManager;

/// Agent registry entry
#[derive(Clone)]
pub struct AgentEntry {
    pub name: String,
    pub agent_type: String,
    pub description: Option<String>,
    pub agent: Arc<dyn Agent>,
    pub capabilities: AgentCapabilities,
}

impl AgentEntry {
    pub fn new(name: &str, agent_type: &str, agent: Arc<dyn Agent>) -> Self {
        let description = Some(format!("{} agent", agent.agent_name()));
        Self {
            name: name.to_string(),
            agent_type: agent_type.to_string(),
            description,
            agent,
            capabilities: AgentCapabilities::default(),
        }
    }

    pub fn with_description(mut self, description: &str) -> Self {
        self.description = Some(description.to_string());
        self
    }
}

/// ACP Agent Registry
pub struct Registry {
    agents: Arc<RwLock<HashMap<String, AgentEntry>>>,
    session_manager: Arc<SessionManager>,
}

impl Registry {
    /// Create a new registry
    pub fn new() -> Self {
        Self {
            agents: Arc::new(RwLock::new(HashMap::new())),
            session_manager: Arc::new(SessionManager::new()),
        }
    }

    /// Create a new registry with a custom session manager
    pub fn with_session_manager(session_manager: Arc<SessionManager>) -> Self {
        Self {
            agents: Arc::new(RwLock::new(HashMap::new())),
            session_manager,
        }
    }

    /// Register an agent
    pub async fn register(
        &self,
        name: &str,
        agent_type: &str,
        agent: Arc<dyn Agent>,
    ) -> Result<(), AcpError> {
        let mut agents = self.agents.write().await;

        if agents.contains_key(name) {
            return Err(AcpError::InvalidRequest(format!(
                "Agent '{}' is already registered",
                name
            )));
        }

        let entry = AgentEntry::new(name, agent_type, agent);
        agents.insert(name.to_string(), entry);

        Ok(())
    }

    /// Register an agent with custom configuration
    pub async fn register_with(
        &self,
        name: &str,
        agent_type: &str,
        agent: Arc<dyn Agent>,
        description: &str,
    ) -> Result<(), AcpError> {
        let mut agents = self.agents.write().await;

        if agents.contains_key(name) {
            return Err(AcpError::InvalidRequest(format!(
                "Agent '{}' is already registered",
                name
            )));
        }

        let entry = AgentEntry::new(name, agent_type, agent).with_description(description);
        agents.insert(name.to_string(), entry);

        Ok(())
    }

    /// Unregister an agent
    pub async fn unregister(&self, name: &str) -> Option<AgentEntry> {
        let mut agents = self.agents.write().await;
        agents.remove(name)
    }

    /// Get an agent by name
    pub async fn get(&self, name: &str) -> Option<Arc<dyn Agent>> {
        let agents = self.agents.read().await;
        agents.get(name).map(|e| e.agent.clone())
    }

    /// Get agent entry by name
    pub async fn get_entry(&self, name: &str) -> Option<AgentEntry> {
        let agents = self.agents.read().await;
        agents.get(name).cloned()
    }

    /// List all registered agent names
    pub async fn list_agents(&self) -> Vec<String> {
        let agents = self.agents.read().await;
        agents.keys().cloned().collect()
    }

    /// List all agent entries
    pub async fn list_entries(&self) -> Vec<AgentEntry> {
        let agents = self.agents.read().await;
        agents.values().cloned().collect()
    }

    /// Get session manager
    pub fn session_manager(&self) -> Arc<SessionManager> {
        self.session_manager.clone()
    }

    /// Check if an agent is registered
    pub async fn contains(&self, name: &str) -> bool {
        let agents = self.agents.read().await;
        agents.contains_key(name)
    }

    // ========================================================================
    // Session operations - delegate to the appropriate agent
    // ========================================================================

    /// Create a new session with the specified agent
    pub async fn session_new(
        &self,
        agent_name: &str,
        params: SessionNewParams,
    ) -> Result<SessionNewResult, AcpError> {
        let agent = self.get(agent_name).await.ok_or_else(|| {
            AcpError::AgentNotAvailable(agent_name.to_string())
        })?;

        agent.session_new(params).await
    }

    /// Send a prompt to a session
    pub async fn session_prompt(
        &self,
        agent_name: &str,
        params: SessionPromptParams,
    ) -> Result<SessionPromptResult, AcpError> {
        let agent = self.get(agent_name).await.ok_or_else(|| {
            AcpError::AgentNotAvailable(agent_name.to_string())
        })?;

        agent.session_prompt(params).await
    }

    /// Cancel an ongoing operation
    pub async fn session_cancel(
        &self,
        agent_name: &str,
        params: SessionCancelParams,
    ) -> Result<SessionCancelResult, AcpError> {
        let agent = self.get(agent_name).await.ok_or_else(|| {
            AcpError::AgentNotAvailable(agent_name.to_string())
        })?;

        agent.session_cancel(params).await
    }

    /// Load a session
    pub async fn session_load(
        &self,
        agent_name: &str,
        params: SessionLoadParams,
    ) -> Result<SessionNewResult, AcpError> {
        let agent = self.get(agent_name).await.ok_or_else(|| {
            AcpError::AgentNotAvailable(agent_name.to_string())
        })?;

        agent.session_load(params).await
    }

    /// Resume a session
    pub async fn session_resume(
        &self,
        agent_name: &str,
        params: SessionResumeParams,
    ) -> Result<SessionNewResult, AcpError> {
        let agent = self.get(agent_name).await.ok_or_else(|| {
            AcpError::AgentNotAvailable(agent_name.to_string())
        })?;

        agent.session_resume(params).await
    }

    /// Close a session
    pub async fn session_close(
        &self,
        agent_name: &str,
        params: SessionCloseParams,
    ) -> Result<SessionCloseResult, AcpError> {
        let agent = self.get(agent_name).await.ok_or_else(|| {
            AcpError::AgentNotAvailable(agent_name.to_string())
        })?;

        agent.session_close(params).await
    }

    /// Set session mode
    pub async fn session_set_mode(
        &self,
        agent_name: &str,
        params: SessionSetModeParams,
    ) -> Result<SessionSetModeResult, AcpError> {
        let agent = self.get(agent_name).await.ok_or_else(|| {
            AcpError::AgentNotAvailable(agent_name.to_string())
        })?;

        agent.session_set_mode(params).await
    }

    /// List sessions
    pub async fn session_list(
        &self,
        agent_name: &str,
    ) -> Result<SessionListResult, AcpError> {
        let agent = self.get(agent_name).await.ok_or_else(|| {
            AcpError::AgentNotAvailable(agent_name.to_string())
        })?;

        agent.session_list().await
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for Registry {
    fn clone(&self) -> Self {
        Self {
            agents: self.agents.clone(),
            session_manager: self.session_manager.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Create a mock agent for testing
    struct TestAgent;

    #[async_trait::async_trait]
    impl Agent for TestAgent {
        fn agent_type(&self) -> &str {
            "test"
        }

        fn agent_name(&self) -> &str {
            "Test Agent"
        }

        fn get_capabilities(&self) -> AgentCapabilities {
            AgentCapabilities::default()
        }

        async fn session_new(
            &self,
            params: SessionNewParams,
        ) -> Result<SessionNewResult, AcpError> {
            Ok(SessionNewResult {
                session_id: format!("test-session-{}", params.cwd),
                agent_session_id: Some("test-agent-session".to_string()),
            })
        }

        async fn session_prompt(
            &self,
            params: SessionPromptParams,
        ) -> Result<SessionPromptResult, AcpError> {
            Ok(SessionPromptResult {
                session_id: params.session_id,
                stop_reason: crate::protocol::StopReason::EndTurn,
            })
        }

        async fn session_cancel(
            &self,
            params: SessionCancelParams,
        ) -> Result<SessionCancelResult, AcpError> {
            Ok(SessionCancelResult {
                session_id: params.session_id,
                stop_reason: crate::protocol::StopReason::Cancelled,
            })
        }

        async fn session_load(
            &self,
            params: SessionLoadParams,
        ) -> Result<SessionNewResult, AcpError> {
            Ok(SessionNewResult {
                session_id: params.session_id,
                agent_session_id: Some("loaded".to_string()),
            })
        }

        async fn session_resume(
            &self,
            params: SessionResumeParams,
        ) -> Result<SessionNewResult, AcpError> {
            Ok(SessionNewResult {
                session_id: params.session_id,
                agent_session_id: Some("resumed".to_string()),
            })
        }

        async fn session_close(
            &self,
            params: SessionCloseParams,
        ) -> Result<SessionCloseResult, AcpError> {
            Ok(SessionCloseResult {
                session_id: params.session_id,
            })
        }

        async fn session_set_mode(
            &self,
            params: SessionSetModeParams,
        ) -> Result<SessionSetModeResult, AcpError> {
            Ok(SessionSetModeResult {
                session_id: params.session_id,
                mode: params.mode,
            })
        }

        async fn session_list(&self) -> Result<SessionListResult, AcpError> {
            Ok(SessionListResult { sessions: vec![] })
        }
    }

    #[tokio::test]
    async fn test_register_agent() {
        let registry = Registry::new();
        let agent = Arc::new(TestAgent);

        registry
            .register("test", "test", agent)
            .await
            .unwrap();

        assert!(registry.contains("test").await);
    }

    #[tokio::test]
    async fn test_get_agent() {
        let registry = Registry::new();
        let agent = Arc::new(TestAgent);

        registry
            .register("test", "test", agent)
            .await
            .unwrap();

        let retrieved = registry.get("test").await;
        assert!(retrieved.is_some());
    }

    #[tokio::test]
    async fn test_session_new() {
        let registry = Registry::new();
        let agent = Arc::new(TestAgent);

        registry
            .register("test", "test", agent)
            .await
            .unwrap();

        let result = registry
            .session_new("test", SessionNewParams {
                cwd: "/tmp".to_string(),
                additional_directories: vec![],
                mcp_servers: vec![],
            })
            .await;

        assert!(result.is_ok());
        let result = result.unwrap();
        assert!(result.session_id.contains("test-session"));
    }

    #[tokio::test]
    async fn test_session_prompt() {
        let registry = Registry::new();
        let agent = Arc::new(TestAgent);

        registry
            .register("test", "test", agent)
            .await
            .unwrap();

        let result = registry
            .session_prompt(
                "test",
                SessionPromptParams {
                    session_id: "test-session".to_string(),
                    content: vec![],
                    system_prompt: None,
                    mode: None,
                },
            )
            .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_agent_not_found() {
        let registry = Registry::new();

        let result = registry
            .session_new("nonexistent", SessionNewParams {
                cwd: "/tmp".to_string(),
                additional_directories: vec![],
                mcp_servers: vec![],
            })
            .await;

        assert!(result.is_err());
    }
}
