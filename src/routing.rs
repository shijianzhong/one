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

        // No precise routing matched → default to General AI
        eprintln!("[ROUTER] No precise route, defaulting to General AI");
        self.spawn_general_ai_run(cx);
    }

    fn handle_routing_decision(&mut self, decision: RoutingDecision, cx: &mut Context<Self>) {
        match decision {
            RoutingDecision::ClaudeCode {
                instruction,
                session_id,
            } => {
                eprintln!("[ROUTER] Routing to Claude Code (fast route)");
                self.request_in_flight = true;
                self.request_status_text = Some(
                    t(
                        self.current_lang,
                        Translations::CLAUDE_CODE_RUNNING_ELLIPSIS,
                    )
                    .to_string(),
                );
                self.request_kind = Some(RequestKind::ClaudeCode);
                self.spawn_claude_code_run(instruction, session_id, cx);
            }
            RoutingDecision::SystemTools { task } => {
                eprintln!("[ROUTER] Routing to System Tools (fast route)");
                self.spawn_system_tools_run(task, cx);
            }
            RoutingDecision::GeneralAI { .. } => {
                eprintln!("[ROUTER] Routing to General AI (fast route)");
                self.spawn_general_ai_run(cx);
            }
            _ => {
                eprintln!("[ROUTER] Unknown decision, defaulting to General AI");
                self.spawn_general_ai_run(cx);
            }
        }
    }
}
