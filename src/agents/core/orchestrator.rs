use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use anyhow::Result;
use serde_json::Value;

use super::{Agent, AgentContext, AgentResponse, ToolCall};
use crate::memory::types::ChatMessage;

#[derive(Debug, Clone)]
pub enum OrchestratorEvent {
    /// MainAgent is generating a plan / thinking text
    Plan { plan: String },
    /// Real-time stream delta from the main assistant
    AssistantDelta(String),
    /// A sub-step has started (kept for UI compatibility, but no longer used for sub-agents)
    StepStarted { agent_id: String, agent_name: String },
    /// A step has finished
    StepFinished { result: String },
    /// A tool has been called
    ToolCall { name: String, args: String },
    /// A tool returned a result
    ToolResult { name: String, result: String },
    /// Orchestrator is waiting for the user's next message (multi-turn)
    AwaitingUserInput { reply: String },
}

pub struct Orchestrator {
    main_agent: Arc<dyn Agent>,
}

impl Orchestrator {
    pub fn new(
        main_agent: Arc<dyn Agent>,
        _work_dir: std::path::PathBuf,
    ) -> Self {
        Self {
            main_agent,
        }
    }

    pub async fn run_task<F>(
        &self,
        task: &str,
        session_id: String,
        history: Vec<ChatMessage>,
        workspace: &str,
        task_id: Option<usize>,
        cancel_flag: Option<Arc<AtomicBool>>,
        mut user_input_rx: Option<tokio::sync::mpsc::UnboundedReceiver<String>>,
        mut on_event: F,
    ) -> Result<String>
    where
        F: FnMut(OrchestratorEvent) + Send,
    {
        let mut context = AgentContext::new(session_id);

        // ── 取消检查 ─────────────────────────────────────────────────
        if cancel_flag.as_ref().map(|f| f.load(Ordering::SeqCst)).unwrap_or(false) {
            return Ok("任务已被用户取消。".to_string());
        }

        // ── 主动注入记忆 ──────────────────────────────────────────────
        let mut all_facts = crate::memory::profile::get_global_facts();
        all_facts.extend(crate::memory::profile::get_all_facts(workspace));
        let set: std::collections::HashSet<String> = all_facts.into_iter().collect();
        let mut unique_facts: Vec<String> = set.into_iter().collect();
        unique_facts.sort();

        if !unique_facts.is_empty() {
            let memory_hint = format!(
                "### User Profile & Project Context\n{}",
                unique_facts.iter().map(|f| format!("- {}", f)).collect::<Vec<_>>().join("\n")
            );
            context.add_message(ChatMessage::new("system", &memory_hint));
        }

        // ── L3 相关历史上下文 ─────────────────────────────────────────
        let l3_context = crate::memory::snapshot::build_memory_context(
            workspace,
            task_id.unwrap_or(0),
            task,
        );
        if !l3_context.is_empty() {
            context.add_message(ChatMessage::new("system", &l3_context));
        }

        // ── 历史消息（去掉最后一条 user，避免重复） ──────────────────
        let msg_count = history.len();
        for msg in history.into_iter().take(msg_count.saturating_sub(1)) {
            context.add_message(msg);
        }
        context.add_message(ChatMessage::new("user", task));

        let mut max_steps = 15;
        while max_steps > 0 {
            max_steps -= 1;

            if cancel_flag.as_ref().map(|f| f.load(Ordering::SeqCst)).unwrap_or(false) {
                return Ok("任务已被用户取消。".to_string());
            }

            // ── 流式调用 MainAgent ────────────────────────────────────
            let (delta_tx, mut delta_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
            let main_agent = self.main_agent.clone();

            let mut context_inner = AgentContext::new(context.session_id.clone());
            context_inner.history = context.history.clone();
            context_inner.metadata = context.metadata.clone();

            let response = {
                let mut step_fut = Box::pin(main_agent.step_stream(
                    &mut context_inner,
                    Box::new(move |delta| { let _ = delta_tx.send(delta); }),
                ));
                loop {
                    tokio::select! {
                        res = &mut step_fut => break res,
                        Some(delta) = delta_rx.recv() => {
                            on_event(OrchestratorEvent::AssistantDelta(delta));
                        }
                    }
                }
            }?;

            match response {
                AgentResponse::Answer(answer) => {
                    context.add_message(ChatMessage {
                        role: "assistant".to_string(),
                        content: answer.clone(),
                        tool_calls: None,
                        tool_call_id: None,
                    });

                    if let Some(ref mut input_rx) = user_input_rx {
                        on_event(OrchestratorEvent::AwaitingUserInput { reply: answer.clone() });
                        match input_rx.recv().await {
                            Some(user_msg) => {
                                context.add_message(ChatMessage::new("user", &user_msg));
                                continue;
                            }
                            None => {
                                // 用户输入通道关闭（可能是切换 task 或启动了新对话），
                                // 返回空字符串，因为 reply 已通过 AwaitingUserInput 事件处理，
                                // Finished handler 收到空结果时不会重复写入。
                                return Ok(String::new());
                            }
                        }
                    } else {
                        on_event(OrchestratorEvent::StepFinished { result: answer.clone() });
                        return Ok(answer);
                    }
                }

                AgentResponse::ToolCalls(calls, thinking) => {
                    if !thinking.is_empty() {
                        on_event(OrchestratorEvent::Plan { plan: thinking });
                    }
                    self.execute_tool_calls(&mut context, &calls, cancel_flag.clone(), &mut on_event)
                        .await?;
                }
            }
        }

        Ok("Reached maximum execution steps.".to_string())
    }

    /// Execute tool calls from MainAgent and feed results back into context.
    async fn execute_tool_calls<F>(
        &self,
        context: &mut AgentContext,
        calls: &[ToolCall],
        cancel_flag: Option<Arc<AtomicBool>>,
        on_event: &mut F,
    ) -> Result<()>
    where
        F: FnMut(OrchestratorEvent) + Send,
    {
        let tool_calls_json: Vec<Value> = calls.iter().map(|c| serde_json::json!({
            "id": c.id,
            "type": "function",
            "function": { "name": c.name, "arguments": c.arguments }
        })).collect();

        context.add_message(ChatMessage {
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

            let result = self.dispatch_tool(call, cancel_flag.clone(), on_event).await;

            on_event(OrchestratorEvent::ToolResult {
                name: call.name.clone(),
                result: result.clone(),
            });

            context.add_message(ChatMessage {
                role: "tool".to_string(),
                content: result,
                tool_calls: None,
                tool_call_id: Some(call.id.clone()),
            });
        }

        Ok(())
    }

    /// Dispatch a single tool call. Intercepts special tools; falls back to MainAgent tools.
    async fn dispatch_tool<F>(
        &self,
        call: &ToolCall,
        _cancel_flag: Option<Arc<AtomicBool>>,
        _on_event: &mut F,
    ) -> String
    where
        F: FnMut(OrchestratorEvent) + Send,
    {
        let args: Value = serde_json::from_str(&call.arguments).unwrap_or(Value::Null);

        match call.name.as_str() {
            // ── Skill dispatch ───────────────────────────────────────
            "run_system_task" => {
                let skill_id = args.get("skill_id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty());

                if let Some(skill_id) = skill_id {
                    if let Some(skill) = crate::skills::registry().find(skill_id) {
                        let apply = args.get("apply").and_then(|v| v.as_bool()).unwrap_or(false);
                        let skill_args = args.get("args").cloned().unwrap_or(Value::Object(Default::default()));

                        if apply {
                            match skill.execute(skill_args, None).await {
                                Ok(exec) => {
                                    // 对于 system.tools 直接返回总结内容，避免 JSON 嵌套让 LLM 困惑
                                    if skill_id == "system.tools" {
                                        exec.summary
                                    } else {
                                        serde_json::json!({
                                            "stage": "execute",
                                            "skill_id": skill_id,
                                            "denied": exec.denied,
                                            "summary": exec.summary,
                                            "freed_bytes": exec.freed_bytes,
                                            "success": exec.success_items,
                                            "failed": exec.failed_items.iter()
                                                .map(|(k, v)| serde_json::json!({"item": k, "error": v}))
                                                .collect::<Vec<_>>(),
                                        }).to_string()
                                    }
                                }
                                Err(e) => format!("Error: skill execute failed: {}", e),
                            }
                        } else {
                            match skill.preview(skill_args).await {
                                Ok(preview) => serde_json::json!({
                                    "stage": "preview",
                                    "skill_id": skill_id,
                                    "summary": preview.summary,
                                    "estimated_bytes": preview.estimated_bytes,
                                    "items": preview.items.iter().map(|it| serde_json::json!({
                                        "label": it.label,
                                        "detail": it.detail,
                                        "bytes": it.bytes,
                                    })).collect::<Vec<_>>(),
                                    "warnings": preview.warnings,
                                    "hint": "若用户同意继续，再次调用 run_system_task 时把 apply 设为 true。",
                                }).to_string(),
                                Err(e) => format!("Error: skill preview failed: {}", e),
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
                    // 没有 skill_id：提示用户使用具体的 Skill
                    let known: Vec<String> = crate::skills::registry()
                        .manifests()
                        .into_iter()
                        .map(|m| m.id)
                        .collect();
                    format!(
                        "请通过 skill_id 参数指定要使用的 Skill。当前已安装：{:?}",
                        known
                    )
                }
            }

            // ── 已废弃工具的友好提示 ─────────────────────────────────
            "run_claude_code" => {
                "run_claude_code 工具已移除。编码能力将通过 Skill Market 中的 coding_assistant Skill 提供，安装后通过 run_system_task(skill_id=\"coding_assistant\") 调用。".to_string()
            }

            // ── 其他工具：交给 MainAgent 自身处理 ────────────────────
            _ => {
                if let Some(tool) = self.main_agent.tools().iter().find(|t| t.name() == call.name) {
                    match tool.call(args).await {
                        Ok(res) => res.to_string(),
                        Err(e) => format!("Error: {}", e),
                    }
                } else {
                    format!("Error: Tool '{}' not found", call.name)
                }
            }
        }
    }
}
