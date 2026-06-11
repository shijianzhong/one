//! 动态 Skill 加载器
//!
//! 从 ~/.one/skills/<name>/ 目录加载 SKILL.md 文件，
//! 解析 frontmatter 和 Markdown 内容，创建可执行的 DynamicSkill。
//!
//! # SKILL.md 规范
//!
//! ```markdown
//! ---
//! name: skill-name
//! description: 技能描述
//! version: 1.0.0
//! author: ONE
//! platforms: [macos, linux]
//! danger_level: Normal
//! category: Development
//! executor:
//!   type: mcp_tool        # mcp_tool | command
//!   server: server-name   # MCP 服务器名（mcp_tool 时必填）
//!   tool: tool-name       # MCP 工具名（mcp_tool 时必填）
//!   # 或
//!   # type: command
//!   # command: "claude -p '{task}'"
//! ---
//!
//! # Skill 功能描述
//!
//! 此 Skill 的详细说明，给 LLM 阅读。
//! ```

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::agents::permission::DangerLevel;
use crate::skills::{Skill, SkillCategory, SkillExecution, SkillManifest, SkillPreview, SkillPreviewItem};

// ── SKILL.md 解析 ───────────────────────────────────────────────────────────────

/// SKILL.md 的 YAML frontmatter
#[derive(Debug, Deserialize)]
pub struct SkillFrontmatter {
    pub name: String,
    pub description: String,
    #[serde(default = "default_version")]
    pub version: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub platforms: Vec<String>,
    #[serde(default)]
    pub danger_level: DangerLevel,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub executor: Option<ExecutorConfig>,
}

fn default_version() -> String {
    "1.0.0".to_string()
}

/// 执行器配置
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum ExecutorConfig {
    #[serde(rename = "mcp_tool")]
    McpTool {
        server: String,
        tool: String,
        #[serde(default)]
        args_template: Option<String>,
    },
    #[serde(rename = "command")]
    Command {
        command: String,
        #[serde(default)]
        args: Vec<String>,
    },
}

/// 解析后的 Skill 文档
#[derive(Debug)]
pub struct ParsedSkillDoc {
    pub frontmatter: SkillFrontmatter,
    pub body: String,
}

impl ParsedSkillDoc {
    /// 从 SKILL.md 内容解析
    pub fn parse(content: &str) -> Result<Self> {
        let content = content.trim();
        if !content.starts_with("---") {
            anyhow::bail!("SKILL.md must start with '---'");
        }

        // 找到第二个 ---
        let end = content[3..]
            .find("\n---")
            .map(|pos| pos + 3)
            .context("SKILL.md missing closing '---'")?;

        let frontmatter_str = &content[3..end].trim();
        let body = content[end + 4..].trim().to_string();

        let frontmatter: SkillFrontmatter = serde_yaml::from_str(frontmatter_str)
            .context("Failed to parse SKILL.md frontmatter")?;

        Ok(Self {
            frontmatter,
            body,
        })
    }
}

// ── DynamicSkill ────────────────────────────────────────────────────────────────

/// 从 SKILL.md 文件加载的动态 Skill
#[derive(Debug)]
pub struct DynamicSkill {
    pub manifest: SkillManifest,
    pub executor: SkillExecutor,
    pub body: String,
    pub source_dir: PathBuf,
}

/// Skill 执行器
#[derive(Debug, Clone)]
pub enum SkillExecutor {
    /// 通过 MCP 调用
    McpTool {
        server: String,
        tool: String,
        args_template: Option<String>,
    },
    /// 直接执行终端命令
    Command {
        command: String,
        args: Vec<String>,
    },
}

impl DynamicSkill {
    /// 从目录加载 Skill（目录下必须有 SKILL.md）
    pub fn load_from_dir(dir: &Path) -> Result<Self> {
        let skill_path = dir.join("SKILL.md");
        let content = std::fs::read_to_string(&skill_path)
            .context(format!("Failed to read SKILL.md: {}", skill_path.display()))?;

        let parsed = ParsedSkillDoc::parse(&content)?;
        let fm = &parsed.frontmatter;

        // 平台检查
        if !fm.platforms.is_empty() {
            let current_os = std::env::consts::OS;
            let os_alias = match current_os {
                "macos" => "macos",
                "linux" => "linux",
                "windows" => "windows",
                _ => current_os,
            };
            if !fm.platforms.iter().any(|p| p == os_alias) {
                anyhow::bail!(
                    "Skill '{}' does not support current platform '{}' (supports: {:?})",
                    fm.name, current_os, fm.platforms
                );
            }
        }

        // 解析执行器
        let executor = match &fm.executor {
            Some(ExecutorConfig::McpTool { server, tool, args_template }) => {
                SkillExecutor::McpTool {
                    server: server.clone(),
                    tool: tool.clone(),
                    args_template: args_template.clone(),
                }
            }
            Some(ExecutorConfig::Command { command, args }) => {
                SkillExecutor::Command {
                    command: command.clone(),
                    args: args.clone(),
                }
            }
            None => {
                anyhow::bail!("SKILL.md must define an executor");
            }
        };

        // 推断 category
        let category = match fm.category.as_deref() {
            Some("System") | Some("系统") => SkillCategory::System,
            Some("Desktop") | Some("桌面") => SkillCategory::Desktop,
            Some("App") | Some("应用") => SkillCategory::App,
            Some("Doc") | Some("文档") => SkillCategory::Doc,
            Some("Media") | Some("媒体") => SkillCategory::Media,
            Some("Development") | Some("开发") => SkillCategory::System,
            _ => SkillCategory::System,
        };

        let manifest = SkillManifest {
            id: format!("skill.{}", fm.name),
            name: fm.name.clone(),
            description: fm.description.clone(),
            category,
            danger_level: fm.danger_level,
        };

        Ok(Self {
            manifest,
            executor,
            body: parsed.body,
            source_dir: dir.to_path_buf(),
        })
    }

