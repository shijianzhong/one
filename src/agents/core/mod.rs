use std::sync::Arc;
use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

pub mod coordinator;
pub mod orchestrator;
pub mod tools;
pub mod system_agent;
pub mod coding_agent;
pub mod factory;
pub mod memory_agent;
pub mod general_agent;

pub use coordinator::Coordinator;
pub use orchestrator::{Orchestrator, OrchestratorEvent};
pub use system_agent::SystemAgent;
pub use coding_agent::CodingAgent;
pub use memory_agent::MemoryAgent;
pub use general_agent::GeneralAgent;
pub use factory::AgentFactory;

/// Trait for tools that agents can use
#[async_trait]
pub trait Tool: Send + Sync {
    /// Unique name of the tool
    fn name(&self) -> &str;
    
    /// Description of what the tool does
    fn description(&self) -> &str;
    
    /// JSON Schema of the parameters
    fn parameters_schema(&self) -> Value;
    
    /// Execute the tool with given arguments
    async fn call(&self, arguments: Value) -> Result<Value>;
}

/// Context for agent execution, managing state and memory
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
    /// Final answer to the user
    Answer(String),
    /// Request to call tools. The String is the thinking/plan text that accompanied
    /// the tool calls (may be empty).
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
    /// Unique identifier for the agent type
    fn id(&self) -> &str;
    
    /// Display name of the agent
    fn name(&self) -> &str;
    
    /// The system prompt that defines the agent's persona and behavior
    fn system_prompt(&self) -> &str;
    
    /// List of tools available to this agent
    fn tools(&self) -> Vec<Arc<dyn Tool>>;
    
    /// Handle a single turn of interaction
    async fn step(&self, context: &mut AgentContext) -> Result<AgentResponse>;
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
        let mut messages = vec![crate::memory::types::ChatMessage::new("system", &self.system_prompt)];
        messages.extend(context.history.clone());

        let tool_defs: Vec<serde_json::Value> = if self.tools.is_empty() {
            vec![]
        } else {
            self.tools.iter().map(|t| {
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": t.name(),
                        "description": t.description(),
                        "parameters": t.parameters_schema()
                    }
                })
            }).collect()
        };

        let tool_defs_opt = if tool_defs.is_empty() { None } else { Some(&tool_defs[..]) };

        let response = crate::services::api::call_chat_api_stream(
            &self.api_base,
            &self.api_key,
            &self.model,
            &messages,
            tool_defs_opt,
            |_| {}
        ).await.map_err(|e| anyhow::anyhow!(e))?;

        if let Some(tool_calls) = response.get("tool_calls").and_then(|v| v.as_array()) {
            let mut calls = Vec::new();
            for tc in tool_calls {
                calls.push(ToolCall {
                    id: tc["id"].as_str().unwrap_or_default().to_string(),
                    name: tc["function"]["name"].as_str().unwrap_or_default().to_string(),
                    arguments: tc["function"]["arguments"].as_str().unwrap_or_default().to_string(),
                });
            }
            // Preserve the thinking text that accompanied tool calls
            let thinking = response["content"].as_str().unwrap_or_default().to_string();
            Ok(AgentResponse::ToolCalls(calls, thinking))
        } else {
            let content = response["content"].as_str().unwrap_or_default().to_string();
            Ok(AgentResponse::Answer(content))
        }
    }

    /// Full turn: calls LLM, executes tools, feeds results back, repeats until we get an Answer.
    /// This is the "inner loop" — one orchestration step for this agent.
    pub async fn step_with_tools(&self, context: &mut AgentContext) -> Result<AgentResponse> {
        let mut max_inner_steps = 10;
        loop {
            max_inner_steps -= 1;

            let tool_defs: Vec<serde_json::Value> = if self.tools.is_empty() {
                vec![]
            } else {
                self.tools.iter().map(|t| {
                    serde_json::json!({
                        "type": "function",
                        "function": {
                            "name": t.name(),
                            "description": t.description(),
                            "parameters": t.parameters_schema()
                        }
                    })
                }).collect()
            };

            let tool_defs_opt = if tool_defs.is_empty() { None } else { Some(&tool_defs[..]) };

            let mut messages = vec![crate::memory::types::ChatMessage::new("system", &self.system_prompt)];
            messages.extend(context.history.clone());

            let response = crate::services::api::call_chat_api_stream(
                &self.api_base,
                &self.api_key,
                &self.model,
                &messages,
                tool_defs_opt,
                |_| {}
            ).await.map_err(|e| anyhow::anyhow!(e))?;

            // If the LLM called tools, execute them and feed the results back
            if let Some(tool_calls) = response.get("tool_calls").and_then(|v| v.as_array()) {
                if tool_calls.is_empty() {
                    // No actual tool calls, treat as Answer
                    let content = response["content"].as_str().unwrap_or_default().to_string();
                    return Ok(AgentResponse::Answer(content));
                }

                let mut calls = Vec::new();
                for tc in tool_calls {
                    calls.push(ToolCall {
                        id: tc["id"].as_str().unwrap_or_default().to_string(),
                        name: tc["function"]["name"].as_str().unwrap_or_default().to_string(),
                        arguments: tc["function"]["arguments"].as_str().unwrap_or_default().to_string(),
                    });
                }

                // Build tool_calls JSON for the assistant message
                let tool_calls_json: Vec<Value> = calls.iter().map(|c| {
                    serde_json::json!({
                        "id": c.id,
                        "type": "function",
                        "function": {
                            "name": c.name,
                            "arguments": c.arguments
                        }
                    })
                }).collect();

                // Preserve any content that came alongside tool_calls (thinking text)
                let assistant_content = response["content"].as_str().unwrap_or_default().to_string();

                // Add the assistant message (with content + tool_calls) to history
                context.add_message(crate::memory::types::ChatMessage {
                    role: "assistant".to_string(),
                    content: assistant_content,
                    tool_calls: Some(tool_calls_json),
                    tool_call_id: None,
                });

                // Execute each tool and add tool result messages
                for call in &calls {
                    if let Some(tool) = self.tools.iter().find(|t| t.name() == call.name) {
                        let args: Value = serde_json::from_str(&call.arguments)
                            .unwrap_or(serde_json::json!({}));
                        let result = match tool.call(args).await {
                            Ok(res) => res.to_string(),
                            Err(e) => format!("Error: {}", e),
                        };
                        context.add_message(crate::memory::types::ChatMessage {
                            role: "tool".to_string(),
                            content: result,
                            tool_calls: None,
                            tool_call_id: Some(call.id.clone()),
                        });
                    } else {
                        context.add_message(crate::memory::types::ChatMessage {
                            role: "tool".to_string(),
                            content: format!("Error: Tool '{}' not found", call.name),
                            tool_calls: None,
                            tool_call_id: Some(call.id.clone()),
                        });
                    }
                }

                if max_inner_steps == 0 {
                    return Ok(AgentResponse::Answer(
                        "Reached maximum inner tool call steps.".to_string(),
                    ));
                }

                // Continue the inner loop: LLM will now see the tool results
                continue;
            }

            // No tool calls — this is an Answer
            let content = response["content"].as_str().unwrap_or_default().to_string();

            // IMPORTANT: Add the assistant answer to history so the next outer-step sees it
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
