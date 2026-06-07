#![allow(dead_code)]

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl ChatMessage {
    pub fn new(role: &str, content: &str) -> Self {
        Self {
            role: role.to_string(),
            content: content.to_string(),
            tool_calls: None,
            tool_call_id: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactEntry {
    pub content: String,
    pub timestamp: i64,
    pub source_task_id: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskMemory {
    pub task_id: usize,
    pub task_title: String,
    pub messages: Vec<ChatMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MemorySnapshot {
    pub task_id: usize,
    pub task_title: String,
    pub summary: String,
    pub key_facts: Vec<String>,
    pub open_loops: Vec<String>,
    pub preferences: Vec<String>,
    pub last_updated: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryChunk {
    pub chunk_id: String,
    pub workspace: String,
    pub task_id: usize,
    pub task_title: String,
    pub role: String,
    pub content: String,
    pub turn_index: usize,
}
