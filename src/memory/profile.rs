//! User profile storage — independent of task snapshots.
//!
//! Stores long-term user facts (name, preferences, habits, important context)
//! that should persist across sessions. Each workspace has its own profile.
//! Data is stored as a simple JSON file: `memory/<workspace>/profile.json`.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use crate::memory::storage::get_workspace_memory_dir;

/// A user profile for a workspace, holding long-term facts.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UserProfile {
    /// List of known facts about the user
    pub key_facts: Vec<String>,
    /// When the profile was last updated (unix timestamp)
    pub last_updated: i64,
}

/// Path to the profile JSON file for a workspace.
fn get_profile_path(workspace_name: &str) -> PathBuf {
    get_workspace_memory_dir(workspace_name).join("profile.json")
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

/// Save a user fact to the profile. Deduplicates by exact string match.
pub fn save_fact(workspace_name: &str, fact: &str) -> anyhow::Result<()> {
    let dir = get_workspace_memory_dir(workspace_name);
    fs::create_dir_all(&dir)?;

    let mut profile = load_profile(workspace_name);

    // Avoid inserting duplicate facts
    if !profile.key_facts.iter().any(|f| f == fact) {
        profile.key_facts.push(fact.to_string());
    }
    profile.last_updated = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    let path = get_profile_path(workspace_name);
    let content = serde_json::to_string_pretty(&profile)?;
    fs::write(&path, content)?;
    Ok(())
}

/// Get all stored facts for a workspace.
pub fn get_all_facts(workspace_name: &str) -> Vec<String> {
    load_profile(workspace_name).key_facts
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

        save_fact(workspace, "User likes dark mode").unwrap();
        save_fact(workspace, "User prefers Rust").unwrap();
        save_fact(workspace, "User likes dark mode").unwrap(); // duplicate

        let facts = get_all_facts(workspace);
        assert_eq!(facts.len(), 2);
        assert!(facts.contains(&"User likes dark mode".to_string()));
        assert!(facts.contains(&"User prefers Rust".to_string()));

        // Clean up
        let _ = fs::remove_file(&path);
    }
}