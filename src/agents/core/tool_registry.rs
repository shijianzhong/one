use std::sync::OnceLock;
use std::sync::Arc;

use serde_json::Value;

use super::agent::{ToolDefinition, ToolSource};
use crate::agents::core::Tool;

/// Skill 工具注册信息
#[derive(Clone)]
pub struct SkillToolRegistration {
    pub skill_id: String,
    pub name: String,
    pub description: String,
    pub parameters_schema: Value,
}

/// MCP 工具注册信息
#[derive(Clone)]
pub struct McpToolRegistration {
    pub server_name: String,
    pub tool_name: String,
    pub description: String,
    pub input_schema: Value,
}

/// 全局工具注册表。所有工具（Builtin + Skill + MCP）统一注册到此。
///
/// Orchestrator 在构造 AgentRunContext 时从此拉取 tool_sources。
/// Agent 本身的 tool_filter 可用来过滤。
pub struct ToolRegistry {
    builtin_tools: Vec<Arc<dyn Tool>>,
    skill_tools: Vec<SkillToolRegistration>,
    mcp_tools: Vec<McpToolRegistration>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            builtin_tools: Vec::new(),
            skill_tools: Vec::new(),
            mcp_tools: Vec::new(),
        }
    }

    // ── 注册 ────────────────────────────────────────────────────────────────────

    pub fn register_builtin(&mut self, tool: Arc<dyn Tool>) {
        self.builtin_tools.push(tool);
    }

    pub fn register_skill(&mut self, skill: SkillToolRegistration) {
        if let Some(pos) = self.skill_tools.iter().position(|s| s.skill_id == skill.skill_id) {
            self.skill_tools[pos] = skill;
        } else {
            self.skill_tools.push(skill);
        }
    }

    pub fn register_mcp(&mut self, mcp: McpToolRegistration) {
        let key = format!("{}:{}", mcp.server_name, mcp.tool_name);
        if let Some(pos) = self.mcp_tools.iter().position(|m| {
            format!("{}:{}", m.server_name, m.tool_name) == key
        }) {
            self.mcp_tools[pos] = mcp;
        } else {
            self.mcp_tools.push(mcp);
        }
    }

    pub fn register_mcp_batch(&mut self, tools: Vec<McpToolRegistration>) {
        for tool in tools {
            self.register_mcp(tool);
        }
    }

    // ── 查询 ────────────────────────────────────────────────────────────────────

    pub fn tool_definitions(&self, agent_filter: Option<&[String]>) -> Vec<ToolDefinition> {
        let mut defs = Vec::new();

        for tool in &self.builtin_tools {
            if let Some(filter) = agent_filter {
                if !filter.contains(&tool.name().to_string()) {
                    continue;
                }
            }
            defs.push(ToolDefinition {
                name: tool.name().to_string(),
                description: tool.description().to_string(),
                input_schema: tool.parameters_schema(),
            });
        }

        for skill in &self.skill_tools {
            let full_name = format!("skill:{}", skill.skill_id);
            if let Some(filter) = agent_filter {
                if !filter.contains(&full_name) && !filter.contains(&skill.skill_id) {
                    continue;
                }
            }
            defs.push(ToolDefinition {
                name: full_name,
                description: skill.description.clone(),
                input_schema: skill.parameters_schema.clone(),
            });
        }

        for mcp in &self.mcp_tools {
            let full_name = format!("mcp:{}:{}", mcp.server_name, mcp.tool_name);
            if let Some(filter) = agent_filter {
                if !filter.contains(&full_name) {
                    continue;
                }
            }
            defs.push(ToolDefinition {
                name: full_name,
                description: mcp.description.clone(),
                input_schema: mcp.input_schema.clone(),
            });
        }

        defs
    }

    pub(crate) fn builtin_tools(&self) -> &[Arc<dyn Tool>] {
        &self.builtin_tools
    }

    pub fn skill_tools(&self) -> &[SkillToolRegistration] {
        &self.skill_tools
    }

    pub fn mcp_tools(&self) -> &[McpToolRegistration] {
        &self.mcp_tools
    }

    pub fn clear_mcp_tools(&mut self) {
        self.mcp_tools.clear();
    }

    // ── 执行 ────────────────────────────────────────────────────────────────────

    pub fn resolve_tool_source(&self, name: &str) -> Option<ToolSource> {
        if let Some(tool) = self.builtin_tools.iter().find(|t| t.name() == name) {
            return Some(ToolSource::Builtin(tool.clone()));
        }

        if let Some(skill_id) = name.strip_prefix("skill:") {
            if self.skill_tools.iter().any(|s| s.skill_id == skill_id) {
                return Some(ToolSource::Skill(skill_id.to_string()));
            }
        }

        if let Some(rest) = name.strip_prefix("mcp:") {
            if let Some((server, tool_name)) = rest.split_once(':') {
                if self.mcp_tools.iter().any(|m| m.server_name == server && m.tool_name == tool_name) {
                    return Some(ToolSource::Mcp {
                        server: server.to_string(),
                        tool_name: tool_name.to_string(),
                    });
                }
            }
        }

        None
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// 将 ToolDefinition 列表格式化为 system prompt 可读的文本
pub fn format_tool_descriptions(defs: &[ToolDefinition]) -> String {
    if defs.is_empty() {
        return "（当前没有可用工具）".to_string();
    }
    defs.iter()
        .map(|d| {
            format!("- **{}**: {}", d.name, d.description)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

impl ToolSource {
    /// 获取显示名称（用于 UI 和日志）
    pub fn display_name(&self) -> &str {
        match self {
            ToolSource::Builtin(t) => t.name(),
            ToolSource::Skill(id) => id,
            ToolSource::Mcp { server, tool_name } => {
                // 使用 leak 简化，实际应该返回 String
                Box::leak(format!("{}/{}", server, tool_name).into_boxed_str())
            }
        }
    }
}

// ── 全局单例 ────────────────────────────────────────────────────────────────────

static REGISTRY: OnceLock<std::sync::Mutex<ToolRegistry>> = OnceLock::new();

pub fn tool_registry() -> &'static std::sync::Mutex<ToolRegistry> {
    REGISTRY.get_or_init(|| std::sync::Mutex::new(ToolRegistry::new()))
}

/// 初始化 ToolRegistry 并注册所有 Builtin 工具和 Skill 工具
pub fn init_tool_registry() {
    let mut registry = tool_registry().lock().expect("ToolRegistry lock");

    for manifest in crate::skills::registry().manifests() {
        registry.register_skill(SkillToolRegistration {
            skill_id: manifest.id.clone(),
            name: format!("skill:{}", manifest.id),
            description: manifest.description.clone(),
            parameters_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "apply": {
                        "type": "boolean",
                        "description": "false=preview, true=execute. 默认 false。"
                    },
                    "args": {
                        "type": "object",
                        "description": "传给 Skill 的参数"
                    }
                }
            }),
        });
    }
}