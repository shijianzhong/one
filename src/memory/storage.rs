#![allow(dead_code)]

use crate::memory::types::{MemorySnapshot, TaskMemory};
use std::fs;
use std::path::PathBuf;

pub fn get_memory_base_path() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("memory")
}

pub fn get_workspace_memory_dir(workspace_name: &str) -> PathBuf {
    get_memory_base_path().join(workspace_name)
}

pub fn get_task_memory_path(workspace_name: &str, task_id: usize) -> PathBuf {
    get_workspace_memory_dir(workspace_name).join(format!("task_{}.json", task_id))
}

pub fn get_task_snapshot_path(workspace_name: &str, task_id: usize) -> PathBuf {
    get_workspace_memory_dir(workspace_name).join(format!("task_{}_snapshot.yaml", task_id))
}

pub fn ensure_memory_dir(workspace_name: &str) -> anyhow::Result<PathBuf> {
    let dir = get_workspace_memory_dir(workspace_name);
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn load_task_memory(workspace_name: &str, task_id: usize) -> Option<TaskMemory> {
    let path = get_task_memory_path(workspace_name, task_id);
    if path.exists() {
        if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(memory) = serde_json::from_str(&content) {
                return Some(memory);
            }
        }
    }
    None
}

pub fn save_task_memory(workspace_name: &str, memory: &TaskMemory) -> anyhow::Result<()> {
    let dir = ensure_memory_dir(workspace_name)?;
    let path = dir.join(format!("task_{}.json", memory.task_id));
    let content = serde_json::to_string_pretty(memory)?;
    fs::write(&path, content)?;
    Ok(())
}

pub fn save_task_memory_async(
    workspace_name: String,
    task_id: usize,
    task_title: String,
    messages: Vec<crate::memory::types::ChatMessage>,
) {
    use crate::memory::search::upsert_task_chunks;
    std::thread::spawn(move || {
        let memory = TaskMemory {
            task_id,
            task_title,
            messages,
        };
        if let Err(e) = save_task_memory(&workspace_name, &memory) {
            eprintln!("[Memory L2] save messages failed: {}", e);
        }
        upsert_task_chunks(
            &workspace_name,
            task_id,
            &memory.task_title,
            &memory.messages,
        );
    });
}

pub fn load_task_snapshot(workspace_name: &str, task_id: usize) -> Option<MemorySnapshot> {
    let path = get_task_snapshot_path(workspace_name, task_id);
    if path.exists() {
        if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(snap) = serde_yaml::from_str(&content) {
                return Some(snap);
            }
        }
    }
    None
}

pub fn save_task_snapshot(workspace_name: &str, snapshot: &MemorySnapshot) -> anyhow::Result<()> {
    let dir = ensure_memory_dir(workspace_name)?;
    let path = dir.join(format!("task_{}_snapshot.yaml", snapshot.task_id));
    let content = serde_yaml::to_string(snapshot)?;
    fs::write(&path, content)?;
    Ok(())
}
