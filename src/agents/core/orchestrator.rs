use std::sync::Arc;
use anyhow::Result;
use serde_json::Value;

use super::{Agent, AgentContext, AgentResponse, ToolCall};

#[derive(Debug, Clone)]
pub enum OrchestratorEvent {
    /// Coordinator is generating a plan
    Plan { plan: String },
    StepStarted { agent_id: String, agent_name: String },
    StepFinished { result: String },
    ToolCall { name: String, args: String },
    ToolResult { name: String, result: String },
}

pub struct Orchestrator {
    coordinator: Arc<dyn Agent>,
    sub_agents: std::collections::HashMap<String, Arc<dyn Agent>>,
}

impl Orchestrator {
    pub fn new(
        coordinator: Arc<dyn Agent>,
        sub_agents: std::collections::HashMap<String, Arc<dyn Agent>>,
    ) -> Self {
        Self {
            coordinator,
            sub_agents,
        }
    }

    pub async fn run_task<F>(&self, task: &str, session_id: String, mut on_event: F) -> Result<String>
    where F: FnMut(OrchestratorEvent) + Send
    {
        let mut context = AgentContext::new(session_id);
        context.add_message(crate::memory::types::ChatMessage::new("user", task));

        let mut max_steps = 15;
        while max_steps > 0 {
            max_steps -= 1;

            on_event(OrchestratorEvent::StepStarted {
                agent_id: self.coordinator.id().to_string(),
                agent_name: self.coordinator.name().to_string(),
            });

            let response = self.coordinator.step(&mut context).await?;

            match response {
                AgentResponse::Answer(answer) => {
                    on_event(OrchestratorEvent::StepFinished { result: answer.clone() });
                    return Ok(answer);
                }
                AgentResponse::ToolCalls(calls, thinking) => {
                    if !thinking.is_empty() {
                        on_event(OrchestratorEvent::Plan { plan: thinking });
                    }
                    self.execute_tool_calls_and_feed_back(&mut context, &calls, &mut on_event).await?;
                }
            }
        }

        Ok("Reached maximum execution steps.".to_string())
    }

    /// Execute a list of tool calls, feed results back into context.
    /// Intercepts "delegate" calls to dispatch to sub-agents.
    async fn execute_tool_calls_and_feed_back<F>(
        &self,
        context: &mut AgentContext,
        calls: &[ToolCall],
        on_event: &mut F,
    ) -> Result<()>
    where F: FnMut(OrchestratorEvent) + Send
    {
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

        context.add_message(crate::memory::types::ChatMessage {
            role: "assistant".to_string(),
            content: String::new(),
            tool_calls: Some(tool_calls_json),
            tool_call_id: None,
        });

        for call in calls {
            on_event(OrchestratorEvent::ToolCall {
                name: call.name.clone(),
                args: call.arguments.clone(),
            });

            let result = if call.name == "delegate" {
                let args: Value = serde_json::from_str(&call.arguments)
                    .unwrap_or(Value::Null);
                let agent_id = args["agent_id"].as_str().unwrap_or_default();
                let sub_task = args["task"].as_str().unwrap_or_default();

                if let Some(agent) = self.sub_agents.get(agent_id) {
                    self.run_sub_agent(agent.clone(), sub_task, &context.session_id, on_event).await?
                } else {
                    format!("Error: Agent '{}' not found", agent_id)
                }
            } else if let Some(tool) = self.coordinator.tools().iter().find(|t| t.name() == call.name) {
                let args: Value = serde_json::from_str(&call.arguments)
                    .unwrap_or(Value::Null);
                match tool.call(args).await {
                    Ok(res) => res.to_string(),
                    Err(e) => format!("Error: {}", e),
                }
            } else {
                format!("Error: Tool '{}' not found", call.name)
            };

            on_event(OrchestratorEvent::ToolResult {
                name: call.name.clone(),
                result: result.clone(),
            });

            context.add_message(crate::memory::types::ChatMessage {
                role: "tool".to_string(),
                content: result,
                tool_calls: None,
                tool_call_id: Some(call.id.clone()),
            });
        }

        Ok(())
    }

    async fn run_sub_agent<F>(&self, agent: Arc<dyn Agent>, task: &str, session_id: &str, on_event: &mut F) -> Result<String>
    where F: FnMut(OrchestratorEvent) + Send
    {
        let mut context = AgentContext::new(format!("{}-{}", session_id, agent.id()));
        context.add_message(crate::memory::types::ChatMessage::new("user", task));

        on_event(OrchestratorEvent::StepStarted {
            agent_id: agent.id().to_string(),
            agent_name: agent.name().to_string()
        });

        let response = agent.step(&mut context).await?;
        match response {
            AgentResponse::Answer(answer) => {
                on_event(OrchestratorEvent::StepFinished { result: answer.clone() });
                Ok(answer)
            }
            AgentResponse::ToolCalls(_, _) => {
                // step_with_tools should never return ToolCalls; it loops internally.
                // If it does, just use the context history as fallback answer
                let fallback = "Sub-agent did not produce an answer.".to_string();
                on_event(OrchestratorEvent::StepFinished { result: fallback.clone() });
                Ok(fallback)
            }
        }
    }
}
