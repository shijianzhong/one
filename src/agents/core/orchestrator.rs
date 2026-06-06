use std::sync::Arc;
use anyhow::Result;
use serde_json::Value;

use super::{Agent, AgentContext, AgentResponse, ToolCall};
use crate::agents::claude_code::ClaudeStreamEvent;
use crate::memory::types::ChatMessage;

#[derive(Debug, Clone)]
pub enum OrchestratorEvent {
    /// MainAgent is generating a plan
    Plan { plan: String },
    /// Real-time stream delta from the main assistant
    AssistantDelta(String),
    StepStarted { agent_id: String, agent_name: String },
    StepFinished { result: String },
    ToolCall { name: String, args: String },
    ToolResult { name: String, result: String },
    /// Real-time stream delta from a sub-agent (maps to a subagent card in UI)
    SubAgentStream { agent_id: String, event: ClaudeStreamEvent },
}

pub struct Orchestrator {
    main_agent: Arc<dyn Agent>,
    sub_agents: std::collections::HashMap<String, Arc<dyn Agent>>,
}

impl Orchestrator {
    pub fn new(
        main_agent: Arc<dyn Agent>,
        sub_agents: std::collections::HashMap<String, Arc<dyn Agent>>,
    ) -> Self {
        Self {
            main_agent,
            sub_agents,
        }
    }

