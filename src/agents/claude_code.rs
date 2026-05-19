#![allow(dead_code)]

use std::process::{Command, Stdio};
use std::io::{BufRead, BufReader};
use std::path::PathBuf;

use anyhow::{Context, Result};
use async_trait::async_trait;

use super::{Agent, AgentConfig, AgentInstance, AgentStatus};

pub struct ClaudeCodeAgent {
    binary_path: PathBuf,
}

impl ClaudeCodeAgent {
    pub fn new() -> Self {
        let binary_path = which::which("claude")
            .unwrap_or_else(|_| PathBuf::from("claude"));
        Self { binary_path }
    }

    pub fn check_installation() -> Option<PathBuf> {
        which::which("claude").ok()
    }

    pub fn get_project_dir(task_id: usize) -> PathBuf {
        PathBuf::from(format!("/tmp/one_task_{}", task_id))
    }

    fn parse_stream_line(line: &str) -> Option<ClaudeResponse> {
        let json: serde_json::Value = serde_json::from_str(line).ok()?;
        let type_str = json.get("type")?.as_str()?;

        match type_str {
            "assistant" => {
                let content = json.get("message")?.get("content")?;
                if let Some(arr) = content.as_array() {
                    for item in arr {
                        if item.get("type")?.as_str()? == "text" {
                            return Some(ClaudeResponse::Text(item.get("text")?.as_str()?.to_string()));
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
            _ => None,
        }
    }

    pub fn execute_instruction(
        project_dir: &PathBuf,
        instruction: &str,
        session_id: Option<&str>,
    ) -> Result<String> {
        let mut cmd = Command::new("claude");
        cmd.args(&[
            "-p",
            instruction,
            "--output-format", "stream-json",
            "--verbose",
            "--permission-mode", "bypassPermissions",
        ])
        .current_dir(project_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

        if let Some(sid) = session_id {
            cmd.arg("--session-id");
            cmd.arg(sid);
        }

        let mut child = cmd.spawn().context("Failed to spawn claude process")?;

        let stdout = child.stdout.take().context("Failed to capture stdout")?;
        let reader = BufReader::new(stdout);

        let mut result_text = String::new();

        for line in reader.lines() {
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
                    ClaudeResponse::System => continue,
                }
            }
        }

        child.wait().context("Failed to wait for process")?;

        Ok(result_text.trim().to_string())
    }
}

enum ClaudeResponse {
    Text(String),
    Result(String),
    System,
}

#[async_trait]
impl Agent for ClaudeCodeAgent {
    fn agent_type(&self) -> &str {
        "claude_code"
    }

    fn agent_name(&self) -> &str {
        "Claude Code"
    }

    async fn spawn(&self, config: AgentConfig) -> Result<AgentInstance> {
        let task_id = config.session_id
            .as_ref()
            .and_then(|s| s.split('_').last()?.parse().ok())
            .unwrap_or(0);

        let project_dir = Self::get_project_dir(task_id);
        std::fs::create_dir_all(&project_dir)
            .context("Failed to create project directory")?;

        let instance = AgentInstance {
            id: 0,
            agent_id: 0,
            task_id: Some(task_id),
            status: AgentStatus::Idle,
            session_state: serde_json::json!({
                "project_dir": project_dir.to_string_lossy(),
                "session_id": config.session_id,
            }),
        };

        Ok(instance)
    }

    async fn send_message(&self, instance: &mut AgentInstance, msg: &str) -> Result<String> {
        instance.status = AgentStatus::Running;

        let project_dir = instance.session_state.get("project_dir")
            .and_then(|v| v.as_str())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/tmp"));

        let session_id = instance.session_state.get("session_id")
            .and_then(|v| v.as_str());

        let result = Self::execute_instruction(&project_dir, msg, session_id)?;

        instance.status = AgentStatus::Idle;

        Ok(result)
    }

    async fn get_status(&self, instance: &AgentInstance) -> AgentStatus {
        instance.status
    }

    async fn pause(&self, instance: &mut AgentInstance) -> Result<()> {
        instance.status = AgentStatus::Paused;
        Ok(())
    }

    async fn resume(&self, instance: &mut AgentInstance) -> Result<()> {
        instance.status = AgentStatus::Running;
        Ok(())
    }

    async fn destroy(&self, instance: &mut AgentInstance) -> Result<()> {
        instance.status = AgentStatus::Terminated;
        Ok(())
    }
}

impl Default for ClaudeCodeAgent {
    fn default() -> Self {
        Self::new()
    }
}
