#![allow(dead_code)]

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc::Sender;
use std::thread;

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;

use super::{Agent, AgentConfig, AgentInstance, AgentStatus};

#[derive(Debug, Clone)]
pub enum ClaudeStreamEvent {
    Started {
        command: String,
        workdir: String,
    },
    AssistantText(String),
    Progress {
        label: String,
        detail: String,
    },
    Stderr(String),
    Session {
        session_id: String,
    },
    AskUserQuestion {
        prompt: String,
        options: Vec<String>,
    },
    Finished {
        result: String,
    },
    Failed {
        error: String,
    },
}

pub struct ClaudeCodeAgent {
    binary_path: PathBuf,
}

impl ClaudeCodeAgent {
    pub fn new() -> Self {
        let binary_path = which::which("claude").unwrap_or_else(|_| PathBuf::from("claude"));
        Self { binary_path }
    }

    pub fn check_installation() -> Option<PathBuf> {
        which::which("claude").ok()
    }

    pub fn get_project_dir(task_id: usize) -> PathBuf {
        PathBuf::from(format!("/tmp/one_task_{}", task_id))
    }

    fn extract_detail(value: &serde_json::Value) -> String {
        let preferred_fields = [
            "message",
            "status",
            "summary",
            "subtype",
            "result",
            "session_id",
            "model",
        ];

        for field in preferred_fields {
            if let Some(field_value) = value.get(field) {
                if let Some(text) = field_value.as_str() {
                    if !text.trim().is_empty() {
                        return text.to_string();
                    }
                } else {
                    return serde_json::to_string(field_value)
                        .unwrap_or_else(|_| "<invalid event>".to_string());
                }
            }
        }

        serde_json::to_string(value).unwrap_or_else(|_| "<invalid event>".to_string())
    }

