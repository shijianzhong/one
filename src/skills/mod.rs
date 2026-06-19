#![allow(dead_code)]

use std::sync::OnceLock;

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

/// 统一的 Skill 枚举（解决 dyn Skill 的兼容性问题）
pub enum AnySkill {
    Builtin(Box<dyn Skill>),
    Dynamic(dynamic_skill::DynamicSkill),
}

#[async_trait]
impl Skill for AnySkill {
    fn manifest(&self) -> SkillManifest {
        match self {
            AnySkill::Builtin(s) => s.manifest(),
            AnySkill::Dynamic(s) => s.manifest(),
        }
    }

    async fn preview(&self, args: serde_json::Value) -> anyhow::Result<SkillPreview> {
        match self {
            AnySkill::Builtin(s) => s.preview(args).await,
            AnySkill::Dynamic(s) => s.preview(args).await,
        }
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        source: Option<&str>,
    ) -> anyhow::Result<SkillExecution> {
        match self {
            AnySkill::Builtin(s) => s.execute(args, source).await,
            AnySkill::Dynamic(s) => s.execute(args, source).await,
        }
    }
}

pub struct SkillRegistry {
    skills: Vec<AnySkill>,
}

impl SkillRegistry {
    fn new() -> Self {
        let builtin: Vec<AnySkill> = vec![
            AnySkill::Builtin(Box::new(system_cleaner::SystemCleanerSkill)),
            AnySkill::Builtin(Box::new(desktop_organizer::DesktopOrganizerSkill)),
            AnySkill::Builtin(Box::new(app_uninstaller::AppUninstallerSkill)),
            AnySkill::Builtin(Box::new(doc_summarizer::DocSummarizerSkill)),
            AnySkill::Builtin(Box::new(media_dedup::MediaDedupSkill)),
            AnySkill::Builtin(Box::new(system_tools::SystemToolsSkill)),
        ];

        let dynamic: Vec<AnySkill> = dynamic_skill::scan_skills_dir()
            .into_iter()
            .map(AnySkill::Dynamic)
            .collect();

        let mut skills = builtin;
        skills.extend(dynamic);
        Self { skills }
    }

    pub fn refresh_dynamic(&mut self) {
        // 移除旧的动态 Skill
        self.skills.retain(|s| matches!(s, AnySkill::Builtin(_)));
        // 添加新的动态 Skill
        let dynamic: Vec<AnySkill> = dynamic_skill::scan_skills_dir()
            .into_iter()
            .map(AnySkill::Dynamic)
            .collect();
        self.skills.extend(dynamic);
    }

    pub fn manifests(&self) -> Vec<SkillManifest> {
        self.skills.iter().map(|s| s.manifest()).collect()
    }

    pub fn find(&self, id: &str) -> Option<&AnySkill> {
        self.skills.iter().find(|s| s.manifest().id == id)
    }
}

static REGISTRY: OnceLock<SkillRegistry> = OnceLock::new();

pub fn registry() -> &'static SkillRegistry {
    REGISTRY.get_or_init(SkillRegistry::new)
}

/// 刷新动态 Skill
pub fn refresh_dynamic_skills() {
    // 确保 REGISTRY 已初始化
    let reg = registry();
    // 安全：GPUI 是单线程的
    let ptr = reg as *const SkillRegistry as *mut SkillRegistry;
    unsafe {
        (*ptr).refresh_dynamic();
    }
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
