use anyhow::Result;
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use super::{AgentResponse, AgentRunContext, AgentTrait, ToolCall};
use crate::agents::permission::{classify_mcp_tool_kind, PermissionDecision};
use crate::mcp::McpClientManager;
use crate::memory::types::ChatMessage;
use crate::skills::Skill;

#[derive(Debug, Clone)]
pub enum OrchestratorEvent {
    /// MainAgent is generating a plan / thinking text
    Plan { plan: String },
    /// Real-time stream delta from the main assistant
    AssistantDelta(String),
    /// A sub-step has started (kept for UI compatibility, but no longer used for sub-agents)
    StepStarted {
        agent_id: String,
        agent_name: String,
    },
    /// A step has finished
    StepFinished { result: String },
    /// A tool has been called
    ToolCall { name: String, args: String },
    /// A tool returned a result
    ToolResult { name: String, result: String },
    /// Orchestrator is waiting for the user's next message (multi-turn)
    AwaitingUserInput { reply: String },
    /// Agent wants to run a command in the terminal
    RunInTerminal { command: String, work_dir: String },
    /// Agent identified a coding task and wants runtime to start the two-stage Claude Code workflow.
    CodingWorkflowRequested {
        user_request: String,
        main_agent_summary: String,
        known_constraints: Vec<String>,
        suggested_direction: Option<String>,
        clarification_focus: Vec<String>,
    },
}

pub struct Orchestrator {
    main_agent: Arc<dyn AgentTrait>,
    mcp_manager: Option<Arc<std::sync::Mutex<McpClientManager>>>,
}