    fn parse_stream_line(line: &str) -> Vec<ClaudeStreamEvent> {
        let Ok(json) = serde_json::from_str::<serde_json::Value>(line) else {
            if line.trim().is_empty() {
                return vec![];
            }
            return vec![ClaudeStreamEvent::Progress {
                label: "stdout".to_string(),
                detail: line.to_string(),
            }];
        };

        let type_str = json
            .get("type")
            .and_then(|value| value.as_str())
            .unwrap_or("unknown");

        let mut prefix_events = Vec::new();
        if let Some(session_id) = json.get("session_id").and_then(|value| value.as_str()) {
            if !session_id.trim().is_empty() {
                prefix_events.push(ClaudeStreamEvent::Session {
                    session_id: session_id.to_string(),
                });
            }
        }

        let mut events = match type_str {
            "assistant" => {
                let mut events = Vec::new();
                let mut text_parts = Vec::new();

                if let Some(items) = json
                    .get("message")
                    .and_then(|message| message.get("content"))
                    .and_then(|content| content.as_array())
                {
                    for item in items {
                        match item.get("type").and_then(|value| value.as_str()) {
                            Some("text") => {
                                if let Some(text) =
                                    item.get("text").and_then(|value| value.as_str())
                                {
                                    if !text.trim().is_empty() {
                                        text_parts.push(text.to_string());
                                    }
                                }
                            }
                            Some(other) => events.push(ClaudeStreamEvent::Progress {
                                label: format!("assistant:{other}"),
                                detail: Self::extract_detail(item),
                            }),
                            None => {}
                        }
                    }
                }

                if !text_parts.is_empty() {
                    events.insert(0, ClaudeStreamEvent::AssistantText(text_parts.join("\n")));
                }

                events
            }
            "ask_user_question" | "question" | "input_required" => {
                let prompt = json
                    .get("question")
                    .and_then(|value| value.as_str())
                    .or_else(|| json.get("prompt").and_then(|value| value.as_str()))
                    .or_else(|| json.get("message").and_then(|value| value.as_str()))
                    .unwrap_or("Claude Code is waiting for your answer")
                    .to_string();
                let options = json
                    .get("options")
                    .and_then(|value| value.as_array())
                    .map(|values| {
                        values
                            .iter()
                            .filter_map(|value| value.as_str().map(|text| text.to_string()))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                vec![ClaudeStreamEvent::AskUserQuestion { prompt, options }]
            }
            "result" => {
                let detail = json
                    .get("result")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default()
                    .to_string();

                if detail.is_empty() {
                    vec![]
                } else {
                    vec![ClaudeStreamEvent::Progress {
                        label: "result".to_string(),
                        detail,
                    }]
                }
            }
            "system" => vec![ClaudeStreamEvent::Progress {
                label: "system".to_string(),
                detail: Self::extract_detail(&json),
            }],
            other => vec![ClaudeStreamEvent::Progress {
                label: other.to_string(),
                detail: Self::extract_detail(&json),
            }],
        };

        prefix_events.append(&mut events);
        prefix_events
    }

    pub fn execute_instruction_stream(
        project_dir: &PathBuf,
        instruction: &str,
        session_id: Option<&str>,
        sender: Sender<ClaudeStreamEvent>,
    ) -> Result<String> {
        std::fs::create_dir_all(project_dir).with_context(|| {
            format!("Failed to create Claude workdir: {}", project_dir.display())
        })?;

        let binary_path = Self::check_installation().unwrap_or_else(|| PathBuf::from("claude"));
        let mut cmd = Command::new(&binary_path);
        cmd.args(&[
            "-p",
            instruction,
            "--output-format",
            "stream-json",
            "--verbose",
            "--permission-mode",
            "bypassPermissions",
        ])
        .current_dir(project_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

        if let Some(sid) = session_id {
            cmd.arg("--session-id");
            cmd.arg(sid);
        }

        let command_preview = format!(
            "{} -p {:?} --output-format stream-json --verbose --permission-mode bypassPermissions{}",
            binary_path.display(),
            instruction,
            session_id
                .map(|sid| format!(" --session-id {}", sid))
                .unwrap_or_default()
        );
        let _ = sender.send(ClaudeStreamEvent::Started {
            command: command_preview,
            workdir: project_dir.to_string_lossy().to_string(),
        });

        let mut child = cmd.spawn().with_context(|| {
            format!(
                "Failed to spawn claude process: binary={}, cwd={}",
                binary_path.display(),
                project_dir.display()
            )
        })?;

        let stdout = child.stdout.take().context("Failed to capture stdout")?;
        let stderr = child.stderr.take().context("Failed to capture stderr")?;
        let reader = BufReader::new(stdout);
        let stderr_sender = sender.clone();
        let stderr_thread = thread::spawn(move || {
            let stderr_reader = BufReader::new(stderr);
            for line in stderr_reader.lines() {
                match line {
                    Ok(line) if !line.trim().is_empty() => {
                        let _ = stderr_sender.send(ClaudeStreamEvent::Stderr(line));
                    }
                    Ok(_) => {}
                    Err(error) => {
                        let _ = stderr_sender.send(ClaudeStreamEvent::Stderr(format!(
                            "stderr read error: {}",
                            error
                        )));
                        break;
                    }
                }
            }
        });

        let mut result_text = String::new();
        let mut result_override: Option<String> = None;

        for line in reader.lines() {
            let line = line.context("Failed to read line")?;
            for event in Self::parse_stream_line(&line) {
                match &event {
                    ClaudeStreamEvent::AssistantText(text) => {
                        if !result_text.is_empty() {
                            result_text.push('\n');
                        }
                        result_text.push_str(text);
                    }
                    ClaudeStreamEvent::Progress { label, detail } if label == "result" => {
                        if !detail.is_empty() {
                            result_override = Some(detail.clone());
                        }
                    }
                    _ => {}
                }
                let _ = sender.send(event);
            }
        }

        let status = child.wait().context("Failed to wait for process")?;
        let _ = stderr_thread.join();

        if !status.success() {
            return Err(anyhow!("claude exited with status {}", status));
        }

        let final_text = if result_text.trim().is_empty() {
            result_override.unwrap_or_default()
        } else {
            result_text.trim().to_string()
        };

        Ok(final_text)
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

    async fn spawn(&self, config: AgentConfig) -> Result<AgentInstance> {
        let task_id = config
            .session_id
            .as_ref()
            .and_then(|s| s.split('_').last()?.parse().ok())
            .unwrap_or(0);

        let project_dir = Self::get_project_dir(task_id);
        std::fs::create_dir_all(&project_dir).context("Failed to create project directory")?;

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

        let project_dir = instance
            .session_state
            .get("project_dir")
            .and_then(|v| v.as_str())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/tmp"));

        let session_id = instance
            .session_state
            .get("session_id")
            .and_then(|v| v.as_str());

        let (sender, _receiver) = std::sync::mpsc::channel();
        let result = Self::execute_instruction_stream(&project_dir, msg, session_id, sender)?;

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
