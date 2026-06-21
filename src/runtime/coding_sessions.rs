use gpui::Context;

use crate::memory::types::ChatMessage;
use crate::runtime::CodingAgentProvider;
use crate::{task_db, AppState};

fn attached_session_id(
    sessions: &std::sync::Arc<std::sync::Mutex<crate::runtime::PersistentCliSessionManager>>,
    task_id: usize,
) -> Option<String> {
    sessions
        .lock()
        .ok()
        .and_then(|sessions| sessions.attached_session_id_for_task(task_id))
}

impl AppState {
    pub(crate) fn append_task_message(
        &mut self,
        task_id: Option<usize>,
        role: &str,
        content: &str,
        cx: &mut Context<Self>,
    ) {
        let current_active_id = self.active_task_id;
        if let Some(task) = self.task_mut(task_id) {
            task.messages.push(ChatMessage::new(role, content));
            if task_id == current_active_id {
                task.needs_auto_scroll = true;
            }
        }
        if let Some(task_id) = task_id {
            let _ = task_db::insert_message(&self.db.conn, task_id, role, content);
        }
        cx.notify();
    }

    pub(crate) fn start_persistent_coding_session(
        &mut self,
        agent_kind: CodingAgentProvider,
        prompt: String,
        write_mode: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(task_id) = self.active_task_id else {
            self.append_task_message(None, "assistant", "请先选择一个 task。", cx);
            return;
        };
        let Some(workspace) = self.get_active_workspace() else {
            self.append_task_message(Some(task_id), "assistant", "请先选择一个 workspace。", cx);
            return;
        };
        let workspace_id = workspace.id;
        let cwd = workspace.path.clone();
        let _ = self
            .get_active_task_location()
            .map(|(workspace_id, task_id, title)| {
                self.ensure_task_storage_dir(workspace_id, task_id, &title)
            });

        let prompt = format_coding_runtime_instruction(&prompt, &cwd);
        let start_result = self
            .coding_sessions
            .lock()
            .map_err(|_| "coding session manager lock poisoned".to_string())
            .and_then(|mut sessions| {
                sessions
                    .start_session(
                        &self.db.conn,
                        task_id,
                        workspace_id,
                        agent_kind.clone(),
                        cwd.clone(),
                        write_mode,
                        Some(&prompt),
                    )
                    .map_err(|error| error.to_string())
            });

        match start_result {
            Ok(session_id) => {
                self.terminal_visible = true;
                self.active_terminal_tab = crate::TerminalTab::Coding;
                self.terminal_scroll_handle.scroll_to_bottom();
                self.terminal_refresh_generation = self.terminal_refresh_generation.wrapping_add(1);
                self.terminal_refresh_running = false;
                self.mark_task_inactive(Some(task_id));
                self.job_manager.clear_request_full();
                let mut message = format!(
                    "{} 已在右侧终端启动。我会把整理后的工程任务交给它处理；需要你确认、登录或选择时，我会在这里提醒你。",
                    agent_kind.label()
                );
                if let Ok(mut sessions) = self.coding_sessions.lock() {
                    if let Ok(inspection) = sessions.inspect_runtime(&self.db.conn, &session_id, 80)
                    {
                        if matches!(
                            inspection.kind.as_str(),
                            "auth_required"
                                | "trust_required"
                                | "permission_required"
                                | "choice_required"
                                | "command_missing"
                        ) {
                            message.push_str(&format!(
                                "\n\n当前状态：status=`{}` kind=`{}`。{}\n建议：{}",
                                inspection.status,
                                inspection.kind,
                                inspection.summary,
                                inspection.suggested_message
                            ));
                        }
                    }
                }
                self.append_task_message(Some(task_id), "assistant", &message, cx);
            }
            Err(error) => {
                self.append_task_message(
                    Some(task_id),
                    "assistant",
                    &format!("启动 {} 终端 runtime 失败：{}", agent_kind.label(), error),
                    cx,
                );
            }
        }
    }

