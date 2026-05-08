use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
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
