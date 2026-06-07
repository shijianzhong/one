use std::sync::Arc;
use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

pub mod orchestrator;
pub mod tools;
pub mod factory;
pub mod main_agent;

pub use main_agent::MainAgent;
pub use orchestrator::{Orchestrator, OrchestratorEvent};
pub use factory::AgentFactory;

/// Trait for tools that agents can use
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters_schema(&self) -> Value;
    async fn call(&self, arguments: Value) -> Result<Value>;
}

/// Context for agent execution
pub struct AgentContext {
    pub session_id: String,
    pub history: Vec<crate::memory::types::ChatMessage>,
    pub metadata: std::collections::HashMap<String, String>,
}

impl AgentContext {
    pub fn new(session_id: String) -> Self {
        Self {
            session_id,
            history: Vec::new(),
            metadata: std::collections::HashMap::new(),
        }
    }

    pub fn add_message(&mut self, message: crate::memory::types::ChatMessage) {
        self.history.push(message);
    }
}

/// Response from an agent
#[derive(Debug, Clone)]
pub enum AgentResponse {
    Answer(String),
    /// Tool calls with optional accompanying thinking text
    ToolCalls(Vec<ToolCall>, String),
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

/// Core Agent trait
#[async_trait]
pub trait Agent: Send + Sync {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn system_prompt(&self) -> &str;
    fn tools(&self) -> Vec<Arc<dyn Tool>>;
    async fn step(&self, context: &mut AgentContext) -> Result<AgentResponse>;
    async fn step_stream(
        &self,
        context: &mut AgentContext,
        on_delta: Box<dyn FnMut(String) + Send>,
    ) -> Result<AgentResponse>;
}

pub struct BaseAgent {
    pub id: String,
    pub name: String,
    pub system_prompt: String,
    pub tools: Vec<Arc<dyn Tool>>,
    pub model: String,
    pub api_base: String,
    pub api_key: String,
}

impl BaseAgent {
    pub async fn call_llm(&self, context: &AgentContext) -> Result<AgentResponse> {
        self.call_llm_stream(context, Box::new(|_| {})).await
    }

    pub async fn call_llm_stream(
        &self,
        context: &AgentContext,
        on_delta: Box<dyn FnMut(String) + Send>,
    ) -> Result<AgentResponse> {
        let mut messages = vec![crate::memory::types::ChatMessage::new("system", &self.system_prompt)];
        messages.extend(context.history.clone());

        let tool_defs: Vec<serde_json::Value> = self.tools.iter().map(|t| {
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": t.name(),
                    "description": t.description(),
                    "parameters": t.parameters_schema()
                }
            })
        }).collect();

        let tool_defs_opt = if tool_defs.is_empty() { None } else { Some(&tool_defs[..]) };

        let response = crate::services::api::call_chat_api_stream(
            &self.api_base,
            &self.api_key,
            &self.model,
            &messages,
            tool_defs_opt,
            on_delta,
        ).await.map_err(|e| anyhow::anyhow!(e))?;

        if let Some(tool_calls) = response.get("tool_calls").and_then(|v| v.as_array()) {
            let calls = tool_calls.iter().map(|tc| ToolCall {
                id: tc["id"].as_str().unwrap_or_default().to_string(),
                name: tc["function"]["name"].as_str().unwrap_or_default().to_string(),
                arguments: tc["function"]["arguments"].as_str().unwrap_or_default().to_string(),
            }).collect();
            let thinking = response["content"].as_str().unwrap_or_default().to_string();
            Ok(AgentResponse::ToolCalls(calls, thinking))
        } else {
            let content = response["content"].as_str().unwrap_or_default().to_string();
            Ok(AgentResponse::Answer(content))
        }
    }

    /// Inner tool loop: calls LLM, executes tools, feeds results back, repeats until Answer.
    pub async fn step_with_tools(&self, context: &mut AgentContext) -> Result<AgentResponse> {
        let mut max_inner_steps = 10;
        loop {
            max_inner_steps -= 1;

            let tool_defs: Vec<serde_json::Value> = self.tools.iter().map(|t| {
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": t.name(),
                        "description": t.description(),
                        "parameters": t.parameters_schema()
                    }
                })
            }).collect();

            let tool_defs_opt = if tool_defs.is_empty() { None } else { Some(&tool_defs[..]) };

            let mut messages = vec![crate::memory::types::ChatMessage::new("system", &self.system_prompt)];
            messages.extend(context.history.clone());

            let response = crate::services::api::call_chat_api_stream(
                &self.api_base,
                &self.api_key,
                &self.model,
                &messages,
                tool_defs_opt,
                |_| {},
            ).await.map_err(|e| anyhow::anyhow!(e))?;

            if let Some(tool_calls) = response.get("tool_calls").and_then(|v| v.as_array()) {
                if tool_calls.is_empty() {
                    let content = response["content"].as_str().unwrap_or_default().to_string();
                    return Ok(AgentResponse::Answer(content));
                }

                let calls: Vec<ToolCall> = tool_calls.iter().map(|tc| ToolCall {
                    id: tc["id"].as_str().unwrap_or_default().to_string(),
                    name: tc["function"]["name"].as_str().unwrap_or_default().to_string(),
                    arguments: tc["function"]["arguments"].as_str().unwrap_or_default().to_string(),
                }).collect();

                let tool_calls_json: Vec<Value> = calls.iter().map(|c| serde_json::json!({
                    "id": c.id,
                    "type": "function",
                    "function": { "name": c.name, "arguments": c.arguments }
                })).collect();

                let assistant_content = response["content"].as_str().unwrap_or_default().to_string();
                context.add_message(crate::memory::types::ChatMessage {
                    role: "assistant".to_string(),
                    content: assistant_content,
                    tool_calls: Some(tool_calls_json),
                    tool_call_id: None,
                });

                for call in &calls {
                    let result = if let Some(tool) = self.tools.iter().find(|t| t.name() == call.name) {
                        let args: Value = serde_json::from_str(&call.arguments).unwrap_or(serde_json::json!({}));
                        match tool.call(args).await {
                            Ok(res) => res.to_string(),
                            Err(e) => format!("Error: {}", e),
                        }
                    } else {
                        format!("Error: Tool '{}' not found", call.name)
                    };

                    context.add_message(crate::memory::types::ChatMessage {
                        role: "tool".to_string(),
                        content: result,
                        tool_calls: None,
                        tool_call_id: Some(call.id.clone()),
                    });
                }

                if max_inner_steps == 0 {
                    return Ok(AgentResponse::Answer("Reached maximum inner tool call steps.".to_string()));
                }
                continue;
            }

            let content = response["content"].as_str().unwrap_or_default().to_string();
            context.add_message(crate::memory::types::ChatMessage {
                role: "assistant".to_string(),
                content: content.clone(),
                tool_calls: None,
                tool_call_id: None,
            });
            return Ok(AgentResponse::Answer(content));
        }
    }
}
