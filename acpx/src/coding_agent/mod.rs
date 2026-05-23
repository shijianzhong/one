//! Claude Code Agent Implementation
//!
//! ACP agent implementation that wraps Claude Code CLI.

pub mod cli;
pub mod process;

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tokio::sync::RwLock;

use crate::agent::Agent;
use crate::error::AcpError;
use crate::protocol::AgentCapabilities;
use crate::protocol::{
    SessionCancelParams, SessionCancelResult, SessionCloseParams, SessionCloseResult, SessionInfo,
    SessionListResult, SessionLoadParams, SessionNewParams, SessionNewResult, SessionPromptParams,
    SessionPromptResult, SessionResumeParams, SessionSetModeParams, SessionSetModeResult,
    StopReason,
};
use crate::session::SessionManager;

use self::cli::ClaudeCli;

/// Claude Code Agent
pub struct ClaudeCodeAgent {
    cli: ClaudeCli,
    session_manager: Arc<SessionManager>,
    /// Internal session mapping: ACP session_id -> Claude session info
    sessions: Arc<RwLock<Vec<ClaudeSession>>>,
}

/// Claude-specific session data
#[derive(Debug, Clone)]
struct ClaudeSession {
    /// ACP session ID
    acp_session_id: String,
    /// Claude CLI session ID (for --resume)
    claude_session_id: Option<String>,
    /// Working directory
    cwd: PathBuf,
    /// Mode (auto, plan, etc)
    mode: Option<String>,
    /// Created at
    created_at: DateTime<Utc>,
}

