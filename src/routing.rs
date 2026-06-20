use gpui::Context;

use crate::agents::intent_router::IntentLevel;
use crate::agents::types::RoutingDecision;
use crate::i18n::{t, Translations};
use crate::memory::types::ChatMessage;
use crate::runtime::{detect_configured_coding_clis, PendingCodingActionReply};
use crate::{task_db, AppState, RequestKind};

impl AppState {
    pub(crate) fn route_message(&mut self, message: String, cx: &mut Context<Self>) {
        let captured_task_id = self.active_task_id; // ✅ 入口 capture，防止切换 task 后写错 DB
        if let Some(task) = self.active_task_mut() {
            task.messages.push(ChatMessage::new("user", &message));
            task.needs_auto_scroll = true;
        }
        if let Some(task_id) = captured_task_id {
            task_db::insert_message(&self.db.conn, task_id, "user", &message).ok();
        }
        cx.notify();

        if let Some(task_id) = captured_task_id {
            let pending_reply = self
                .coding_sessions
                .lock()
                .map_err(|_| "coding session manager lock poisoned".to_string())
                .and_then(|mut sessions| {
                    sessions
                        .reply_to_pending_user_action(&self.db.conn, task_id, &message)
                        .map_err(|error| error.to_string())
                });
            match pending_reply {
                Ok(Some(PendingCodingActionReply::Sent {
                    session_id,
                    choice,
                    meaning,
                })) => {
                    self.terminal_visible = true;
                    self.terminal_scroll_handle.scroll_to_bottom();
                    self.append_task_message(
                        Some(task_id),
                        "assistant",
                        &format!(
                            "已帮你选择：{}（发送 `{}` 到 Claude terminal runtime `{}`）。Claude Code 会继续执行。",
                            meaning, choice, session_id
                        ),
                        cx,
                    );
                    return;
                }
                Ok(Some(PendingCodingActionReply::NeedsExplicitChoice { message })) => {
                    self.append_task_message(Some(task_id), "assistant", &message, cx);
                    return;
                }
                Ok(None) => {}
                Err(error) => {
                    self.append_task_message(
                        Some(task_id),
                        "assistant",
                        &format!("处理 Claude Code 待确认选项失败：{}", error),
                        cx,
                    );
                    return;
                }
            }
        }

        // ── 如果 Orchestrator 正在等待用户输入，通过通道发送 ──────
        // 注意：只发送给所属 task 与当前 task 一致的 Orchestrator
        // 防止切换 task 后，新 task 的消息被错误路由到旧 Orchestrator
        if let Some(input_tx) = self.job_manager.orchestrator_user_input_tx.as_ref() {
            if !self.job_manager.request_in_flight
                && self.job_manager.general_ai_task_id == self.active_task_id
            {
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

        let (intent_level, decision) = self.intent_router.route(&message);
        if matches!(intent_level, IntentLevel::Coding) {
            eprintln!("[ROUTER] Fast route matched: Coding");
            self.route_coding_request(message, cx);
            return;
        }

        if let Some(decision) = decision {
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
                let last_msg = self
                    .active_task_ref()
                    .and_then(|t| t.messages.last().map(|m| m.content.clone()))
                    .unwrap_or_default();
                self.spawn_orchestrator_run(last_msg, cx);
            }
        }
    }

    fn route_coding_request(&mut self, message: String, cx: &mut Context<Self>) {
        let Some(task_id) = self.active_task_id else {
            self.append_task_message(None, "assistant", "请先选择一个 task。", cx);
            return;
        };

        let existing_session_id = self
            .coding_sessions
            .lock()
            .ok()
            .and_then(|sessions| sessions.attached_session_id_for_task(task_id));
        if existing_session_id.is_some() {
            let prompt = format_coding_runtime_prompt(&message);
            self.send_to_persistent_coding_session(existing_session_id, prompt, cx);
            return;
        }

        let installed = detect_configured_coding_clis()
            .into_iter()
            .filter(|cli| cli.installed)
            .collect::<Vec<_>>();
        if installed.is_empty() {
            self.append_task_message(
                Some(task_id),
                "assistant",
                "我识别到这是编码任务，但本机没有检测到可用的 coding CLI。请先安装 Claude Code、Codex 或 Gemini；你也可以告诉我“安装 Claude Code”，我会按安装流程处理。",
                cx,
            );
            return;
        }

        let provider = installed
            .iter()
            .find(|cli| cli.provider.id == "claude")
            .or_else(|| {
                installed
                    .iter()
                    .find(|cli| cli.provider.command_line() == "claude")
            })
            .unwrap_or(&installed[0])
            .provider
            .clone();
        let prompt = format_coding_runtime_prompt(&message);
        self.start_persistent_coding_session(provider, prompt, true, cx);
    }
}

fn format_coding_runtime_prompt(message: &str) -> String {
    format!(
        "用户通过 MainAgent 提出了一个编码需求。请在当前 workspace root 中完成，不要创建额外 task 子目录；如需改文件请直接修改项目代码。\n\n用户需求：\n{}",
        message.trim()
    )
}
