use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use async_trait::async_trait;
use serde::Serialize;
use serde_json::Value;

// ── ToolSource ──────────────────────────────────────────────────────────────────

/// 工具来源：一个 Tool 可以来自不同的后端
#[derive(Clone, Debug)]
pub enum ToolSource {
    /// 内置 Rust 工具（remember/recall 等）
    Builtin(Arc<dyn crate::agents::core::Tool>),
    /// 注册的 Skill（来自 SkillRegistry）
    Skill(String), // skill_id
    /// 通过 MCP 发现的外部工具
    Mcp { server: String, tool_name: String },
}

impl std::fmt::Debug for dyn crate::agents::core::Tool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tool").field("name", &self.name()).finish()
    }
}

// ── ToolDefinition（LLM tool calling 格式） ─────────────────────────────────────

/// 格式化后的工具定义，用于 LLM tool calling
#[derive(Debug, Clone, Serialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

impl ToolDefinition {
    pub fn as_openai_tool(&self) -> Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": self.name,
                "description": self.description,
                "parameters": self.input_schema,
            }
        })
    }
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone)]
pub enum AgentResponse {
    Answer(String),
    ToolCalls(Vec<ToolCall>, String),
}

// ── AgentRunContext ─────────────────────────────────────────────────────────────

/// Agent 运行时的完整上下文
pub struct AgentRunContext {
    pub session_id: String,
    pub history: Vec<crate::memory::types::ChatMessage>,
    pub metadata: std::collections::HashMap<String, String>,
    /// 可用的工具来源列表
    pub tool_sources: Vec<ToolSource>,
    /// 格式化后的 LLM tool definitions，来自 ToolRegistry + Agent filter。
    pub tool_definitions: Vec<ToolDefinition>,
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
            tool_definitions: Vec::new(),
            cancel_flag: None,
            user_input_rx: None,
        }
    }

    pub fn add_message(&mut self, message: crate::memory::types::ChatMessage) {
        self.history.push(message);
    }
}

// ── AgentTrait ─────────────────────────────────────────────────────────────────

/// Agent 抽象基类。所有 Agent 类型实现此 trait。
///
/// Agent 只关心：
/// - 自己的身份和人格（id, name, soul_prompt）
/// - 对工具的过滤条件（tool_filter）
///
/// Agent **不持有工具**，工具通过 ToolRegistry 在运行时注入。
///
#[async_trait]
pub trait AgentTrait: Send + Sync {
    /// Agent 唯一标识
    fn id(&self) -> &str;
    /// 显示名称
    fn name(&self) -> &str;
    /// Agent 的灵魂/人格设定
    fn soul_prompt(&self) -> &str;
    fn model(&self) -> &str;
    fn api_base(&self) -> &str;
    fn api_key(&self) -> &str;

    /// 获取该 Agent 专属的工具过滤条件。
    /// 返回 `Some(list)` 时，只保留列表中命名的工具。
    /// 返回 `None` 时，所有工具可用。
    fn tool_filter(&self) -> Option<Vec<String>> {
        None
    }

    /// 生成最终的 system prompt。
    /// 框架自动拼接 soul + 可用工具描述 + 记忆上下文。
    fn build_system_prompt(&self, tool_descriptions: &str) -> String {
        format!(
            "{}\n\n## 可用工具\n\n{}",
            self.soul_prompt(),
            tool_descriptions
        )
    }

    async fn step_stream(
        &self,
        context: &mut AgentRunContext,
        on_delta: Box<dyn FnMut(String) + Send>,
    ) -> anyhow::Result<AgentResponse> {
        let tool_descriptions =
            crate::agents::core::tool_registry::format_tool_descriptions(&context.tool_definitions);
        let mut messages = vec![crate::memory::types::ChatMessage::new(
            "system",
            &self.build_system_prompt(&tool_descriptions),
        )];
        messages.extend(context.history.clone());

        let tools: Vec<Value> = context
            .tool_definitions
            .iter()
            .map(ToolDefinition::as_openai_tool)
            .collect();
        let tool_defs_opt = if tools.is_empty() {
            None
        } else {
            Some(&tools[..])
        };

        let response = crate::services::api::call_chat_api_stream(
            self.api_base(),
            self.api_key(),
            self.model(),
            &messages,
            tool_defs_opt,
            on_delta,
        )
        .await
        .map_err(|e| anyhow::anyhow!(e))?;

        if let Some(tool_calls) = response.get("tool_calls").and_then(|v| v.as_array()) {
            let calls = tool_calls
                .iter()
                .map(|tc| ToolCall {
                    id: tc["id"].as_str().unwrap_or_default().to_string(),
                    name: tc["function"]["name"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                    arguments: tc["function"]["arguments"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                })
                .collect();
            let thinking = response["content"].as_str().unwrap_or_default().to_string();
            Ok(AgentResponse::ToolCalls(calls, thinking))
        } else {
            let content = response["content"].as_str().unwrap_or_default().to_string();
            Ok(AgentResponse::Answer(content))
        }
    }
}
