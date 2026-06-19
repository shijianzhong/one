#![allow(dead_code)]

use std::sync::{Arc, OnceLock, RwLock};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::agents::permission::DangerLevel;

pub mod app_uninstaller;
pub mod desktop_organizer;
pub mod doc_summarizer;
pub mod dynamic_skill;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SkillOrigin {
    Builtin,
    Dynamic,
}

struct SkillEntry {
    origin: SkillOrigin,
    skill: Arc<dyn Skill>,
}

pub struct SkillRegistry {
    skills: Vec<SkillEntry>,
}

impl SkillRegistry {
    fn new() -> Self {
        let builtin: Vec<SkillEntry> = vec![
            SkillEntry::builtin(system_cleaner::SystemCleanerSkill),
            SkillEntry::builtin(desktop_organizer::DesktopOrganizerSkill),
            SkillEntry::builtin(app_uninstaller::AppUninstallerSkill),
            SkillEntry::builtin(doc_summarizer::DocSummarizerSkill),
            SkillEntry::builtin(media_dedup::MediaDedupSkill),
            SkillEntry::builtin(system_tools::SystemToolsSkill),
        ];

        let dynamic: Vec<SkillEntry> = dynamic_skill::scan_skills_dir()
            .into_iter()
            .map(SkillEntry::dynamic)
            .collect();

        let mut skills = builtin;
        skills.extend(dynamic);
        Self { skills }
    }

    pub fn refresh_dynamic(&mut self) {
        self.skills
            .retain(|entry| entry.origin == SkillOrigin::Builtin);
        let dynamic: Vec<SkillEntry> = dynamic_skill::scan_skills_dir()
            .into_iter()
            .map(SkillEntry::dynamic)
            .collect();
        self.skills.extend(dynamic);
    }

    pub fn manifests(&self) -> Vec<SkillManifest> {
        self.skills
            .iter()
            .map(|entry| entry.skill.manifest())
            .collect()
    }

    pub fn find(&self, id: &str) -> Option<Arc<dyn Skill>> {
        self.skills
            .iter()
            .find(|entry| entry.skill.manifest().id == id)
            .map(|entry| entry.skill.clone())
    }
}

impl SkillEntry {
    fn builtin(skill: impl Skill + 'static) -> Self {
        Self {
            origin: SkillOrigin::Builtin,
            skill: Arc::new(skill),
        }
    }

    fn dynamic(skill: dynamic_skill::DynamicSkill) -> Self {
        Self {
            origin: SkillOrigin::Dynamic,
            skill: Arc::new(skill),
        }
    }
}

static REGISTRY: OnceLock<RwLock<SkillRegistry>> = OnceLock::new();

pub fn registry() -> &'static RwLock<SkillRegistry> {
    REGISTRY.get_or_init(|| RwLock::new(SkillRegistry::new()))
}

pub fn skill_manifests() -> Vec<SkillManifest> {
    registry()
        .read()
        .map(|registry| registry.manifests())
        .unwrap_or_default()
}

pub fn find_skill(id: &str) -> Option<Arc<dyn Skill>> {
    registry()
        .read()
        .ok()
        .and_then(|registry| registry.find(id))
}

/// 刷新动态 Skill
pub fn refresh_dynamic_skills() {
    if let Ok(mut registry) = registry().write() {
        registry.refresh_dynamic();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_lists_system_cleaner() {
        let manifests = skill_manifests();
        assert!(manifests.iter().any(|m| m.id == "system.cleaner"));
    }

    #[test]
    fn find_returns_some_for_known_id() {
        assert!(find_skill("system.cleaner").is_some());
        assert!(find_skill("does.not.exist").is_none());
    }
}
