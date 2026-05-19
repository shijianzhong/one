//! Claude CLI Wrapper
//!
//! Wrapper for invoking Claude Code CLI commands.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::io::{BufRead, BufReader};

use anyhow::{Context, Result};
use which::which;

use crate::error::AcpError;
use crate::protocol::McpServer;

/// Claude CLI wrapper
#[derive(Debug, Clone)]
pub struct ClaudeCli {
    path: PathBuf,
}

impl ClaudeCli {
    /// Create a new CLI wrapper, finding Claude in PATH
    pub fn new() -> Self {
        let path = which("claude").unwrap_or_else(|_| PathBuf::from("claude"));
        Self { path }
    }

    /// Create with a specific path
    pub fn with_path(path: PathBuf) -> Self {
        Self { path }
    }

    /// Check if Claude CLI is available
    pub fn is_available(&self) -> bool {
        which("claude").is_ok()
    }

    /// Get Claude CLI path
    pub fn get_path() -> Option<PathBuf> {
        which("claude").ok()
    }

    /// Execute session/new equivalent
    pub async fn session_new(&self, cwd: &str, _mcp_servers: &[McpServer]) -> Result<(String, String)> {
        // For session/new, we just spawn a quick test to get session info
        // Claude CLI doesn't have an explicit "create session" - sessions are created on first use
        let output = Command::new(&self.path)
            .args(&["--print", "session info"])
            .current_dir(cwd)
            .output()
            .context("Failed to get session info")?;

        if !output.status.success() {
            return Err(anyhow::anyhow!("Claude CLI failed").into());
        }

        // Parse session ID from output
        let stdout = String::from_utf8_lossy(&output.stdout);
        let session_id = stdout.trim().to_string();

        Ok((session_id.clone(), session_id))
    }

    /// Execute session/prompt equivalent
    pub fn session_prompt(
        &self,
        instruction: &str,
        cwd: &PathBuf,
        session_id: Option<&str>,
        mode: Option<&str>,
    ) -> Result<String> {
        let mut cmd = Command::new(&self.path);
        cmd.arg("-p")
            .arg(instruction)
            .arg("--output-format")
            .arg("stream-json")
            .arg("--verbose")
            .arg("--permission-mode")
            .arg(mode.unwrap_or("auto"))
            .current_dir(cwd)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if let Some(sid) = session_id {
            cmd.arg("--resume");
            cmd.arg(sid);
        }

        let mut child = cmd.spawn().context("Failed to spawn claude process")?;

        let stdout = child.stdout.take().context("Failed to capture stdout")?;
        let stderr = child.stderr.take().context("Failed to capture stderr")?;
        let mut reader = BufReader::new(stdout).lines();
        let mut stderr_reader = BufReader::new(stderr).lines();

        let mut result_text = String::new();
        let mut _current_thinking = String::new();
        let mut _in_thinking = false;

        // Stream response - same pattern as existing claude_code.rs
        while let Some(line) = reader.next() {
            let line = line.context("Failed to read line")?;

            if let Some(response) = Self::parse_stream_line(&line) {
                match response {
                    ClaudeResponse::Text(text) => {
                        result_text.push_str(&text);
                        result_text.push('\n');
                    }
                    ClaudeResponse::Result(result) => {
                        if !result.is_empty() && result_text.is_empty() {
                            result_text = result;
                        }
                    }
                    ClaudeResponse::Thinking(text) => {
                        _current_thinking.push_str(&text);
                        _in_thinking = true;
                    }
                    ClaudeResponse::ThinkingEnd => {
                        _in_thinking = false;
                        _current_thinking.clear();
                    }
                    ClaudeResponse::System => continue,
                    ClaudeResponse::Error(err) => {
                        return Err(anyhow::anyhow!("Claude CLI error: {}", err).into());
                    }
                }
            }
        }

        // Also read stderr for any error messages
        while let Some(Ok(line)) = stderr_reader.next() {
            log::debug!("claude stderr: {}", line);
        }

        let status = child.wait().context("Failed to wait for process")?;

        if !status.success() {
            return Err(AcpError::InternalError(format!(
                "Claude CLI exited with status: {:?}",
                status
            )).into());
        }

        Ok(result_text.trim().to_string())
    }

    /// Execute session/cancel equivalent
    pub fn session_cancel(&self, _session_id: Option<&str>) -> Result<()> {
        // Claude CLI doesn't have a direct cancel command
        // The parent process would need to SIGTERM the child
        // For now, this is a no-op as cancel is handled at process level
        Ok(())
    }

    /// Execute session/resume equivalent
    pub fn session_resume(&self, session_id: &str, cwd: &str) -> Result<String> {
        let output = Command::new(&self.path)
            .args(&["--print", "session info"])
            .current_dir(cwd)
            .arg("--resume")
            .arg(session_id)
            .output()
            .context("Failed to resume session")?;

        if !output.status.success() {
            return Err(anyhow::anyhow!("Failed to resume session").into());
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(stdout.trim().to_string())
    }

    /// Parse a streaming JSON line from Claude output
    fn parse_stream_line(line: &str) -> Option<ClaudeResponse> {
        let json: serde_json::Value = serde_json::from_str(line).ok()?;
        let type_str = json.get("type")?.as_str()?;

        match type_str {
            "assistant" | "human" | "user" => {
                let message = json.get("message")?;
                let content = message.get("content")?;
                if let Some(arr) = content.as_array() {
                    for item in arr {
                        match item.get("type")?.as_str()? {
                            "text" => {
                                return Some(ClaudeResponse::Text(item.get("text")?.as_str()?.to_string()));
                            }
                            "thinking" => {
                                if let Some(thinking) = item.get("thinking")?.as_str() {
                                    return Some(ClaudeResponse::Thinking(thinking.to_string()));
                                }
                            }
                            "thinkingBlock" | "thinking_end" => {
                                return Some(ClaudeResponse::ThinkingEnd);
                            }
                            _ => {}
                        }
                    }
                }
                None
            }
            "result" => {
                let result = json.get("result")?.as_str()?.to_string();
                Some(ClaudeResponse::Result(result))
            }
            "system" => {
                Some(ClaudeResponse::System)
            }
            "error" => {
                let error = json.get("error").and_then(|e| e.as_str()).unwrap_or("Unknown error");
                Some(ClaudeResponse::Error(error.to_string()))
            }
            "progress" | "info" => {
                None // Skip progress/info messages
            }
            _ => None,
        }
    }
}

/// Parsed response types from Claude streaming output
#[derive(Debug, Clone)]
enum ClaudeResponse {
    Text(String),
    Result(String),
    Thinking(String),
    ThinkingEnd,
    System,
    Error(String),
}

impl Default for ClaudeCli {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_stream_line_result() {
        let line = r#"{"type":"result","result":"Hello, world!"}"#;
        let response = ClaudeCli::parse_stream_line(line);
        assert!(matches!(response, Some(ClaudeResponse::Result(_))));
    }

    #[test]
    fn test_parse_stream_line_system() {
        let line = r#"{"type":"system","message":"Starting..."}"#;
        let response = ClaudeCli::parse_stream_line(line);
        assert!(matches!(response, Some(ClaudeResponse::System)));
    }

    #[test]
    fn test_parse_stream_line_text() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Hello"}]}}"#;
        let response = ClaudeCli::parse_stream_line(line);
        assert!(matches!(response, Some(ClaudeResponse::Text(_))));
    }
}
