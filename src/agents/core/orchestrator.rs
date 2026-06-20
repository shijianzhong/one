use anyhow::Result;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use super::{AgentRunContext, AgentTrait};
use crate::agents::core::agent_runtime::AgentRuntime;
use crate::agents::core::tool_dispatcher::ToolDispatcher;
use crate::mcp::McpClientManager;
use crate::memory::types::ChatMessage;

#[derive(Debug, Clone)]
pub enum OrchestratorEvent {
    /// MainAgent is generating a plan / thinking text
    Plan {
        plan: String,
    },
    /// Real-time stream delta from the main assistant
    AssistantDelta(String),
    /// A sub-step has started (kept for UI compatibility, but no longer used for sub-agents)
    StepStarted {
        agent_id: String,
        agent_name: String,
    },
    /// A step has finished
    StepFinished {
        result: String,
    },
    /// A tool has been called
    ToolCall {
        name: String,
        args: String,
    },
    /// A tool returned a result
    ToolResult {
        name: String,
        result: String,
    },
    /// Orchestrator is waiting for the user's next message (multi-turn)
    AwaitingUserInput {
        reply: String,
    },
    /// Agent wants to run a command in the terminal
    RunInTerminal {
        command: String,
        work_dir: String,
    },
    /// Agent wants runtime to start or operate a persistent coding CLI session.
    StartCodingSession {
        agent_kind: String,
        prompt: String,
        write_mode: bool,
    },
    SendToCodingSession {
        session_id: Option<String>,
        text: String,
    },
    ReadCodingSessionOutput {
        session_id: Option<String>,
        limit: usize,
    },
    StopCodingSession {
        session_id: Option<String>,
    },
    ListCodingSessions,
    GetWorkspaceWriteStatus,
}

pub struct Orchestrator {
    main_runtime: AgentRuntime,
}

impl Orchestrator {
    pub fn new(
        main_agent: Arc<dyn AgentTrait>,
        _work_dir: std::path::PathBuf,
        mcp_manager: Option<Arc<std::sync::Mutex<McpClientManager>>>,
    ) -> Self {
        Self {
            main_runtime: AgentRuntime::new(main_agent, ToolDispatcher::new(mcp_manager)),
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
        on_event: F,
    ) -> Result<String>
    where
        F: FnMut(OrchestratorEvent) + Send,
    {
        let mut context = AgentRunContext::new(session_id);
        context.cancel_flag = cancel_flag.clone();
        context.user_input_rx = user_input_rx.take();

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
        let skill_info: Vec<String> = crate::skills::skill_manifests()
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

        let capabilities = crate::workflows::capability_manifests();
        if !capabilities.is_empty() {
            let capability_context =
                crate::workflows::format_capabilities_for_prompt(&capabilities);
            context.add_message(ChatMessage::new("system", &capability_context));
        }

        // ── 历史消息（去掉最后一条 user，避免重复） ──────────────────
        let msg_count = history.len();
        for msg in history.into_iter().take(msg_count.saturating_sub(1)) {
            context.add_message(msg);
        }
        context.add_message(ChatMessage::new("user", task));

        self.main_runtime.run(context, on_event).await
    }
}