    /// 构造预览信息（显示 SKILL.md 的描述和参数）
    pub fn build_preview(&self, _args: &serde_json::Value) -> SkillPreview {
        let executor_desc = match &self.executor {
            SkillExecutor::McpTool { server, tool, .. } => {
                format!("通过 MCP 调用 {}/{}", server, tool)
            }
            SkillExecutor::Command { command, .. } => {
                format!("执行命令: {}", command)
            }
        };

        SkillPreview {
            summary: format!("{} — {}", self.manifest.name, executor_desc),
            items: vec![SkillPreviewItem {
                label: "执行方式".to_string(),
                detail: executor_desc,
                bytes: 0,
            }],
            estimated_bytes: 0,
            warnings: vec![],
        }
    }

    /// 构造执行结果（实际执行由 MCP 或命令完成）
    pub fn build_execution(&self, result: String) -> SkillExecution {
        SkillExecution {
            summary: result.clone(),
            freed_bytes: 0,
            success_items: vec![result],
            failed_items: vec![],
            denied: false,
        }
    }
}

// ── 目录扫描 ────────────────────────────────────────────────────────────────────

/// 扫描 ~/.one/skills/ 目录，返回所有有效的 DynamicSkill
pub fn scan_skills_dir() -> Vec<DynamicSkill> {
    let root = skills_root_dir();
    if !root.exists() {
        return Vec::new();
    }

    let mut skills = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && path.join("SKILL.md").exists() {
                match DynamicSkill::load_from_dir(&path) {
                    Ok(skill) => {
                        log::info!("[Skills] Loaded dynamic skill: {} ({})", skill.manifest.name, path.display());
                        skills.push(skill);
                    }
                    Err(e) => {
                        log::warn!("[Skills] Failed to load skill from {}: {}", path.display(), e);
                    }
                }
            }
        }
    }

    skills
}

/// 获取 Skills 根目录（~/.one/skills/）
fn skills_root_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".one")
        .join("skills")
}

// ── Skill trait 实现 ────────────────────────────────────────────────────────────

use async_trait::async_trait;

#[async_trait]
impl Skill for DynamicSkill {
    fn manifest(&self) -> SkillManifest {
        self.manifest.clone()
    }

    async fn preview(&self, args: serde_json::Value) -> Result<SkillPreview> {
        Ok(self.build_preview(&args))
    }

    async fn execute(
        &self,
        _args: serde_json::Value,
        _source: Option<&str>,
    ) -> Result<SkillExecution> {
        // SKILL.md 的 body 是给 AI 阅读的操作指南。
        // 直接返回 body 内容，Agent 阅读后自行决定如何执行
        // （例如调用终端命令 claude -p 等）
        Ok(self.build_execution(format!(
            "## Skill: {}\n\n以下是从 SKILL.md 加载的使用指南：\n\n{}\n\n请按照上述指南执行任务。",
            self.manifest.name,
            self.body
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_skill_md() {
        let content = r#"---
name: test-skill
description: A test skill
version: 1.0.0
author: ONE
platforms: [macos, linux]
danger_level: Normal
executor:
  type: mcp_tool
  server: test-server
  tool: test-tool
---
# Test Skill

This is a test skill.
"#;
        let parsed = ParsedSkillDoc::parse(content).unwrap();
        assert_eq!(parsed.frontmatter.name, "test-skill");
        assert_eq!(parsed.frontmatter.description, "A test skill");
        assert!(parsed.body.contains("Test Skill"));
    }

    #[test]
    fn test_parse_command_skill() {
        let content = r#"---
name: cmd-skill
description: A command skill
executor:
  type: command
  command: echo
  args: ["hello"]
---
Simple command skill.
"#;
        let parsed = ParsedSkillDoc::parse(content).unwrap();
        match &parsed.frontmatter.executor {
            Some(ExecutorConfig::Command { command, .. }) => {
                assert_eq!(command, "echo");
            }
            _ => panic!("expected command executor"),
        }
    }

    #[test]
    fn test_invalid_skill_md() {
        assert!(ParsedSkillDoc::parse("no frontmatter").is_err());
    }

    #[test]
    fn test_platform_check() {
        // 当前平台应该在支持列表中
        let content = format!(
            r#"---
name: test
description: test
executor:
  type: mcp_tool
  server: s
  tool: t
platforms: [{}]
---"#,
            std::env::consts::OS
        );
        assert!(ParsedSkillDoc::parse(&content).is_ok());
    }
}