//! Claude CLI Process Management
//!
//! Low-level child process management for Claude CLI.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::sync::RwLock;
use tokio::time::timeout;

use crate::error::AcpError;

/// Process handle for a Claude CLI invocation
pub struct ProcessHandle {
    child: Child,
    session_id: Option<String>,
}

impl ProcessHandle {
    /// Wait for process to complete
    pub fn wait(&mut self) -> Result<Option<i32>> {
        let status = self.child.wait().context("Failed to wait for child")?;
        Ok(status.code())
    }

    /// Try to kill the process
    pub fn kill(&mut self) -> Result<()> {
        self.child.kill().context("Failed to kill child")?;
        Ok(())
    }

    /// Get session ID
    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }
}

/// Process manager for Claude CLI
pub struct ProcessManager {
    processes: Arc<RwLock<HashMap<String, Arc<tokio::sync::Mutex<ProcessHandle>>>>>,
}

impl ProcessManager {
    /// Create a new process manager
    pub fn new() -> Self {
        Self {
            processes: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Spawn a Claude CLI process
    pub async fn spawn(
        &self,
        session_id: &str,
        cli_path: &PathBuf,
        args: &[&str],
        cwd: &PathBuf,
    ) -> Result<Arc<tokio::sync::Mutex<ProcessHandle>>> {
        let mut cmd = Command::new(cli_path);
        cmd.args(args)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd.spawn().context("Failed to spawn claude process")?;

        let _stdin = child.stdin.take().context("Failed to take stdin")?;
        let _stdout = child.stdout.take().context("Failed to take stdout")?;
        let _stderr = child.stderr.take().context("Failed to take stderr")?;

        let handle = Arc::new(tokio::sync::Mutex::new(ProcessHandle {
            child,
            session_id: Some(session_id.to_string()),
        }));

        self.processes
            .write()
            .await
            .insert(session_id.to_string(), handle.clone());

        Ok(handle)
    }

    /// Get a process by session ID
    pub async fn get(&self, session_id: &str) -> Option<Arc<tokio::sync::Mutex<ProcessHandle>>> {
        self.processes.read().await.get(session_id).cloned()
    }

    /// Remove a process (cleanup)
    pub async fn remove(&self, session_id: &str) -> Option<Arc<tokio::sync::Mutex<ProcessHandle>>> {
        self.processes.write().await.remove(session_id)
    }

    /// Kill a process
    pub async fn kill(&self, session_id: &str) -> Result<()> {
        let processes = self.processes.read().await;
        if let Some(handle) = processes.get(session_id) {
            let mut h = handle.lock().await;
            h.kill()?;
        }
        Ok(())
    }

    /// List all managed process session IDs
    pub async fn list(&self) -> Vec<String> {
        self.processes.read().await.keys().cloned().collect()
    }
}

impl Default for ProcessManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Builder for Claude CLI command arguments
pub struct CommandBuilder {
    instruction: String,
    cwd: PathBuf,
    session_id: Option<String>,
    mode: Option<String>,
    timeout_seconds: Option<u64>,
    mcp_servers: Vec<String>,
}

impl CommandBuilder {
    /// Create a new builder
    pub fn new(instruction: &str, cwd: PathBuf) -> Self {
        Self {
            instruction: instruction.to_string(),
            cwd,
            session_id: None,
            mode: None,
            timeout_seconds: None,
            mcp_servers: Vec::new(),
        }
    }

    /// Set session ID for resume
    pub fn with_session(mut self, session_id: &str) -> Self {
        self.session_id = Some(session_id.to_string());
        self
    }

    /// Set permission mode
    pub fn with_mode(mut self, mode: &str) -> Self {
        self.mode = Some(mode.to_string());
        self
    }

    /// Set timeout
    pub fn with_timeout(mut self, seconds: u64) -> Self {
        self.timeout_seconds = Some(seconds);
        self
    }

    /// Add MCP server
    pub fn with_mcp_server(mut self, server: &str) -> Self {
        self.mcp_servers.push(server.to_string());
        self
    }

    /// Build command arguments
    pub fn build_args(&self) -> Vec<String> {
        let mut args = vec![
            "-p".to_string(),
            self.instruction.clone(),
            "--output-format".to_string(),
            "stream-json".to_string(),
            "--verbose".to_string(),
            "--permission-mode".to_string(),
            self.mode.clone().unwrap_or_else(|| "auto".to_string()),
        ];

        if let Some(ref session_id) = self.session_id {
            args.push("--resume".to_string());
            args.push(session_id.clone());
        }

        for server in &self.mcp_servers {
            args.push("--mcp-server".to_string());
            args.push(server.clone());
        }

        args
    }

    /// Get working directory
    pub fn cwd(&self) -> &PathBuf {
        &self.cwd
    }

    /// Get timeout
    pub fn timeout(&self) -> Option<Duration> {
        self.timeout_seconds.map(Duration::from_secs)
    }
}

/// Execute a command with timeout
pub async fn execute_with_timeout<F, Fut>(timeout_duration: Duration, future: F) -> Result<String>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<String>>,
{
    let result = timeout(timeout_duration, future()).await;

    match result {
        Ok(Ok(output)) => Ok(output),
        Ok(Err(e)) => Err(e),
        Err(_) => Err(AcpError::ExecutionTimeout {
            session_id: "unknown".to_string(),
            timeout_ms: timeout_duration.as_millis() as u64,
        }
        .into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_builder() {
        let builder = CommandBuilder::new("test instruction", PathBuf::from("/tmp"))
            .with_session("session-123")
            .with_mode("plan")
            .with_mcp_server("mcp-server-1");

        let args = builder.build_args();
        assert!(args.contains(&"--resume".to_string()));
        assert!(args.contains(&"session-123".to_string()));
        assert!(args.contains(&"plan".to_string()));
        assert!(args.contains(&"--mcp-server".to_string()));
    }

    #[tokio::test]
    async fn test_process_manager_spawn() {
        let manager = ProcessManager::new();
        let cli_path = which::which("claude").unwrap_or_else(|_| PathBuf::from("claude"));

        // This test requires claude to be installed
        if !cli_path.exists() {
            return;
        }

        // Note: This would actually spawn a process, so we skip in unit tests
        // In integration tests, we'd verify actual spawning
    }
}
