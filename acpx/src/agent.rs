//! ACP Agent Trait
//!
//! Core trait for implementing ACP agents.

use std::sync::Arc;

use async_trait::async_trait;

use crate::protocol::AgentCapabilities;
use crate::error::AcpError;
use crate::protocol::{
    SessionCloseParams, SessionCloseResult, SessionListResult, SessionLoadParams,
    SessionNewParams, SessionNewResult, SessionPromptParams, SessionPromptResult,
    SessionResumeParams, SessionSetModeParams, SessionSetModeResult,
};
use crate::session::SessionManager;

/// Agent trait - implemented by all ACP agents
#[async_trait]
pub trait Agent: Send + Sync {
    /// Get agent type identifier
    fn agent_type(&self) -> &str;

    /// Get agent display name
    fn agent_name(&self) -> &str;

    /// Get agent capabilities
    fn get_capabilities(&self) -> AgentCapabilities;

    /// Get session manager (if agent manages sessions internally)
    fn get_session_manager(&self) -> Option<Arc<SessionManager>> {
        None
    }

    /// Initialize the agent
    async fn initialize(&self) -> Result<AgentCapabilities, AcpError> {
        Ok(self.get_capabilities())
    }

    /// Create a new session
    async fn session_new(&self, params: SessionNewParams) -> Result<SessionNewResult, AcpError>;

    /// Send a prompt to a session
    async fn session_prompt(
        &self,
        params: SessionPromptParams,
    ) -> Result<SessionPromptResult, AcpError>;

    /// Cancel an ongoing operation
    async fn session_cancel(
        &self,
        params: crate::protocol::SessionCancelParams,
    ) -> Result<crate::protocol::SessionCancelResult, AcpError>;

    /// Load a session by ID (replay history)
    async fn session_load(&self, params: SessionLoadParams) -> Result<SessionNewResult, AcpError>;

    /// Resume a session without replay
    async fn session_resume(&self, params: SessionResumeParams) -> Result<SessionNewResult, AcpError>;

    /// Close a session
    async fn session_close(&self, params: SessionCloseParams) -> Result<SessionCloseResult, AcpError>;

    /// Set session mode
    async fn session_set_mode(
        &self,
        params: SessionSetModeParams,
    ) -> Result<SessionSetModeResult, AcpError>;

    /// List available sessions
    async fn session_list(&self) -> Result<SessionListResult, AcpError>;

    /// Check if agent is available/healthy
    async fn is_available(&self) -> bool {
        true
    }
}

/// Agent instance wrapper for managing agent state
pub struct AgentInstance {
    pub agent: Arc<dyn Agent>,
    pub instance_id: usize,
}

impl AgentInstance {
    pub fn new(agent: Arc<dyn Agent>, instance_id: usize) -> Self {
        Self { agent, instance_id }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::SessionInfo;
    use std::path::PathBuf;

    // Mock agent for testing
    struct MockAgent {
        capabilities: AgentCapabilities,
    }

    impl MockAgent {
        fn new() -> Self {
            Self {
                capabilities: AgentCapabilities::default(),
            }
        }
    }

    #[async_trait]
    impl Agent for MockAgent {
        fn agent_type(&self) -> &str {
            "mock"
        }

        fn agent_name(&self) -> &str {
            "Mock Agent"
        }

        fn get_capabilities(&self) -> AgentCapabilities {
            self.capabilities.clone()
        }

        async fn session_new(
            &self,
            params: SessionNewParams,
        ) -> Result<SessionNewResult, AcpError> {
            Ok(SessionNewResult {
                session_id: format!("session-{}", params.cwd),
                agent_session_id: Some("agent-session-123".to_string()),
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
            params: crate::protocol::SessionCancelParams,
        ) -> Result<crate::protocol::SessionCancelResult, AcpError> {
            Ok(crate::protocol::SessionCancelResult {
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
                agent_session_id: Some("agent-session-loaded".to_string()),
            })
        }

        async fn session_resume(
            &self,
            params: SessionResumeParams,
        ) -> Result<SessionNewResult, AcpError> {
            Ok(SessionNewResult {
                session_id: params.session_id,
                agent_session_id: Some("agent-session-resumed".to_string()),
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
            Ok(SessionListResult {
                sessions: vec![SessionInfo {
                    session_id: "test-session".to_string(),
                    cwd: "/tmp".to_string(),
                    created_at: "2025-01-01T00:00:00Z".to_string(),
                    last_used_at: "2025-01-01T00:00:00Z".to_string(),
                }],
            })
        }
    }

    #[tokio::test]
    async fn test_mock_agent_session_new() {
        let agent = Arc::new(MockAgent::new());

        let result = agent
            .session_new(SessionNewParams {
                cwd: "/tmp/test".to_string(),
                additional_directories: vec![],
                mcp_servers: vec![],
            })
            .await;

        assert!(result.is_ok());
        let result = result.unwrap();
        assert!(result.session_id.contains("session-"));
    }

    #[tokio::test]
    async fn test_mock_agent_session_prompt() {
        let agent = Arc::new(MockAgent::new());

        let result = agent
            .session_prompt(SessionPromptParams {
                session_id: "test-session".to_string(),
                content: vec![],
                system_prompt: None,
                mode: None,
            })
            .await;

        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.stop_reason, crate::protocol::StopReason::EndTurn);
    }

    #[tokio::test]
    async fn test_mock_agent_session_list() {
        let agent = Arc::new(MockAgent::new());

        let result = agent.session_list().await;

        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.sessions.len(), 1);
    }
}
