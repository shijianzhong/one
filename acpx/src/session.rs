//! ACP Session Management
//!
//! Session state machine and session manager implementation.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::error::AcpError;
use crate::protocol::SessionInfo;

/// Session state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Active,
    Idle,
    Cancelled,
    Closed,
}

impl std::fmt::Display for SessionState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionState::Active => write!(f, "active"),
            SessionState::Idle => write!(f, "idle"),
            SessionState::Cancelled => write!(f, "cancelled"),
            SessionState::Closed => write!(f, "closed"),
        }
    }
}

impl Default for SessionState {
    fn default() -> Self {
        SessionState::Idle
    }
}

/// Session representation
#[derive(Debug, Clone)]
pub struct Session {
    /// ACP session ID
    pub id: String,

    /// Agent's internal session ID
    pub agent_session_id: Option<String>,

    /// Working directory
    pub cwd: PathBuf,

    /// Current state
    pub state: SessionState,

    /// When session was created
    pub created_at: DateTime<Utc>,

    /// When session was last used
    pub last_used_at: DateTime<Utc>,

    /// Additional directories
    pub additional_directories: Vec<PathBuf>,
}

impl Session {
    /// Create a new session
    pub fn new(cwd: PathBuf) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            agent_session_id: None,
            cwd,
            state: SessionState::Idle,
            created_at: now,
            last_used_at: now,
            additional_directories: Vec::new(),
        }
    }

    /// Create a session with a specific ID (for resuming)
    pub fn with_id(id: &str, cwd: PathBuf) -> Self {
        let now = Utc::now();
        Self {
            id: id.to_string(),
            agent_session_id: None,
            cwd,
            state: SessionState::Idle,
            created_at: now,
            last_used_at: now,
            additional_directories: Vec::new(),
        }
    }

    /// Set the agent session ID
    pub fn set_agent_session_id(&mut self, agent_session_id: String) {
        self.agent_session_id = Some(agent_session_id);
    }

    /// Transition to active state
    pub fn activate(&mut self) {
        self.state = SessionState::Active;
        self.last_used_at = Utc::now();
    }

    /// Transition to idle state
    pub fn idle(&mut self) {
        self.state = SessionState::Idle;
        self.last_used_at = Utc::now();
    }

    /// Cancel the session
    pub fn cancel(&mut self) {
        self.state = SessionState::Cancelled;
        self.last_used_at = Utc::now();
    }

    /// Close the session
    pub fn close(&mut self) {
        self.state = SessionState::Closed;
        self.last_used_at = Utc::now();
    }

    /// Check if session is in a terminal state
    pub fn is_terminal(&self) -> bool {
        matches!(
            self.state,
            SessionState::Cancelled | SessionState::Closed
        )
    }

    /// Check if session can accept prompts
    pub fn can_prompt(&self) -> bool {
        matches!(self.state, SessionState::Idle | SessionState::Active)
    }

    /// Convert to SessionInfo for listing
    pub fn to_session_info(&self) -> SessionInfo {
        SessionInfo {
            session_id: self.id.clone(),
            cwd: self.cwd.to_string_lossy().to_string(),
            created_at: self.created_at.to_rfc3339(),
            last_used_at: self.last_used_at.to_rfc3339(),
        }
    }
}

/// Session manager - manages all active sessions
#[derive(Debug, Clone)]
pub struct SessionManager {
    sessions: Arc<RwLock<HashMap<String, Arc<Session>>>>,
    cwd_index: Arc<RwLock<HashMap<String, String>>>,
}

impl SessionManager {
    /// Create a new session manager
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            cwd_index: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a new session
    pub async fn create_session(&self, cwd: &PathBuf) -> Result<Arc<Session>, AcpError> {
        let session = Arc::new(Session::new(cwd.clone()));

        let mut sessions = self.sessions.write().await;
        sessions.insert(session.id.clone(), session.clone());

        let mut cwd_index = self.cwd_index.write().await;
        cwd_index.insert(cwd.to_string_lossy().to_string(), session.id.clone());

        Ok(session)
    }

    /// Create a session with a specific ID (for loading/resuming)
    pub async fn create_session_with_id(
        &self,
        id: &str,
        cwd: &PathBuf,
    ) -> Result<Arc<Session>, AcpError> {
        // Check if session already exists
        {
            let sessions = self.sessions.read().await;
            if sessions.contains_key(id) {
                return Err(AcpError::SessionAlreadyExists(id.to_string()));
            }
        }

        let session = Arc::new(Session::with_id(id, cwd.clone()));

        let mut sessions = self.sessions.write().await;
        sessions.insert(session.id.clone(), session.clone());

        let mut cwd_index = self.cwd_index.write().await;
        cwd_index.insert(cwd.to_string_lossy().to_string(), session.id.clone());

        Ok(session)
    }