impl Orchestrator {
    pub fn new(
        main_agent: Arc<dyn AgentTrait>,
        _work_dir: std::path::PathBuf,
        mcp_manager: Option<Arc<std::sync::Mutex<McpClientManager>>>,
    ) -> Self {
        Self {
            main_agent,
            mcp_manager,
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
        let mut context = AgentRunContext::new(session_id);
        context.cancel_flag = cancel_flag.clone();
        context.user_input_rx = user_input_rx.take();
        self.refresh_agent_tools(&mut context);

        // ── 取消检查 ─────────────────────────────────────────────────
        if cancel_flag
            .as_ref()
            .map(|f| f.load(Ordering::SeqCst))
            .unwrap_or(false)
        {
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
                unique_facts
                    .iter()
                    .map(|f| format!("- {}", f))
                    .collect::<Vec<_>>()
                    .join("\n")
            );
            context.add_message(ChatMessage::new("system", &memory_hint));
        }

        // ── L3 相关历史上下文 ─────────────────────────────────────────
        let l3_context =
            crate::memory::snapshot::build_memory_context(workspace, task_id.unwrap_or(0), task);
        if !l3_context.is_empty() {
            context.add_message(ChatMessage::new("system", &l3_context));
        }

        // ── 已安装 Skill 信息注入 ────────────────────────────────────
        let skill_info: Vec<String> = crate::skills::registry()
            .manifests()
            .into_iter()
            .map(|m| {
                format!(
                    "- **{}**: {}。调用方式：`run_system_task(skill_id=\"{}\", apply=true)` 获取详细使用说明。",
                    m.name, m.description, m.id
                )
            })
            .collect();

        if !skill_info.is_empty() {
            let skill_context = format!(
                "### 已安装的 Skill\n以下 Skill 当前已安装：\n{}\n\n先调用 `run_system_task` 查看 Skill 的使用说明，再按说明执行。",
                skill_info.join("\n")
            );
            context.add_message(ChatMessage::new("system", &skill_context));
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

            if cancel_flag
                .as_ref()
                .map(|f| f.load(Ordering::SeqCst))
                .unwrap_or(false)
            {
                return Ok("任务已被用户取消。".to_string());
            }

            // ── 流式调用 MainAgent ────────────────────────────────────
            let (delta_tx, mut delta_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
            let main_agent = self.main_agent.clone();

            let mut context_inner = AgentRunContext::new(context.session_id.clone());
            context_inner.history = context.history.clone();
            context_inner.metadata = context.metadata.clone();
            context_inner.cancel_flag = context.cancel_flag.clone();
            self.refresh_agent_tools(&mut context_inner);

            let response = {
                let mut step_fut = Box::pin(main_agent.step_stream(
                    &mut context_inner,
                    Box::new(move |delta| {
                        let _ = delta_tx.send(delta);
                    }),
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

                    if let Some(ref mut input_rx) = context.user_input_rx {
                        on_event(OrchestratorEvent::AwaitingUserInput {
                            reply: answer.clone(),
                        });
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
                        on_event(OrchestratorEvent::StepFinished {
                            result: answer.clone(),
                        });
                        return Ok(answer);
                    }
                }

                AgentResponse::ToolCalls(calls, thinking) => {
                    if !thinking.is_empty() {
                        on_event(OrchestratorEvent::Plan { plan: thinking });
                    }
                    self.execute_tool_calls(
                        &mut context,
                        &calls,
                        cancel_flag.clone(),
                        &mut on_event,
                    )
                    .await?;
                }
            }
        }

        Ok("Reached maximum execution steps.".to_string())
    }

    /// Execute tool calls from MainAgent and feed results back into context.
    async fn execute_tool_calls<F>(
        &self,
        context: &mut AgentRunContext,
        calls: &[ToolCall],
        cancel_flag: Option<Arc<AtomicBool>>,
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
                    "function": { "name": c.name, "arguments": c.arguments }
                })
            })
            .collect();

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

            let result = self
                .dispatch_tool(call, cancel_flag.clone(), on_event)
                .await;

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

    fn refresh_agent_tools(&self, context: &mut AgentRunContext) {
        let filter = self.main_agent.tool_filter();
        if let Ok(registry) = crate::agents::core::tool_registry::tool_registry().lock() {
            context.tool_sources = registry.tool_sources(filter.as_deref());
            context.tool_definitions = registry.tool_definitions(filter.as_deref());
        }
    }

    /// Dispatch a single tool call. Routes through ToolRegistry for unified dispatch.
    /// Builtin tools → direct execution
    /// Skill tools (skill: prefix) → SkillRegistry
    /// MCP tools (mcp: prefix) → McpClientManager
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
        let name = &call.name;

        // Runtime bridge tools are exposed through ToolRegistry but executed
        // here because they produce UI/runtime events rather than direct values.
        if name == "start_coding_workflow" {
            let user_request = args
                .get("user_request")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let main_agent_summary = args
                .get("main_agent_summary")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let known_constraints = string_array_arg(&args, "known_constraints");
            let suggested_direction = args
                .get("suggested_direction")
                .and_then(|v| v.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
            let clarification_focus = string_array_arg(&args, "clarification_focus");

            if user_request.trim().is_empty() {
                return "请提供 user_request。".to_string();
            }

            _on_event(OrchestratorEvent::CodingWorkflowRequested {
                user_request,
                main_agent_summary,
                known_constraints,
                suggested_direction,
                clarification_focus,
            });
            return "编码工作流已启动。Claude Code 会先做方案梳理，完成后等待用户确认再执行编码。"
                .to_string();
        }

        if name == "run_in_terminal" {
            let command = args
                .get("command")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            if !command.is_empty() {
                let work_dir = args
                    .get("work_dir")
                    .and_then(|v| v.as_str())
                    .unwrap_or(".")
                    .to_string();
                _on_event(OrchestratorEvent::RunInTerminal { command, work_dir });
                return "命令已发送到终端执行。用户可以在终端中查看实时输出。".to_string();
            }
            return "请提供要执行的命令。".to_string();
        }

        if name == "run_system_task" {
            let skill_id = args
                .get("skill_id")
                .and_then(|v| v.as_str())
                .map(|s| s.trim())
                .filter(|s| !s.is_empty());

            if let Some(sid) = skill_id {
                return self.execute_skill(sid, args.clone()).await;
            } else {
                let known: Vec<String> = crate::skills::registry()
                    .manifests()
                    .into_iter()
                    .map(|m| m.id)
                    .collect();
                return format!("请在 skill_id 参数中指定 Skill。当前已安装：{:?}", known);
            }
        }

        // ── 1. 尝试通过 ToolRegistry 路由已知工具名称 ──────────────
        if let Some(source) = crate::agents::core::tool_registry::tool_registry()
            .lock()
            .ok()
            .and_then(|reg| reg.resolve_tool_source(name))
        {
            match source {
                crate::agents::core::agent::ToolSource::Skill(skill_id) => {
                    return self.execute_skill(&skill_id, args).await;
                }
                crate::agents::core::agent::ToolSource::Mcp { server, tool_name } => {
                    let detail = format!("MCP {}/{} {}", server, tool_name, args);
                    let kind = classify_mcp_tool_kind(&tool_name);
                    match crate::agents::permission::global()
                        .request_async(kind, detail, None)
                        .await
                    {
                        PermissionDecision::Allow => {}
                        PermissionDecision::Deny(reason) => {
                            return format!("MCP 调用已拒绝：{}", reason);
                        }
                        PermissionDecision::Ask => {
                            return "MCP 调用未获得授权。".to_string();
                        }
                    }
                    if let Some(mcp) = &self.mcp_manager {
                        match crate::mcp::call_tool_async(
                            mcp.clone(),
                            server.clone(),
                            tool_name.clone(),
                            args,
                        )
                        .await
                        {
                            Ok(result) => return result,
                            Err(e) => return format!("MCP Error: {}", e),
                        }
                    }
                    return format!("MCP server '{}' not connected", server);
                }
                crate::agents::core::agent::ToolSource::Builtin(tool) => {
                    // 内置工具直接调用
                    match tool.call(args).await {
                        Ok(res) => return res.to_string(),
                        Err(e) => return format!("Error: {}", e),
                    }
                }
            }
        }

        format!("Error: Tool '{}' not found", name)
    }

    /// 执行 Skill（支持 skill: 前缀匹配）
    async fn execute_skill(&self, skill_id: &str, args: Value) -> String {
        let sid = skill_id.strip_prefix("skill:").unwrap_or(skill_id);
        let apply = args.get("apply").and_then(|v| v.as_bool()).unwrap_or(false);
        let skill_args = args
            .get("args")
            .cloned()
            .unwrap_or(Value::Object(Default::default()));

        if let Some(skill) = crate::skills::registry().find(sid) {
            if apply {
                match skill.execute(skill_args, None).await {
                    Ok(exec) => {
                        if sid == "system.tools" {
                            exec.summary
                        } else {
                            serde_json::json!({
                                "stage": "execute",
                                "skill_id": sid,
                                "denied": exec.denied,
                                "summary": exec.summary,
                                "freed_bytes": exec.freed_bytes,
                                "success": exec.success_items,
                                "failed": exec.failed_items.iter()
                                    .map(|(k, v)| serde_json::json!({"item": k, "error": v}))
                                    .collect::<Vec<_>>(),
                            })
                            .to_string()
                        }
                    }
                    Err(e) => format!("Error: skill execute failed: {}", e),
                }
            } else {
                match skill.preview(skill_args).await {
                    Ok(preview) => serde_json::json!({
                        "stage": "preview",
                        "skill_id": sid,
                        "summary": preview.summary,
                        "estimated_bytes": preview.estimated_bytes,
                        "items": preview.items.iter().map(|it| serde_json::json!({
                            "label": it.label,
                            "detail": it.detail,
                            "bytes": it.bytes,
                        })).collect::<Vec<_>>(),
                        "warnings": preview.warnings,
                        "hint": "若用户同意继续，再次调用时把 apply 设为 true。",
                    })
                    .to_string(),
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
                "Error: skill_id '{}' not found. Available: {:?}",
                sid, known
            )
        }
    }
}

fn string_array_arg(args: &Value, key: &str) -> Vec<String> {
    args.get(key)
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}
