use crate::memory::types::ChatMessage;
use crate::task_db;
use crate::AppState;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Workspace {
    pub id: usize,
    pub name: String,
    pub path: PathBuf,
    pub tasks: Vec<TaskItem>,
    pub expanded: bool,
}

#[derive(Debug, Clone)]
pub struct TaskItem {
    pub id: usize,
    pub title: String,
    pub is_draft: bool,
    pub messages: Vec<ChatMessage>,
    pub pending_summarize: bool,
    pub needs_auto_scroll: bool,
    pub think_collapsed: HashMap<String, bool>,
}

pub fn slugify_task_title(title: &str) -> String {
    let mut slug = String::new();
    let mut prev_dash = false;
    for ch in title.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash {
            slug.push('-');
            prev_dash = true;
        }
    }
    slug.trim_matches('-')
        .to_string()
        .chars()
        .take(32)
        .collect()
}

impl AppState {
    pub fn get_task_dir_for_ids(
        &self,
        workspace_id: usize,
        task_id: usize,
        task_title: &str,
    ) -> PathBuf {
        let workspace_root = self
            .workspaces
            .iter()
            .find(|w| w.id == workspace_id)
            .map(|w| w.path.clone())
            .unwrap_or_else(|| self.default_work_dir.clone());
        let slug = slugify_task_title(task_title);
        let dir_name = if slug.is_empty() {
            format!("{}", task_id)
        } else {
            format!("{}-{}", task_id, slug)
        };
        workspace_root.join("tasks").join(dir_name)
    }

    pub fn get_active_task_location(&self) -> Option<(usize, usize, String)> {
        let workspace_id = self.active_workspace_id?;
        let task_id = self.active_task_id?;
        let task = self.get_active_task()?;
        Some((workspace_id, task_id, task.title.clone()))
    }

    pub fn get_active_task_dir_path(&self) -> Option<PathBuf> {
        let (workspace_id, task_id, title) = self.get_active_task_location()?;
        Some(self.get_task_dir_for_ids(workspace_id, task_id, &title))
    }

    pub fn ensure_task_storage_dir(
        &self,
        workspace_id: usize,
        task_id: usize,
        task_title: &str,
    ) -> PathBuf {
        let task_dir = self.get_task_dir_for_ids(workspace_id, task_id, task_title);
        let _ = std::fs::create_dir_all(&task_dir);
        task_dir
    }

    pub fn restore_task_context(&mut self) {
        // ── 清理前一个 task 的运行状态，防止污染新 task ───────────
        self.job_manager.request_in_flight = false;
        self.job_manager.request_kind = None;
        self.job_manager.request_status_text = None;
        self.job_manager.general_ai_run_id = None;
        self.job_manager.general_ai_task_id = None;
        self.job_manager.general_ai_show_live_bubble = false;
        self.job_manager.general_ai_live_text.clear();
        // 注意：不要关闭 orchestrator_user_input_tx！
        // 旧 Orchestrator 可能在后台运行，关闭 channel 会导致它生成
        // "用户取消了操作"的回复写入 DB。保留 channel，让旧 Orchestrator
        // 自然结束后写入 DB，下次切回 task 时从 DB 加载就能看到结果。

        if let Some((workspace_id, task_id, title)) = self.get_active_task_location() {
            let _ = self.ensure_task_storage_dir(workspace_id, task_id, &title);
            let msgs = task_db::load_messages(&self.db.conn, task_id).unwrap_or_default();
            let msg_vec: Vec<ChatMessage> = msgs
                .into_iter()
                .map(|m| ChatMessage::new(&m.role, &m.content))
                .collect();
            if let Some(task) = self.task_mut(Some(task_id)) {
                task.messages = msg_vec;
            }
        } else if let Some(task) = self.active_task_mut() {
            task.messages.clear();
        }
    }

    pub(crate) fn active_task_ref(&self) -> Option<&TaskItem> {
        let tid = self.active_task_id?;
        self.workspaces
            .iter()
            .flat_map(|w| &w.tasks)
            .find(|t| t.id == tid)
    }

    pub(crate) fn active_task_mut(&mut self) -> Option<&mut TaskItem> {
        let tid = self.active_task_id?;
        self.workspaces
            .iter_mut()
            .flat_map(|w| &mut w.tasks)
            .find(|t| t.id == tid)
    }

    pub(crate) fn task_mut(&mut self, task_id: Option<usize>) -> Option<&mut TaskItem> {
        let tid = task_id?;
        self.workspaces
            .iter_mut()
            .flat_map(|w| &mut w.tasks)
            .find(|t| t.id == tid)
    }
}
