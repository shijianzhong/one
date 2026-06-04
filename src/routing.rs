use gpui::Context;

use crate::agents::types::RoutingDecision;
use crate::i18n::{t, Translations};
use crate::memory::types::ChatMessage;
use crate::{task_db, AppState, RequestKind};

impl AppState {
    /// Route a message using deterministic routing first, with the orchestrator
    /// as the fallback for complex or unclear requests.
    pub(crate) fn route_message(&mut self, message: String, cx: &mut Context<Self>) {
        self.messages.push(ChatMessage::new("user", &message));
        if let Some(task_id) = self.active_task_id {
            task_db::insert_message(&self.db.conn, task_id, "user", &message).ok();
        }
        self.needs_auto_scroll = true;
        cx.notify();

        if let Some(decision) = self.intent_router.quick_route(&message) {
            eprintln!("[ROUTER] Fast route matched: {:?}", decision);
            self.handle_routing_decision(decision, cx);
            return;
        }

        // No precise routing matched → use Orchestrator (MainAgent)
        eprintln!("[ROUTER] No precise route, switching to Orchestrator");
        self.spawn_orchestrator_run(message, cx);
    }

    fn handle_routing_decision(&mut self, decision: RoutingDecision, cx: &mut Context<Self>) {
        match decision {
            RoutingDecision::ClaudeCode {
                instruction,
                session_id,
            } => {
                eprintln!("[ROUTER] Routing to Claude Code (fast route)");
                self.job_manager.set_request(
                    RequestKind::ClaudeCode,
                    Some(
                        t(
                            self.current_lang,
                            Translations::CLAUDE_CODE_RUNNING_ELLIPSIS,
                        )
                        .to_string(),
                    ),
                );
                self.spawn_claude_code_run(instruction, session_id, cx);
            }
            RoutingDecision::SystemTools { task } => {
                eprintln!("[ROUTER] Routing to System Tools (fast route)");
                self.spawn_system_tools_run(task, cx);
            }
            RoutingDecision::GeneralAI { .. } => {
                eprintln!("[ROUTER] Routing to General AI (via Orchestrator)");
                // We could pass the messages, but Orchestrator currently takes a task string.
                // Since route_message already added the latest message to self.messages,
                // and spawn_orchestrator_run will use self.messages (indirectly or directly)?
                // Wait, spawn_orchestrator_run takes instruction.
                
                // If it was a GeneralAI decision, we might want to just let it fall through.
                // But for now, let's just use the last message.
                let last_msg = self.messages.last().map(|m| m.content.clone()).unwrap_or_default();
                self.spawn_orchestrator_run(last_msg, cx);
            }
        }
    }
}
