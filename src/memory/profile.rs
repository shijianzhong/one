//! User profile storage — independent of task snapshots.
//!
//! Stores long-term user facts (name, preferences, habits, important context)
//! that should persist across sessions. Each workspace has its own profile.
//! Data is stored as a simple JSON file: `memory/<workspace>/profile.json`.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use crate::memory::storage::{get_global_memory_dir, get_workspace_memory_dir};
use crate::memory::types::FactEntry;

/// A user profile for a workspace, holding long-term facts.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UserProfile {
    /// List of known facts about the user
    pub key_facts: Vec<FactEntry>,
    /// When the profile was last updated (unix timestamp)
    pub last_updated: i64,
}

/// Path to the profile JSON file for a workspace.
fn get_profile_path(workspace_name: &str) -> PathBuf {
    if workspace_name == "global" {
        get_global_memory_dir().join("profile.json")
    } else {
        get_workspace_memory_dir(workspace_name).join("profile.json")
    }
}

/// Load the user profile for a workspace. Returns an empty profile if none exists.
pub fn load_profile(workspace_name: &str) -> UserProfile {
    let path = get_profile_path(workspace_name);
    if path.exists() {
        if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(profile) = serde_json::from_str(&content) {
                return profile;
            }
        }
    }
    UserProfile::default()
}

/// Save a user fact to the profile. 
/// Implements semantic de-duplication: 
/// - If new fact is a sub-string of an existing fact, skip.
/// - If new fact contains an existing fact, replace the old one.
pub fn save_fact(workspace_name: &str, fact: &str, task_id: Option<usize>) -> anyhow::Result<()> {
    let dir = if workspace_name == "global" {
        get_global_memory_dir()
    } else {
        get_workspace_memory_dir(workspace_name)
    };
    fs::create_dir_all(&dir)?;

    let mut profile = load_profile(workspace_name);

    // Semantic de-duplication
    // 1. If an existing fact contains the new fact, do nothing.
    if profile.key_facts.iter().any(|f| f.content.contains(fact)) {
        return Ok(());
    }
    
    // 2. Remove existing facts that are contained within the new fact.
    profile.key_facts.retain(|f| !fact.contains(&f.content));

    // 3. Add the new fact
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
        
    profile.key_facts.push(FactEntry {
        content: fact.to_string(),
        timestamp: now,
        source_task_id: task_id,
    });
    profile.last_updated = now;

    let path = get_profile_path(workspace_name);
    let content = serde_json::to_string_pretty(&profile)?;
    
    // Atomic write
    let tmp_path = path.with_extension("json.tmp");
    fs::write(&tmp_path, content)?;
    fs::rename(&tmp_path, &path)?;
    
    Ok(())
}

/// Get all stored facts for a workspace as raw strings.
pub fn get_all_facts(workspace_name: &str) -> Vec<String> {
    load_profile(workspace_name)
        .key_facts
        .into_iter()
        .map(|f| f.content)
        .collect()
}

/// Save fact to global memory.
pub fn save_global_fact(fact: &str, task_id: Option<usize>) -> anyhow::Result<()> {
    save_fact("global", fact, task_id)
}

/// Get all global facts.
pub fn get_global_facts() -> Vec<String> {
    get_all_facts("global")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_save_and_load_facts() {
        let workspace = "_test_profile";
        // Clean slate
        let path = get_profile_path(workspace);
        let _ = fs::remove_file(&path);

        assert!(get_all_facts(workspace).is_empty());

        save_fact(workspace, "User likes dark mode", None).unwrap();
        save_fact(workspace, "User prefers Rust", None).unwrap();
        save_fact(workspace, "User likes dark mode", None).unwrap(); // exact duplicate, should be skipped by .contains

        let facts = get_all_facts(workspace);
        assert_eq!(facts.len(), 2);
        assert!(facts.contains(&"User likes dark mode".to_string()));
        assert!(facts.contains(&"User prefers Rust".to_string()));

        // Test semantic replacement
        save_fact(workspace, "User prefers Rust and GPUI", None).unwrap();
        let facts = get_all_facts(workspace);
        assert_eq!(facts.len(), 2);
        assert!(facts.contains(&"User prefers Rust and GPUI".to_string()));
        assert!(!facts.contains(&"User prefers Rust".to_string()));

        // Test semantic skip
        save_fact(workspace, "User likes dark", None).unwrap();
        let facts = get_all_facts(workspace);
        assert_eq!(facts.len(), 2);
        assert!(facts.contains(&"User likes dark mode".to_string()));

        // Clean up
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_global_facts() {
        let workspace = "global";
        let path = get_profile_path(workspace);
        let _ = fs::remove_file(&path);

        save_global_fact("Global setting 1", None).unwrap();
        let facts = get_global_facts();
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0], "Global setting 1");

        let _ = fs::remove_file(&path);
    }
}