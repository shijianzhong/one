#![allow(dead_code)]

use std::sync::OnceLock;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::agents::permission::DangerLevel;

pub mod app_uninstaller;
pub mod desktop_organizer;
pub mod doc_summarizer;
pub mod media_dedup;
pub mod system_cleaner;
pub mod system_tools;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SkillCategory {
    System,
    Desktop,
    App,
    Doc,
    Media,
}

impl SkillCategory {
    pub fn label(&self) -> &'static str {
        match self {
            Self::System => "系统",
            Self::Desktop => "桌面",
            Self::App => "应用",
            Self::Doc => "文档",
            Self::Media => "媒体",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillManifest {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: SkillCategory,
    #[serde(default)]
    pub danger_level: DangerLevel,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SkillPreview {
    pub summary: String,
    pub items: Vec<SkillPreviewItem>,
    pub estimated_bytes: u64,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillPreviewItem {
    pub label: String,
    pub detail: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SkillExecution {
    pub summary: String,
    pub freed_bytes: u64,
    pub success_items: Vec<String>,
    pub failed_items: Vec<(String, String)>,
    pub denied: bool,
}

#[async_trait]
pub trait Skill: Send + Sync {
    fn manifest(&self) -> SkillManifest;

    async fn preview(&self, args: serde_json::Value) -> anyhow::Result<SkillPreview>;

    async fn execute(
        &self,
        args: serde_json::Value,
        source: Option<&str>,
    ) -> anyhow::Result<SkillExecution>;
}

pub struct SkillRegistry {
    skills: Vec<Box<dyn Skill>>,
}

impl SkillRegistry {
    fn new() -> Self {
        Self {
            skills: vec![
                Box::new(system_cleaner::SystemCleanerSkill),
                Box::new(desktop_organizer::DesktopOrganizerSkill),
                Box::new(app_uninstaller::AppUninstallerSkill),
                Box::new(doc_summarizer::DocSummarizerSkill),
                Box::new(media_dedup::MediaDedupSkill),
                Box::new(system_tools::SystemToolsSkill),
            ],
        }
    }

    pub fn manifests(&self) -> Vec<SkillManifest> {
        self.skills.iter().map(|s| s.manifest()).collect()
    }

    pub fn find(&self, id: &str) -> Option<&dyn Skill> {
        self.skills
            .iter()
            .find(|s| s.manifest().id == id)
            .map(|b| b.as_ref())
    }
}

static REGISTRY: OnceLock<SkillRegistry> = OnceLock::new();

pub fn registry() -> &'static SkillRegistry {
    REGISTRY.get_or_init(SkillRegistry::new)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_lists_system_cleaner() {
        let manifests = registry().manifests();
        assert!(manifests.iter().any(|m| m.id == "system.cleaner"));
    }

    #[test]
    fn find_returns_some_for_known_id() {
        assert!(registry().find("system.cleaner").is_some());
        assert!(registry().find("does.not.exist").is_none());
    }
}
