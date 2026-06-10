use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use async_trait::async_trait;
use serde::Serialize;
use serde_json::Value;

use crate::skills::SkillExecution;

// ── ToolSource ──────────────────────────────────────────────────────────────────

/// 工具来源：一个 Tool 可以来自不同的后端
#[derive(Clone, Debug)]
pub enum ToolSource {
    /// 内置 Rust 工具（remember/recall 等）
    Builtin(Arc<dyn crate::agents::core::Tool>),
    /// 注册的 Skill（来自 SkillRegistry）
    Skill(String),  // skill_id
    /// 通过 MCP 发现的外部工具
    Mcp { server: String, tool_name: String },
}

impl std::fmt::Debug for dyn crate::agents::core::Tool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolTrait").field("name", &self.name()).finish()
    }
}

// ── ToolResult ──────────────────────────────────────────────────────────────────

/// 统一的工具路由结果
#[derive(Debug, Clone)]
pub enum ToolResult {
    /// 内置工具直接返回
    Builtin(Value),
    /// Skill 执行结果
    Skill(SkillExecution),
    /// MCP 调用结果
    Mcp(Value),
}

// ── ToolTrait（与现有 Tool 区分） ───────────────────────────────────────────────

/// 工具 trait（用于 ToolRegistry 注册，与旧 Tool 兼容）
#[async_trait]
pub trait ToolTrait: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters_schema(&self) -> Value;
    async fn call(&self, arguments: Value) -> anyhow::Result<Value>;
}

// ── ToolDefinition（LLM tool calling 格式） ─────────────────────────────────────

/// 格式化后的工具定义，用于 LLM tool calling
#[derive(Debug, Clone, Serialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

// ── AgentRunContext ─────────────────────────────────────────────────────────────

/// Agent 运行时的完整上下文
pub struct AgentRunContext {
    pub session_id: String,
    pub history: Vec<crate::memory::types::ChatMessage>,
    pub metadata: std::collections::HashMap<String, String>,
    /// 可用的工具来源列表
    pub tool_sources: Vec<ToolSource>,
    /// 取消标志
    pub cancel_flag: Option<Arc<AtomicBool>>,
    /// 用户输入通道（支持多轮交互）
    pub user_input_rx: Option<tokio::sync::mpsc::UnboundedReceiver<String>>,
}

impl AgentRunContext {
    pub fn new(session_id: String) -> Self {
        Self {
            session_id,
            history: Vec::new(),
            metadata: std::collections::HashMap::new(),
            tool_sources: Vec::new(),
            cancel_flag: None,
            user_input_rx: None,
        }
    }

    pub fn add_message(&mut self, message: crate::memory::types::ChatMessage) {
        self.history.push(message);
    }
}

// ── AgentTrait（新抽象基类，与旧 Agent 区分） ──────────────────────────────────

/// Agent 抽象基类。所有 Agent 类型实现此 trait。
///
/// Agent 只关心：
/// - 自己的身份和人格（id, name, soul_prompt）
/// - 对工具的过滤条件（tool_filter）
///
/// Agent **不持有工具**，工具通过 ToolRegistry 在运行时注入。
///
/// 注意：这是新设计的抽象基类，与旧 `mod.rs` 中的 `Agent` trait 不同名。
/// 旧 `Agent` trait 是 `CoreAgent`，用于 `MainAgent` 当前的 step_stream 实现。
/// 新 `AgentTrait` 是逐步替换的目标。
#[async_trait]
pub trait AgentTrait: Send + Sync {
    /// Agent 唯一标识
    fn id(&self) -> &str;
    /// 显示名称
    fn name(&self) -> &str;
    /// Agent 的灵魂/人格设定
    fn soul_prompt(&self) -> &str;

    /// 获取该 Agent 专属的工具过滤条件。
    /// 返回 `Some(list)` 时，只保留列表中命名的工具。
    /// 返回 `None` 时，所有工具可用。
    fn tool_filter(&self) -> Option<Vec<String>> {
        None
    }

    /// 生成最终的 system prompt。
    /// 框架自动拼接 soul + 可用工具描述 + 记忆上下文。
    fn build_system_prompt(&self, tool_descriptions: &str) -> String {
        format!("{}\n\n## 可用工具\n\n{}", self.soul_prompt(), tool_descriptions)
    }
}

// ── AgentBuilder ────────────────────────────────────────────────────────────────

/// Agent 构建器 trait。每个 Agent 类型注册一个构建器到 AgentRegistry。
#[async_trait]
pub trait AgentBuilder: Send + Sync {
    fn agent_id(&self) -> &str;
    fn agent_name(&self) -> &str;
    fn build(&self, config: &crate::services::Config, workspace: &str) -> Box<dyn AgentTrait>;
}