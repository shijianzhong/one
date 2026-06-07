use std::path::PathBuf;
use crate::agents::types::ArtifactEntry;
use crate::memory::types::ChatMessage;
use crate::task_db;
use crate::AppState;

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

    pub fn get_claude_meta_dir_for_task_dir(task_dir: &std::path::Path) -> PathBuf {
        task_dir.join(".claude")
    }

    pub fn ensure_task_storage_dir(
        &self,
        workspace_id: usize,
        task_id: usize,
        task_title: &str,
    ) -> PathBuf {
        let task_dir = self.get_task_dir_for_ids(workspace_id, task_id, task_title);
        let _ = std::fs::create_dir_all(&task_dir);
        let _ = std::fs::create_dir_all(Self::get_claude_meta_dir_for_task_dir(&task_dir));
        task_dir
    }

    pub fn load_artifacts_for_task_dir(task_dir: &std::path::Path) -> Vec<ArtifactEntry> {
        fn walk(
            root: &std::path::Path,
            dir: &std::path::Path,
            out: &mut Vec<ArtifactEntry>,
            depth: usize,
        ) {
            if depth > 4 {
                return;
            }
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().to_string();
                if path.is_dir() {
                    if [".claude", ".git", "node_modules", "target"].contains(&name.as_str()) {
                        continue;
                    }
                    walk(root, &path, out, depth + 1);
                } else if path.is_file() {
                    let relative_path = path
                        .strip_prefix(root)
                        .unwrap_or(&path)
                        .to_string_lossy()
                        .replace('\\', "/");
                    let kind = path
                        .extension()
                        .and_then(|ext| ext.to_str())
                        .map(|ext| ext.to_ascii_lowercase())
                        .unwrap_or_else(|| "file".to_string());
                    out.push(ArtifactEntry {
                        name,
                        relative_path,
                        absolute_path: path.to_string_lossy().to_string(),
                        kind,
                    });
                }
            }
        }

        let mut out = Vec::new();
        if task_dir.exists() {
            walk(task_dir, task_dir, &mut out, 0);
        }
        out.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
        out
    }

    pub fn restore_task_context(&mut self) {
        // ── 清理前一个 task 的运行状态，防止污染新 task ───────────
        // 保留 subagent_messages 和 orchestrator_agent_run_map 不清理，
        // subagent 卡片由 UI 层按 task_id 过滤渲染，切换时不清除数据
        self.job_manager.request_in_flight = false;
        self.job_manager.request_kind = None;
        self.job_manager.request_status_text = None;
        self.job_manager.general_ai_run_id = None;
        self.job_manager.general_ai_task_id = None;
        self.job_manager.general_ai_show_live_bubble = false;
        self.job_manager.general_ai_live_text.clear();

        if let Some((workspace_id, task_id, title)) = self.get_active_task_location() {
            let _ = self.ensure_task_storage_dir(workspace_id, task_id, &title);
            let msgs = task_db::load_messages(&self.db.conn, task_id).unwrap_or_default();
            self.messages = msgs
                .into_iter()
                .map(|m| ChatMessage::new(&m.role, &m.content))
                .collect();
            self.job_manager.current_claude_run =
                self.load_claude_state_for_task(workspace_id, task_id, &title);
        } else {
            self.messages.clear();
            self.job_manager.current_claude_run = None;
        }
    }
}