    pub async fn run_task<F>(
        &self,
        task: &str,
        session_id: String,
        history: Vec<ChatMessage>,
        mut on_event: F,
    ) -> Result<String>
    where
        F: FnMut(OrchestratorEvent) + Send,
    {
        let mut context = AgentContext::new(session_id);
        // 先加载历史消息（除最后一条 user 消息外，避免重复）
        let msg_count = history.len();
        for msg in history.into_iter().take(msg_count.saturating_sub(1)) {
            context.add_message(msg);
        }
        context.add_message(crate::memory::types::ChatMessage::new("user", task));

        let mut max_steps = 15;
        while max_steps > 0 {
            max_steps -= 1;

            // StepStarted 仅在 run_sub_agent 中发送（真正启动子代理时）
            // 主循环不发送，避免简单对话也创建空白的 subagent 卡片

            let (delta_tx, mut delta_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
            let main_agent = self.main_agent.clone();
            
            // We need to capture the updated history from the inner step.
            // Let's modify AgentResponse or the return type to include context,
            // or just perform the step and return (Response, updated_history).
            
            // For now, let's just re-apply the response to our main context.
            // In a real implementation, step_stream should probably take &mut context.
            // To do that in a spawn, we'd need Arc<Mutex<AgentContext>>.
            
            let mut context_inner = AgentContext::new(context.session_id.clone());
            context_inner.history = context.history.clone();
            context_inner.metadata = context.metadata.clone();

            let response = {
                let mut step_fut = Box::pin(main_agent.step_stream(&mut context_inner, Box::new(move |delta| {
                    let _ = delta_tx.send(delta);
                })));

                loop {
                    tokio::select! {
                        res = &mut step_fut => break res,
                        Some(delta) = delta_rx.recv() => {
                            on_event(OrchestratorEvent::AssistantDelta(delta));
                        }
                    }
                }
            }?;

            // Sync history back: Assistant and Tool results are added to context in execute_tool_calls_and_feed_back
            // But we need to make sure the Assistant message from the LLM is added.
            // Actually, BaseAgent::call_llm_stream DOES NOT add the message to history.
            // BaseAgent::step_with_tools DOES. 
            // MainAgent::step_stream calls call_llm_stream.
            
            match response {
                AgentResponse::Answer(answer) => {
                    // Add the assistant's answer to history
                    context.add_message(crate::memory::types::ChatMessage {
                        role: "assistant".to_string(),
                        content: answer.clone(),
                        tool_calls: None,
                        tool_call_id: None,
                    });
                    on_event(OrchestratorEvent::StepFinished { result: answer.clone() });
                    return Ok(answer);
                }
                AgentResponse::ToolCalls(calls, thinking) => {
                    if !thinking.is_empty() {
                        on_event(OrchestratorEvent::Plan { plan: thinking });
                    }
                    self.execute_tool_calls_and_feed_back(
                        &mut context,
                        &calls,
                        &mut on_event,
                    )
                    .await?;
                }
            }
        }

        Ok("Reached maximum execution steps.".to_string())
    }

    /// Execute a list of tool calls and feed results back into context.
    /// Intercepts MainAgent tool calls that dispatch to specialized sub-agents.
    async fn execute_tool_calls_and_feed_back<F>(
        &self,
        context: &mut AgentContext,
        calls: &[ToolCall],
        on_event: &mut F,
    ) -> Result<()>
    where
        F: FnMut(OrchestratorEvent) + Send,
    {
        let tool_calls_json: Vec<Value> = calls
            .iter()
            .map(|c| {
                serde_json::json!({
                    "id": c.id,
                    "type": "function",
                    "function": {
                        "name": c.name,
                        "arguments": c.arguments
                    }
                })
            })
            .collect();

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

            let result = match call.name.as_str() {
                "run_claude_code" => {
                    let args: Value = serde_json::from_str(&call.arguments).unwrap_or(Value::Null);
                    let instruction = args["instruction"].as_str().unwrap_or_default();
                    if let Some(agent) = self.sub_agents.get("coding") {
                        self.run_sub_agent(
                            agent.clone(),
                            instruction,
                            &context.session_id,
                            on_event,
                        ).await?
                    } else {
                        "Error: Coding agent not found".to_string()
                    }
                }
                "run_system_task" => {
                    let args: Value = serde_json::from_str(&call.arguments).unwrap_or(Value::Null);
                    let skill_id = args
                        .get("skill_id")
                        .and_then(|v| v.as_str())
                        .map(|s| s.trim())
                        .filter(|s| !s.is_empty());
                    if let Some(skill_id) = skill_id {
                        if let Some(skill) = crate::skills::registry().find(skill_id) {
                            let apply = args
                                .get("apply")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false);
                            let skill_args = args
                                .get("args")
                                .cloned()
                                .unwrap_or(Value::Object(Default::default()));
                            if apply {
                                match skill.execute(skill_args, None).await {
                                    Ok(exec) => serde_json::json!({
                                        "stage": "execute",
                                        "skill_id": skill_id,
                                        "denied": exec.denied,
                                        "summary": exec.summary,
                                        "freed_bytes": exec.freed_bytes,
                                        "success": exec.success_items,
                                        "failed": exec.failed_items
                                            .iter()
                                            .map(|(k, v)| serde_json::json!({"item": k, "error": v}))
                                            .collect::<Vec<_>>(),
                                    })
                                    .to_string(),
                                    Err(err) => format!("Error: skill execute failed: {}", err),
                                }
                            } else {
                                match skill.preview(skill_args).await {
                                    Ok(preview) => serde_json::json!({
                                        "stage": "preview",
                                        "skill_id": skill_id,
                                        "summary": preview.summary,
                                        "estimated_bytes": preview.estimated_bytes,
                                        "items": preview.items
                                            .iter()
                                            .map(|it| serde_json::json!({
                                                "label": it.label,
                                                "detail": it.detail,
                                                "bytes": it.bytes,
                                            }))
                                            .collect::<Vec<_>>(),
                                        "warnings": preview.warnings,
                                        "hint": "若用户同意继续，再次调用 run_system_task 时把 apply 设为 true。",
                                    })
                                    .to_string(),
                                    Err(err) => format!("Error: skill preview failed: {}", err),
                                }
                            }
                        } else {
                            let known: Vec<String> = crate::skills::registry()
                                .manifests()
                                .into_iter()
                                .map(|m| m.id)
                                .collect();
                            format!(
                                "Error: skill_id '{}' not found. Available skills: {:?}",
                                skill_id, known
                            )
                        }
                    } else {
                        let sub_task = args
                            .get("task")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default();
                        if let Some(agent) = self.sub_agents.get("system") {
                            self.run_sub_agent(
                                agent.clone(),
                                sub_task,
                                &context.session_id,
                                on_event,
                            ).await?
                        } else {
                            "Error: System agent not found".to_string()
                        }
                    }
                }
                // Generic tool execution for tools owned by the MainAgent itself (e.g. remember, update_soul)
                _ => {
                    if let Some(tool) = self.main_agent.tools().iter().find(|t| t.name() == call.name) {
                        let args: Value = serde_json::from_str(&call.arguments).unwrap_or(Value::Null);
                        match tool.call(args).await {
                            Ok(res) => res.to_string(),
                            Err(e) => format!("Error: {}", e),
                        }
                    } else {
                        format!("Error: Tool '{}' not found", call.name)
                    }
                }
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

    async fn run_sub_agent<F>(
        &self,
        agent: Arc<dyn Agent>,
        task: &str,
        session_id: &str,
        on_event: &mut F,
    ) -> Result<String>
    where
        F: FnMut(OrchestratorEvent) + Send,
    {
        let agent_id = agent.id().to_string();
        let agent_name = agent.name().to_string();

        on_event(OrchestratorEvent::StepStarted {
            agent_id: agent_id.clone(),
            agent_name: agent_name.clone(),
        });

        // ── Coding agent: stream Claude Code output in real time ──────────────
        if agent_id == "coding" {
            use std::sync::mpsc;
            let (tx, rx) = mpsc::channel::<ClaudeStreamEvent>();
            let tx2 = tx.clone(); // keep sender alive until we drop it
            let task_owned = task.to_string();
            let project_dir = std::path::PathBuf::from(".");

            let handle = std::thread::spawn(move || {
                crate::agents::claude_code::ClaudeCodeAgent::execute_instruction_stream(
                    &project_dir,
                    &task_owned,
                    None,
                    tx,
                )
            });

            let mut final_result = String::new();

            loop {
                match rx.recv_timeout(std::time::Duration::from_millis(100)) {
                    Ok(event) => {
                        let is_terminal = matches!(
                            event,
                            ClaudeStreamEvent::Finished { .. } | ClaudeStreamEvent::Failed { .. }
                        );
                        if let ClaudeStreamEvent::Finished { ref result } = event {
                            final_result = result.clone();
                        } else if let ClaudeStreamEvent::Failed { ref error } = event {
                            final_result = format!("Error: {}", error);
                        }
                        on_event(OrchestratorEvent::SubAgentStream {
                            agent_id: agent_id.clone(),
                            event,
                        });
                        if is_terminal {
                            break;
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        if handle.is_finished() {
                            break;
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }

            // drain any remaining events
            drop(tx2);
            while let Ok(event) = rx.try_recv() {
                if let ClaudeStreamEvent::Finished { ref result } = event {
                    if final_result.is_empty() {
                        final_result = result.clone();
                    }
                }
                on_event(OrchestratorEvent::SubAgentStream {
                    agent_id: agent_id.clone(),
                    event,
                });
            }

            let _ = handle.join();
            on_event(OrchestratorEvent::StepFinished { result: final_result.clone() });
            return Ok(final_result);
        }

        // ── Other sub-agents: generic single-step ─────────────────────────────
        let mut context =
            AgentContext::new(format!("{}-{}", session_id, agent_id));
        context.add_message(crate::memory::types::ChatMessage::new("user", task));

        let response = agent.step(&mut context).await?;
        match response {
            AgentResponse::Answer(answer) => {
                on_event(OrchestratorEvent::StepFinished { result: answer.clone() });
                Ok(answer)
            }
            AgentResponse::ToolCalls(_, _) => {
                let fallback = "Sub-agent did not produce an answer.".to_string();
                on_event(OrchestratorEvent::StepFinished { result: fallback.clone() });
                Ok(fallback)
            }
        }
    }
}