    /// Get a session by ID
    pub async fn get_session(&self, id: &str) -> Option<Arc<Session>> {
        let sessions = self.sessions.read().await;
        sessions.get(id).cloned()
    }

    /// Find a session by working directory
    pub async fn find_by_cwd(&self, cwd: &PathBuf) -> Option<Arc<Session>> {
        let cwd_index = self.cwd_index.read().await;
        let session_id = cwd_index.get(&cwd.to_string_lossy().to_string())?;
        let sessions = self.sessions.read().await;
        sessions.get(session_id).cloned()
    }

    /// List all active sessions (not closed)
    pub async fn list_sessions(&self) -> Vec<SessionInfo> {
        let sessions = self.sessions.read().await;
        sessions
            .values()
            .filter(|s| !s.is_terminal())
            .map(|s| s.to_session_info())
            .collect()
    }

    /// Update session state
    pub async fn update_state(&self, id: &str, state: SessionState) -> Result<(), AcpError> {
        let session = self.get_session(id).await.ok_or_else(|| AcpError::SessionNotFound(id.to_string()))?;

        let mut session = (*session).clone();
        match state {
            SessionState::Active => session.activate(),
            SessionState::Idle => session.idle(),
            SessionState::Cancelled => session.cancel(),
            SessionState::Closed => session.close(),
        }

        let mut sessions = self.sessions.write().await;
        if let Some(s) = sessions.get_mut(id) {
            *s = Arc::new(session);
        }

        Ok(())
    }

    /// Set agent session ID for a session
    pub async fn set_agent_session_id(
        &self,
        id: &str,
        agent_session_id: &str,
    ) -> Result<(), AcpError> {
        let session = self.get_session(id).await.ok_or_else(|| AcpError::SessionNotFound(id.to_string()))?;

        let mut session = (*session).clone();
        session.set_agent_session_id(agent_session_id.to_string());

        let mut sessions = self.sessions.write().await;
        if let Some(s) = sessions.get_mut(id) {
            *s = Arc::new(session);
        }

        Ok(())
    }

    /// Close a session
    pub async fn close_session(&self, id: &str) -> Result<(), AcpError> {
        self.update_state(id, SessionState::Closed).await
    }

    /// Cancel a session
    pub async fn cancel_session(&self, id: &str) -> Result<(), AcpError> {
        self.update_state(id, SessionState::Cancelled).await
    }

    /// Remove a session (cleanup)
    pub async fn remove_session(&self, id: &str) -> Option<Arc<Session>> {
        let session = {
            let mut sessions = self.sessions.write().await;
            sessions.remove(id)
        };

        if let Some(ref s) = session {
            let mut cwd_index = self.cwd_index.write().await;
            cwd_index.remove(&s.cwd.to_string_lossy().to_string());
        }

        session
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_session() {
        let manager = SessionManager::new();
        let cwd = PathBuf::from("/tmp/test");

        let session = manager.create_session(&cwd).await.unwrap();
        assert_eq!(session.cwd, cwd);
        assert_eq!(session.state, SessionState::Idle);
        assert!(session.agent_session_id.is_none());
    }

    #[tokio::test]
    async fn test_get_session() {
        let manager = SessionManager::new();
        let cwd = PathBuf::from("/tmp/test");

        let created = manager.create_session(&cwd).await.unwrap();
        let retrieved = manager.get_session(&created.id).await.unwrap();

        assert_eq!(created.id, retrieved.id);
    }

    #[tokio::test]
    async fn test_session_state_transitions() {
        let manager = SessionManager::new();
        let cwd = PathBuf::from("/tmp/test");

        let session_id = {
            let session = manager.create_session(&cwd).await.unwrap();
            session.id.clone()
        };

        manager.update_state(&session_id, SessionState::Active).await.unwrap();
        {
            let session = manager.get_session(&session_id).await.unwrap();
            assert_eq!(session.state, SessionState::Active);
        }

        manager.update_state(&session_id, SessionState::Idle).await.unwrap();
        {
            let session = manager.get_session(&session_id).await.unwrap();
            assert_eq!(session.state, SessionState::Idle);
        }
    }

    #[tokio::test]
    async fn test_close_session() {
        let manager = SessionManager::new();
        let cwd = PathBuf::from("/tmp/test");

        let session_id = {
            let session = manager.create_session(&cwd).await.unwrap();
            session.id.clone()
        };

        manager.close_session(&session_id).await.unwrap();
        let session = manager.get_session(&session_id).await.unwrap();
        assert_eq!(session.state, SessionState::Closed);
        assert!(session.is_terminal());
    }
}