    pub(crate) fn send_to_persistent_coding_session(
        &mut self,
        session_id: Option<String>,
        text: String,
        cx: &mut Context<Self>,
    ) {
        let Some(task_id) = self.active_task_id else {
            self.append_task_message(None, "assistant", "请先选择一个 task。", cx);
            return;
        };
        let session_id =
            match session_id.or_else(|| attached_session_id(&self.coding_sessions, task_id)) {
                Some(session_id) => session_id,
                None => {
                    self.append_task_message(
                        Some(task_id),
                        "assistant",
                        "当前 task 没有绑定的持久 coding session。",
                        cx,
                    );
                    return;
                }
            };

        let cwd = self.coding_sessions.lock().ok().and_then(|sessions| {
            sessions
                .list_sessions()
                .into_iter()
                .find(|session| session.session_id == session_id)
                .map(|session| session.cwd)
        });
        let text = cwd
            .as_ref()
            .map(|cwd| format_coding_runtime_instruction(&text, cwd))
            .unwrap_or(text);
        let send_result = self
            .coding_sessions
            .lock()
            .map_err(|_| "coding session manager lock poisoned".to_string())
            .and_then(|mut sessions| {
                sessions
                    .send_input(&self.db.conn, &session_id, &text)
                    .map_err(|error| error.to_string())
            });

        match send_result {
            Ok(()) => {
                self.terminal_visible = true;
                self.terminal_scroll_handle.scroll_to_bottom();
                self.mark_task_inactive(Some(task_id));
                self.job_manager.clear_request_full();
                self.append_task_message(
                    Some(task_id),
                    "assistant",
                    "我已把你的补充需求整理后交给右侧终端继续处理。",
                    cx,
                );
            }
            Err(error) => {
                self.append_task_message(
                    Some(task_id),
                    "assistant",
                    &format!("发送到 coding session 失败：{}", error),
                    cx,
                );
            }
        }
    }

    pub(crate) fn read_persistent_coding_session_output(
        &mut self,
        session_id: Option<String>,
        limit: usize,
        cx: &mut Context<Self>,
    ) {
        let Some(task_id) = self.active_task_id else {
            self.append_task_message(None, "assistant", "请先选择一个 task。", cx);
            return;
        };
        let session_id =
            match session_id.or_else(|| attached_session_id(&self.coding_sessions, task_id)) {
                Some(session_id) => session_id,
                None => {
                    self.append_task_message(
                        Some(task_id),
                        "assistant",
                        "当前 task 没有绑定的持久 coding session。",
                        cx,
                    );
                    return;
                }
            };
        let read_result = self
            .coding_sessions
            .lock()
            .map_err(|_| "coding session manager lock poisoned".to_string())
            .and_then(|mut sessions| {
                sessions
                    .read_recent_output(&self.db.conn, &session_id, limit)
                    .map_err(|error| error.to_string())
            });

        match read_result {
            Ok(lines) => {
                self.append_task_message(
                    Some(task_id),
                    "assistant",
                    &format!(
                        "已读取 coding runtime `{}` 的最近输出。原始内容保留在右侧终端；我只会在需要你确认、登录或选择时在这里提醒你。本次读取到 {} 行。",
                        session_id,
                        lines.len()
                    ),
                    cx,
                );
            }
            Err(error) => {
                self.append_task_message(
                    Some(task_id),
                    "assistant",
                    &format!("读取 coding session 输出失败：{}", error),
                    cx,
                );
            }
        }
    }

    pub(crate) fn inspect_persistent_coding_session(
        &mut self,
        session_id: Option<String>,
        limit: usize,
        cx: &mut Context<Self>,
    ) {
        let Some(task_id) = self.active_task_id else {
            self.append_task_message(None, "assistant", "请先选择一个 task。", cx);
            return;
        };
        let session_id =
            match session_id.or_else(|| attached_session_id(&self.coding_sessions, task_id)) {
                Some(session_id) => session_id,
                None => {
                    self.append_task_message(
                        Some(task_id),
                        "assistant",
                        "当前 task 没有绑定的终端 coding runtime。",
                        cx,
                    );
                    return;
                }
            };
        let inspect_result = self
            .coding_sessions
            .lock()
            .map_err(|_| "coding session manager lock poisoned".to_string())
            .and_then(|mut sessions| {
                sessions
                    .inspect_runtime(&self.db.conn, &session_id, limit)
                    .map_err(|error| error.to_string())
            });

        match inspect_result {
            Ok(inspection) => {
                let message = match inspection.kind.as_str() {
                    "choice_required" => format!(
                        "Claude Code 需要你确认下一步。\n\n{}",
                        inspection.suggested_message
                    ),
                    "auth_required" | "trust_required" | "permission_required" => format!(
                        "Claude Code 暂停下来等待你的操作。\n\n{}",
                        inspection.suggested_message
                    ),
                    "busy" => "Claude Code 正在处理任务。我会在它需要你确认或补充信息时提醒你。"
                        .to_string(),
                    "ready_for_input" => {
                        "Claude Code 当前已就绪，可以继续接收你的下一步需求。".to_string()
                    }
                    "command_missing" => inspection.suggested_message,
                    "not_active" => {
                        "这个 coding runtime 已经不在运行。如需继续，请重新启动。".to_string()
                    }
                    _ => format!(
                        "已检查 Claude Code 状态：{}。{}",
                        inspection.summary, inspection.suggested_message
                    ),
                };
                self.append_task_message(Some(task_id), "assistant", &message, cx);
            }
            Err(error) => {
                self.append_task_message(
                    Some(task_id),
                    "assistant",
                    &format!("分析 terminal runtime 状态失败：{}", error),
                    cx,
                );
            }
        }
    }

