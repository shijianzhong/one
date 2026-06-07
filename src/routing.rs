use gpui::Context;

use crate::agents::types::RoutingDecision;
use crate::i18n::{t, Translations};
use crate::memory::types::ChatMessage;
use crate::{task_db, AppState, RequestKind};

impl AppState {
    pub(crate) fn route_message(&mut self, message: String, cx: &mut Context<Self>) {
        self.messages.push(ChatMessage::new("user", &message));
        if let Some(task_id) = self.active_task_id {
            task_db::insert_message(&self.db.conn, task_id, "user", &message).ok();
        }
        self.needs_auto_scroll = true;
        cx.notify();

        // ── 如果 Orchestrator 正在等待用户输入，通过通道发送 ──────
        if let Some(input_tx) = self.job_manager.orchestrator_user_input_tx.as_ref() {
            if !self.job_manager.request_in_flight {
                eprintln!("[ROUTER] Sending user input to running orchestrator");
                let _ = input_tx.send(message);
                self.job_manager.set_request(
                    RequestKind::GeneralAi,
                    Some(t(self.current_lang, Translations::GENERATING_RESPONSE).to_string()),
                );
                if let Some(tid) = self.active_task_id {
                    self.mark_task_active(tid);
                }
                cx.notify();
                return;
            }
        }

        if let Some(decision) = self.intent_router.quick_route(&message) {
            eprintln!("[ROUTER] Fast route matched: {:?}", decision);
            self.handle_routing_decision(decision, cx);
            return;
        }

        eprintln!("[ROUTER] No precise route, switching to Orchestrator");
        self.spawn_orchestrator_run(message, cx);
    }

    fn handle_routing_decision(&mut self, decision: RoutingDecision, cx: &mut Context<Self>) {
        match decision {
            RoutingDecision::SystemTools { task } => {
                eprintln!("[ROUTER] Routing to System Tools (fast route)");
                self.spawn_system_tools_run(task, cx);
            }
            RoutingDecision::GeneralAI { .. } => {
                eprintln!("[ROUTER] Routing to General AI (via Orchestrator)");
                let last_msg = self.messages.last().map(|m| m.content.clone()).unwrap_or_default();
                self.spawn_orchestrator_run(last_msg, cx);
            }
        }
    }
}