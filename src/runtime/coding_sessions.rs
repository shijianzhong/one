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
                self.terminal_refresh_generation = self.terminal_refresh_generation.wrapping_add(1);
                self.terminal_refresh_running = false;
                self.mark_task_active(task_id);
                let message = format!(
                    "{} 终端 runtime 已启动，session_id=`{}`，cwd=`{}`，command=`{}`。",
                    agent_kind.label(),
                    session_id,
                    cwd.to_string_lossy(),
                    agent_kind.command_line()
                );
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
                self.mark_task_active(task_id);
                self.append_task_message(
                    Some(task_id),
                    "assistant",
                    &format!("已发送到 coding session `{}`。", session_id),
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
                let body = if lines.is_empty() {
                    "最近没有可见输出。".to_string()
                } else {
                    lines.join("\n")
                };
                self.append_task_message(
                    Some(task_id),
                    "assistant",
                    &format!(
                        "coding session `{}` 最近输出：\n\n```text\n{}\n```",
                        session_id, body
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