impl ClaudeCodeAgent {
    /// Create a new Claude Code agent
    pub fn new() -> Self {
        Self {
            cli: ClaudeCli::new(),
            session_manager: Arc::new(SessionManager::new()),
            sessions: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Create with a specific CLI path
    pub fn with_cli_path(path: PathBuf) -> Self {
        Self {
            cli: ClaudeCli::with_path(path),
            session_manager: Arc::new(SessionManager::new()),
            sessions: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Check if Claude CLI is available
    pub fn is_available(&self) -> bool {
        self.cli.is_available()
    }

    /// Get CLI path
    pub fn get_cli_path() -> Option<PathBuf> {
        ClaudeCli::get_path()
    }

    async fn get_session(&self, acp_session_id: &str) -> Option<ClaudeSession> {
        let sessions = self.sessions.read().await;
        sessions
            .iter()
            .find(|s| s.acp_session_id == acp_session_id)
            .cloned()
    }

    async fn save_session(&self, session: ClaudeSession) {
        let mut sessions = self.sessions.write().await;
        // Remove existing if present
        sessions.retain(|s| s.acp_session_id != session.acp_session_id);
        sessions.push(session);
    }

    async fn remove_session(&self, acp_session_id: &str) -> Option<ClaudeSession> {
        let mut sessions = self.sessions.write().await;
        let idx = sessions
            .iter()
            .position(|s| s.acp_session_id == acp_session_id)?;
        Some(sessions.remove(idx))
    }
}

impl Default for ClaudeCodeAgent {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Agent for ClaudeCodeAgent {
    fn agent_type(&self) -> &str {
        "claude_code"
    }

    fn agent_name(&self) -> &str {
        "Claude Code"
    }

    fn get_capabilities(&self) -> AgentCapabilities {
        AgentCapabilities {
            load_session: true,
            prompt_capabilities: crate::protocol::PromptCapabilities {
                image: false,
                audio: false,
                embedded_context: true,
            },
            mcp_capabilities: crate::protocol::McpCapabilities {
                http: false,
                sse: false,
            },
        }
    }

    fn get_session_manager(&self) -> Option<Arc<SessionManager>> {
        Some(self.session_manager.clone())
    }

    async fn session_new(&self, params: SessionNewParams) -> Result<SessionNewResult, AcpError> {
        // Create session in session manager
        let session = self
            .session_manager
            .create_session(&PathBuf::from(&params.cwd))
            .await?;

        // Prepare Claude session data
        let mut claude_session = ClaudeSession {
            acp_session_id: session.id.clone(),
            claude_session_id: None,
            cwd: PathBuf::from(&params.cwd),
            mode: params.mcp_servers.first().map(|_| "auto".to_string()),
            created_at: Utc::now(),
        };

        // Execute session creation with Claude CLI
        match self
            .cli
            .session_new(&params.cwd, params.mcp_servers.as_slice())
            .await
        {
            Ok((_session_id, agent_session_id)) => {
                let agent_session_id = agent_session_id.clone();
                claude_session.claude_session_id = Some(agent_session_id.clone());
                self.save_session(claude_session).await;

                let session_id = session.id.clone();
                Ok(SessionNewResult {
                    session_id,
                    agent_session_id: Some(agent_session_id),
                })
            }
            Err(e) => {
                self.session_manager.close_session(&session.id).await.ok();
                Err(e.into())
            }
        }
    }

    async fn session_prompt(
        &self,
        params: SessionPromptParams,
    ) -> Result<SessionPromptResult, AcpError> {
        // Get session info
        let session = self
            .get_session(&params.session_id)
            .await
            .ok_or_else(|| AcpError::SessionNotFound(params.session_id.clone()))?;

        // Extract text from content blocks
        let instruction = params
            .content
            .iter()
            .filter_map(|b| match b {
                crate::protocol::ContentBlock::Text { text } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");

        if instruction.is_empty() {
            return Err(AcpError::InvalidParams(
                "No text content in prompt".to_string(),
            ));
        }

        // Execute prompt with Claude CLI
        let _result = self.cli.session_prompt(
            &instruction,
            &session.cwd,
            session.claude_session_id.as_deref(),
            params.mode.as_deref(),
        )?;

        Ok(SessionPromptResult {
            session_id: params.session_id,
            stop_reason: StopReason::EndTurn,
        })
    }

    async fn session_cancel(
        &self,
        params: SessionCancelParams,
    ) -> Result<SessionCancelResult, AcpError> {
        // Update session state
        self.session_manager
            .cancel_session(&params.session_id)
            .await?;

        // Execute cancel with Claude CLI
        if let Some(session) = self.get_session(&params.session_id).await {
            self.cli
                .session_cancel(session.claude_session_id.as_deref())
                .ok();
        }

        Ok(SessionCancelResult {
            session_id: params.session_id,
            stop_reason: StopReason::Cancelled,
        })
    }

    async fn session_load(&self, params: SessionLoadParams) -> Result<SessionNewResult, AcpError> {
        // Create session with specific ID
        let session = self
            .session_manager
            .create_session_with_id(&params.session_id, &PathBuf::from(&params.cwd))
            .await?;

        let mut claude_session = ClaudeSession {
            acp_session_id: session.id.clone(),
            claude_session_id: Some(params.session_id.clone()),
            cwd: PathBuf::from(&params.cwd),
            mode: None,
            created_at: Utc::now(),
        };

        // Execute session load with Claude CLI
        match self.cli.session_resume(&params.session_id, &params.cwd) {
            Ok(agent_session_id) => {
                let agent_session_id = agent_session_id.clone();
                claude_session.claude_session_id = Some(agent_session_id.clone());
                self.save_session(claude_session).await;

                let session_id = session.id.clone();
                Ok(SessionNewResult {
                    session_id,
                    agent_session_id: Some(agent_session_id),
                })
            }
            Err(e) => {
                self.session_manager.close_session(&session.id).await.ok();
                Err(e.into())
            }
        }
    }

    async fn session_resume(
        &self,
        params: SessionResumeParams,
    ) -> Result<SessionNewResult, AcpError> {
        // Similar to session_load but for resume
        self.session_load(SessionLoadParams {
            session_id: params.session_id,
            cwd: params.cwd,
            additional_directories: params.additional_directories,
            mcp_servers: params.mcp_servers,
        })
        .await
    }

    async fn session_close(
        &self,
        params: SessionCloseParams,
    ) -> Result<SessionCloseResult, AcpError> {
        // Remove session
        self.remove_session(&params.session_id).await;

        // Update session manager
        self.session_manager
            .close_session(&params.session_id)
            .await?;

        Ok(SessionCloseResult {
            session_id: params.session_id,
        })
    }

    async fn session_set_mode(
        &self,
        params: SessionSetModeParams,
    ) -> Result<SessionSetModeResult, AcpError> {
        // Update mode in session
        if let Some(mut session) = self.get_session(&params.session_id).await {
            session.mode = Some(params.mode.clone());
            self.save_session(session).await;
        }

        Ok(SessionSetModeResult {
            session_id: params.session_id,
            mode: params.mode,
        })
    }

    async fn session_list(&self) -> Result<SessionListResult, AcpError> {
        let sessions = self.sessions.read().await;
        let infos: Vec<SessionInfo> = sessions
            .iter()
            .map(|s| SessionInfo {
                session_id: s.acp_session_id.clone(),
                cwd: s.cwd.to_string_lossy().to_string(),
                created_at: s.created_at.to_rfc3339(),
                last_used_at: s.created_at.to_rfc3339(), // TODO: track last used
            })
            .collect();

        Ok(SessionListResult { sessions: infos })
    }

    async fn is_available(&self) -> bool {
        self.cli.is_available()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_agent_creation() {
        let agent = ClaudeCodeAgent::new();
        assert_eq!(agent.agent_type(), "claude_code");
        assert_eq!(agent.agent_name(), "Claude Code");
    }

    #[tokio::test]
    async fn test_capabilities() {
        let agent = ClaudeCodeAgent::new();
        let caps = agent.get_capabilities();
        assert!(caps.load_session);
    }
}