    pub(crate) fn stop_persistent_coding_session(
        &mut self,
        session_id: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(task_id) = self.active_task_id else {
            self.append_task_message(None, "assistant", "请先选择一个 task。", cx);
            return;
        };
        let session_id =
            match session_id.or_else(|| attached_session_id(&self.coding_sessions, task_id)) {
                Some(session_id) => session_id,
                None => {
                    self.append_task_message(
                        Some(task_id),
                        "assistant",
                        "当前 task 没有绑定的持久 coding session。",
                        cx,
                    );
                    return;
                }
            };
        let stop_result = self
            .coding_sessions
            .lock()
            .map_err(|_| "coding session manager lock poisoned".to_string())
            .and_then(|mut sessions| {
                sessions
                    .stop_session(&self.db.conn, &session_id)
                    .map_err(|error| error.to_string())
            });

        match stop_result {
            Ok(()) => {
                self.mark_task_inactive(Some(task_id));
                self.append_task_message(
                    Some(task_id),
                    "assistant",
                    &format!("已停止 coding session `{}`。", session_id),
                    cx,
                );
            }
            Err(error) => {
                self.append_task_message(
                    Some(task_id),
                    "assistant",
                    &format!("停止 coding session 失败：{}", error),
                    cx,
                );
            }
        }
    }

    pub(crate) fn list_persistent_coding_sessions(&mut self, cx: &mut Context<Self>) {
        let task_id = self.active_task_id;
        let sessions = self
            .coding_sessions
            .lock()
            .map(|sessions| sessions.list_sessions())
            .unwrap_or_default();
        let body = if sessions.is_empty() {
            "当前没有持久 coding session。".to_string()
        } else {
            sessions
                .into_iter()
                .map(|session| {
                    format!(
                        "- `{}` {} task={} workspace={} status={} cwd=`{}` write={}",
                        session.session_id,
                        session.agent_kind.label(),
                        session.task_id,
                        session.workspace_id,
                        session.status.label(),
                        session.cwd.to_string_lossy(),
                        session.write_mode
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        self.append_task_message(task_id, "assistant", &body, cx);
    }

    pub(crate) fn describe_workspace_write_status(&mut self, cx: &mut Context<Self>) {
        let task_id = self.active_task_id;
        let Some(workspace_id) = self.active_workspace_id else {
            self.append_task_message(task_id, "assistant", "请先选择一个 workspace。", cx);
            return;
        };
        let message = self
            .coding_sessions
            .lock()
            .ok()
            .and_then(|sessions| {
                sessions
                    .active_write_session_for_workspace(workspace_id)
                    .map(|session| {
                        format!(
                            "当前 workspace 的写锁由 `{}` {} 持有，status={}。",
                            session.session_id,
                            session.agent_kind.label(),
                            session.status.label()
                        )
                    })
            })
            .unwrap_or_else(|| "当前 workspace 没有 write-active coding session。".to_string());
        self.append_task_message(task_id, "assistant", &message, cx);
    }
}

fn format_coding_runtime_instruction(text: &str, cwd: &std::path::Path) -> String {
    format!(
        "MainAgent 已理解用户需求并整理成以下工程任务。请严格在当前 runtime cwd 内工作。\n\
         runtime cwd: {}\n\
         约束：除非用户明确给出绝对路径，否则所有创建、读取、修改、删除都必须发生在上述 cwd 内；不要使用历史 workspace、其他 task 目录或父级目录作为目标路径。\n\n\
         任务说明：\n{}",
        cwd.to_string_lossy(),
        text.trim()
    )
}
