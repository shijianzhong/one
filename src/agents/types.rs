#![allow(dead_code)]

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capability {
    pub name: String,
    pub description: String,
    pub trigger_keywords: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub name: String,
    pub tools: Vec<String>,
    pub max_iterations: usize,
    pub timeout_seconds: u64,
    pub memory_enabled: bool,
    pub session_id: Option<String>,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            name: "default".to_string(),
            tools: vec![],
            max_iterations: 100,
            timeout_seconds: 300,
            memory_enabled: true,
            session_id: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentStatus {
    Idle,
    Running,
    Paused,
    Terminated,
}

impl std::fmt::Display for AgentStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentStatus::Idle => write!(f, "idle"),
            AgentStatus::Running => write!(f, "running"),
            AgentStatus::Paused => write!(f, "paused"),
            AgentStatus::Terminated => write!(f, "terminated"),
        }
    }
}

impl From<&str> for AgentStatus {
    fn from(s: &str) -> Self {
        match s {
            "idle" => AgentStatus::Idle,
            "running" => AgentStatus::Running,
            "paused" => AgentStatus::Paused,
            "terminated" => AgentStatus::Terminated,
            _ => AgentStatus::Idle,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInstance {
    pub id: usize,
    pub agent_id: usize,
    pub task_id: Option<usize>,
    pub status: AgentStatus,
    pub session_state: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BusinessCapability {
    pub name: String,
    pub description: String,
    pub trigger_queries: Vec<String>,
    pub response_template: String,
    pub follow_up_questions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BusinessAgentConfig {
    pub name: String,
    pub description: String,
    pub capabilities: Vec<BusinessCapability>,
    pub tools: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum RoutingDecision {
    ClaudeCode {
        instruction: String,
        session_id: Option<String>,
    },
    BusinessAgent {
        agent_id: usize,
        message: String,
    },
    SystemTools {
        task: String,
    },
    GeneralAI {
        messages: Vec<crate::memory::types::ChatMessage>,
    },
    MultiAgent {
        agents: Vec<(String, String)>,
    },
}
