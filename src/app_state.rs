use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use gpui::{AppContext, Context, FocusHandle, Pixels, Point, ScrollHandle, Window};

use crate::agents;
use crate::agents::types::{PreviewLaunchResult, PreviewState, PreviewStatus};

use crate::i18n::{t, Lang, Translations};
use crate::memory::types::ChatMessage;
use crate::sandbox::backend::Backend;
use crate::services::{save_config, Config};
use crate::skills_market::SkillsMarketState;
use crate::task_db;
use crate::ui_theme::{set_theme_mode, ThemeMode};
use crate::util;
use crate::workspace::{TaskItem, Workspace};
use crate::{
    CancelModelConfig, ExportChat, OpenCipherDialog, OpenModelConfigDialog, SaveModelConfig,
    ToggleLang, ToggleTheme,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MainView {
    Chat,
    SkillsMarket,
    Capabilities,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CapabilitiesTab {
    Library,
    Workflows,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ToastLevel {
    Success,
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone)]
pub(crate) struct ToastInfo {
    pub(crate) id: u64,
    pub(crate) level: ToastLevel,
    pub(crate) message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkflowEditState {
    pub(crate) dirty: bool,
    pub(crate) reason: String,
    pub(crate) last_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkflowActivityState {
    pub(crate) level: String,
    pub(crate) message: String,
}

pub(crate) struct AppState {
    pub(crate) db: task_db::Database,
    pub(crate) workspaces: Vec<Workspace>,
    pub(crate) active_workspace_id: Option<usize>,
    pub(crate) active_task_id: Option<usize>,
    pub(crate) default_work_dir: PathBuf,
    pub(crate) sidebar_visible: bool,
    pub(crate) terminal_visible: bool,
    pub(crate) right_panel_width: f32,
    pub(crate) right_panel_resize_initial_mouse_x: Option<f32>,
    pub(crate) right_panel_resize_initial_width: Option<f32>,
    pub(crate) main_view: MainView,
    pub(crate) capabilities_tab: CapabilitiesTab,
    pub(crate) capability_run_inputs: HashMap<String, String>,
    pub(crate) editing_workflow_id: Option<String>,
    pub(crate) selected_workflow_id: Option<String>,
    pub(crate) selected_workflow_node_id: Option<String>,
    pub(crate) workflow_edit_states: HashMap<String, WorkflowEditState>,
    pub(crate) workflow_activity_states: HashMap<String, WorkflowActivityState>,
    pub(crate) workflow_node_run_statuses: HashMap<String, HashMap<String, String>>,
    pub(crate) workflow_edit_json: String,
    pub(crate) expanded_workflow_run_id: Option<usize>,
    pub(crate) capability_import_json: String,
    pub(crate) expanded_capability_versions_id: Option<String>,
    pub(crate) expanded_capability_dependencies_id: Option<String>,
    pub(crate) skills_market: SkillsMarketState,
    pub(crate) show_model_config_dialog: bool,
    pub(crate) show_export_dialog: bool,
    pub(crate) exported_json: Option<String>,
    pub(crate) exported_md: Option<String>,
    pub(crate) model_base_url: String,
    pub(crate) model_api_key: String,
    pub(crate) model_name: String,
    pub(crate) current_lang: Lang,
    pub(crate) theme_mode: ThemeMode,
    pub(crate) editing_model_name: String,
    pub(crate) editing_base_url: String,
    pub(crate) editing_api_key: String,
    pub(crate) chat_scroll_handle: ScrollHandle,
    pub(crate) next_summarize_job_id: u64,
    pub(crate) summarize_job_id: Option<u64>,
    pub(crate) sandbox_backend: Backend,
    pub(crate) hovered_workspace_id: Option<usize>,
    pub(crate) delete_confirm_workspace_id: Option<usize>,
    pub(crate) popup_position: Point<Pixels>,
    pub(crate) terminal_output: Vec<TerminalLine>,
    pub(crate) active_terminal_tab: TerminalTab,
    pub(crate) coding_sessions:
        std::sync::Arc<std::sync::Mutex<crate::runtime::PersistentCliSessionManager>>,
    pub(crate) preview_process: Option<PreviewProcessHandle>,
    pub(crate) preview_state: Option<PreviewState>,
    pub(crate) titlebar_should_move: bool,
    pub(crate) intent_router: agents::intent_router::IntentRouter,
    pub(crate) job_manager: crate::runtime::JobManager,
    /// Approval request currently shown to the user (if any).
    pub(crate) pending_approval: Option<crate::agents::permission::ApprovalRequest>,
    pub(crate) pending_soul_proposal: Option<crate::agents::soul::SoulProposal>,
    pub(crate) skill_card: Option<SkillCardState>,
    /// 暗号设置对话框
    pub(crate) show_cipher_dialog: bool,
    pub(crate) cipher_edit_text: String,
    pub(crate) cipher_confirm_text: String,
    pub(crate) cipher_message: String,
    pub(crate) cipher_message_is_error: bool,
    /// Telegram 绑定引导
    pub(crate) telegram_bind_token: String,
    pub(crate) telegram_bind_status: String,
    pub(crate) telegram_bind_error: bool,
    /// MCP 客户端管理器
    pub(crate) mcp_manager: Option<std::sync::Arc<std::sync::Mutex<crate::mcp::McpClientManager>>>,
    /// 真实的终端模拟器
    pub(crate) terminal_emulator:
        Option<std::sync::Arc<std::sync::Mutex<crate::terminal_emulator::TerminalEmulator>>>,
    /// 当前终端对应的工作目录；切换 task 后用于判断是否需要重建终端。
    pub(crate) terminal_work_dir: Option<PathBuf>,
    /// 终端刷新循环代号；重建终端时递增，让旧刷新循环退出。
    pub(crate) terminal_refresh_generation: u64,
    /// 当前是否已有终端刷新循环在运行。
    pub(crate) terminal_refresh_running: bool,
    /// 当前是否已有终端 runtime event 订阅循环在运行。
    pub(crate) terminal_event_subscription_running: bool,
    pub(crate) pending_coding_supervision: HashMap<String, u64>,
    /// 终端的焦点句柄
    pub(crate) terminal_focus_handle: FocusHandle,
    /// 终端输出滚动状态
    pub(crate) terminal_scroll_handle: ScrollHandle,
    /// 每个 task 的运行状态（true=有请求在运行）
    pub(crate) task_active_states: HashMap<usize, bool>,
    pub(crate) toasts: Vec<ToastInfo>,
    pub(crate) toast_next_id: u64,
}

#[derive(Debug, Clone)]
pub(crate) enum SkillCardStage {
    Previewing,
    PreviewReady(crate::skills::SkillPreview),
    Executing,
    Done(crate::skills::SkillExecution),
    Failed(String),
}

#[derive(Debug, Clone)]
pub(crate) struct SkillCardState {
    pub manifest: crate::skills::SkillManifest,
    pub args: serde_json::Value,
    pub stage: SkillCardStage,
}

#[derive(Debug, Clone)]
pub(crate) struct TerminalLine {
    pub(crate) command: Option<String>,
    pub(crate) output: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TerminalTab {
    Shell,
    Coding,
}

#[derive(Debug)]
pub(crate) struct PreviewProcessHandle {
    pub(crate) child: Child,
}

impl AppState {
    pub(crate) fn get_active_references(&self) -> Vec<String> {
        // 占位：未来扩展为真实属性
        Vec::new()
    }
}

impl Drop for AppState {
    fn drop(&mut self) {
        if let Ok(mut sessions) = self.coding_sessions.lock() {
            sessions.stop_all_sessions(&self.db.conn);
        }
    }
}

fn select_preview_entry(
    root: &PathBuf,
    html_files: &[PathBuf],
    hints: &[String],
    artifacts: &[task_db::TaskArtifactRow],
) -> PathBuf {
    if let Some(found) = artifacts.iter().find_map(|artifact| {
        if !matches!(artifact.kind.as_str(), "html_entry" | "html_file") {
            return None;
        }
        let path = PathBuf::from(&artifact.path);
        if !path.exists() {
            return None;
        }
        html_files.iter().find(|file| **file == path).cloned()
    }) {
        return found;
    }

    if let Some(found) = hints.iter().find_map(|hint| {
        let hint_lower = hint.to_ascii_lowercase();
        html_files.iter().find(|file| {
            file.file_name()
                .and_then(|n| n.to_str())
                .map(|name| name.to_ascii_lowercase() == hint_lower)
                .unwrap_or(false)
                || file
                    .strip_prefix(root)
                    .unwrap_or(file)
                    .to_string_lossy()
                    .to_ascii_lowercase()
                    .ends_with(&hint_lower)
                || file
                    .to_string_lossy()
                    .to_ascii_lowercase()
                    .ends_with(&hint_lower)
        })
    }) {
        return found.clone();
    }

    if let Some(index_file) = html_files.iter().find(|file| {
        file.file_name()
            .and_then(|n| n.to_str())
            .map(|name| name.eq_ignore_ascii_case("index.html"))
            .unwrap_or(false)
    }) {
        return index_file.clone();
    }

    html_files[0].clone()
}

#[cfg(test)]
mod preview_selection_tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "one-preview-select-test-{}-{}",
            std::process::id(),
            nanos
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn artifact_html_entry_wins_over_hint_and_index() {
        let root = temp_dir();
        let index = root.join("index.html");
        let hinted = root.join("hinted.html");
        let artifact_entry = root.join("app.html");
        std::fs::write(&index, "").unwrap();
        std::fs::write(&hinted, "").unwrap();
        std::fs::write(&artifact_entry, "").unwrap();

        let selected = select_preview_entry(
            &root,
            &[index, hinted, artifact_entry.clone()],
            &["hinted.html".to_string()],
            &[task_db::TaskArtifactRow {
                id: 1,
                task_id: 1,
                workflow_id: Some(1),
                kind: "html_entry".to_string(),
                path: artifact_entry.to_string_lossy().to_string(),
                title: "HTML 入口".to_string(),
                status: "ready".to_string(),
                metadata_json: String::new(),
                updated_at: String::new(),
            }],
        );

        assert_eq!(selected, artifact_entry);
    }
}

impl AppState {
    pub(crate) fn new(_window: &mut Window, cx: &mut Context<Self>, config: Config) -> Self {
        let db = task_db::Database::new().expect("Failed to initialize database");
        let theme_mode = config.theme_mode;
        let last_workspace_id = config.last_workspace_id;
        let last_task_id = config.last_task_id;
        set_theme_mode(theme_mode);

        let workspaces = {
            let conn = &db.conn;
            let db_workspaces = task_db::load_workspaces(conn).unwrap_or_default();
            db_workspaces
                .into_iter()
                .map(|w| {
                    let tasks = task_db::load_tasks(conn, w.id)
                        .unwrap_or_default()
                        .into_iter()
                        .map(|t| TaskItem {
                            id: t.id,
                            title: t.title,
                            is_draft: t.is_draft,
                            messages: vec![],
                            pending_summarize: false,
                            needs_auto_scroll: false,
                            think_collapsed: HashMap::new(),
                        })
                        .collect();
                    Workspace {
                        id: w.id,
                        name: w.name,
                        path: PathBuf::from(w.path),
                        tasks,
                        expanded: w.expanded,
                        default_task_id: w.default_task_id,
                    }
                })
                .collect()
        };

        let mut state = Self {
            db,
            workspaces,
            active_workspace_id: None,
            active_task_id: None,
            default_work_dir: dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")),
            sidebar_visible: false,
            terminal_visible: false,
            right_panel_width: 420.0,
            right_panel_resize_initial_mouse_x: None,
            right_panel_resize_initial_width: None,
            main_view: MainView::Chat,
            capabilities_tab: CapabilitiesTab::Library,
            capability_run_inputs: HashMap::new(),
            editing_workflow_id: None,
            selected_workflow_id: None,
            selected_workflow_node_id: None,
            workflow_edit_states: HashMap::new(),
            workflow_activity_states: HashMap::new(),
            workflow_node_run_statuses: HashMap::new(),
            workflow_edit_json: String::new(),
            expanded_workflow_run_id: None,
            capability_import_json: String::new(),
            expanded_capability_versions_id: None,
            expanded_capability_dependencies_id: None,
            skills_market: SkillsMarketState::new(),
            show_model_config_dialog: false,
            show_export_dialog: false,
            exported_json: None,
            exported_md: None,
            model_base_url: config.model_base_url,
            model_api_key: config.model_api_key,
            model_name: config.model_name,
            current_lang: config.lang,
            theme_mode,
            editing_model_name: "gpt-4".to_string(),
            editing_base_url: "https://api.openai.com/v1".to_string(),
            editing_api_key: "".to_string(),
            next_summarize_job_id: 0,
            summarize_job_id: None,
            chat_scroll_handle: ScrollHandle::default(),
            sandbox_backend: futures::executor::block_on(Backend::detect()),
            terminal_output: vec![],
            active_terminal_tab: TerminalTab::Shell,
            coding_sessions: crate::runtime::global_coding_session_manager(),
            preview_process: None,
            preview_state: None,
            hovered_workspace_id: None,
            delete_confirm_workspace_id: None,
            popup_position: Point::default(),
            titlebar_should_move: false,
            intent_router: agents::intent_router::IntentRouter::new(),
            job_manager: crate::runtime::JobManager::new(),
            pending_approval: None,
            pending_soul_proposal: None,
            skill_card: None,
            show_cipher_dialog: false,
            cipher_edit_text: String::new(),
            cipher_confirm_text: String::new(),
            cipher_message: String::new(),
            cipher_message_is_error: false,
            telegram_bind_token: String::new(),
            telegram_bind_status: String::new(),
            telegram_bind_error: false,
            mcp_manager: None,
            terminal_emulator: None,
            terminal_work_dir: None,
            terminal_refresh_generation: 0,
            terminal_refresh_running: false,
            terminal_event_subscription_running: false,
            pending_coding_supervision: HashMap::new(),
            terminal_focus_handle: cx.focus_handle(),
            terminal_scroll_handle: ScrollHandle::default(),
            task_active_states: HashMap::new(),
            toasts: vec![],
            toast_next_id: 0,
        };

        if state.workspaces.is_empty() {
            let default_ws = Workspace {
                id: 1,
                name: "Default".to_string(),
                path: state.default_work_dir.clone(),
                tasks: vec![],
                expanded: true,
                default_task_id: None,
            };
            state.workspaces.push(default_ws);
        }

        state.restore_last_open_task(last_workspace_id, last_task_id);

        state.start_approval_pump(cx);
        state.ensure_terminal_event_subscription(cx);
        state.init_mcp(cx);
        state
    }

    /// Spin up a background pump that wait for global permission approval
    /// or soul proposal notifications and surfaces the next request as a dialog.
    fn start_approval_pump(&self, cx: &mut Context<Self>) {
        let perm_notify = crate::agents::permission::approval_notify();
        let soul_notify = crate::agents::soul::soul_notify();

        cx.spawn(async move |this, cx| loop {
            // Wait for either a permission or a soul proposal notification.
            // This is zero CPU usage while waiting.
            tokio::select! {
                _ = perm_notify.notified() => {}
                _ = soul_notify.notified() => {}
            }

            let _ = this.update(cx, |state, cx| {
                if state.pending_approval.is_none() {
                    if let Some(req) = crate::agents::permission::drain_next() {
                        state.pending_approval = Some(req);
                        cx.notify();
                    }
                }
                if state.pending_soul_proposal.is_none() {
                    if let Some(prop) = crate::agents::soul::drain_next() {
                        state.pending_soul_proposal = Some(prop);
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    pub(crate) fn approve_pending_permission(&mut self, cx: &mut Context<Self>) {
        if let Some(req) = self.pending_approval.take() {
            req.approve();
            cx.notify();
        }
    }

    pub(crate) fn deny_pending_permission(&mut self, cx: &mut Context<Self>) {
        if let Some(req) = self.pending_approval.take() {
            req.deny();
            cx.notify();
        }
    }

    pub(crate) fn push_toast(
        &mut self,
        level: ToastLevel,
        message: String,
        cx: &mut Context<Self>,
    ) {
        let id = self.toast_next_id;
        self.toast_next_id += 1;
        self.toasts.push(ToastInfo { id, level, message });
        cx.notify();

        let toast_id = id;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_secs(2))
                .await;
            let _ = this.update(cx, |this, _cx| {
                this.toasts.retain(|t| t.id != toast_id);
            });
        })
        .detach();
    }

    pub(crate) fn approve_soul_proposal(&mut self, cx: &mut Context<Self>) {
        if let Some(prop) = self.pending_soul_proposal.take() {
            match crate::agents::soul::commit_proposal(&prop) {
                Ok(()) => {
                    if let Some(task) = self.active_task_mut() {
                        task.messages.push(ChatMessage::new(
                            "assistant",
                            "✅ 已应用新的 soul.md 草案，重启或刷新后生效。",
                        ));
                        task.needs_auto_scroll = true;
                    }
                }
                Err(e) => {
                    if let Some(task) = self.active_task_mut() {
                        task.messages.push(ChatMessage::new(
                            "assistant",
                            &format!("⚠️ 写入 soul.md 失败：{}", e),
                        ));
                        task.needs_auto_scroll = true;
                    }
                }
            }
            cx.notify();
        }
    }

    pub(crate) fn deny_soul_proposal(&mut self, cx: &mut Context<Self>) {
        if self.pending_soul_proposal.take().is_some() {
            if let Some(task) = self.active_task_mut() {
                task.messages.push(ChatMessage::new(
                    "assistant",
                    "❌ 已拒绝 soul.md 草案，未做任何改动。",
                ));
                task.needs_auto_scroll = true;
            }
            cx.notify();
        }
    }

    pub(crate) fn launch_skill_card(
        &mut self,
        skill_id: &str,
        args: serde_json::Value,
        cx: &mut Context<Self>,
    ) {
        let Some(skill) = crate::skills::find_skill(skill_id) else {
            if let Some(task) = self.active_task_mut() {
                task.messages.push(ChatMessage::new(
                    "assistant",
                    &format!("⚠️ Skill `{}` 不存在", skill_id),
                ));
                task.needs_auto_scroll = true;
            }
            cx.notify();
            return;
        };
        let manifest = skill.manifest();
        self.skill_card = Some(SkillCardState {
            manifest: manifest.clone(),
            args: args.clone(),
            stage: SkillCardStage::Previewing,
        });
        cx.notify();

        let skill_id = manifest.id.clone();
        let args_for_preview = args.clone();
        cx.spawn(async move |this, cx| {
            let result = match crate::skills::find_skill(&skill_id) {
                Some(skill) => skill.preview(args_for_preview).await,
                None => Err(anyhow::anyhow!("skill disappeared")),
            };
            let _ = this.update(cx, |state, cx| {
                if let Some(card) = state.skill_card.as_mut() {
                    if card.manifest.id != skill_id {
                        return;
                    }
                    card.stage = match result {
                        Ok(p) => SkillCardStage::PreviewReady(p),
                        Err(e) => SkillCardStage::Failed(format!("预览失败：{}", e)),
                    };
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(crate) fn approve_skill_card(&mut self, cx: &mut Context<Self>) {
        let Some(card) = self.skill_card.as_mut() else {
            return;
        };
        if !matches!(card.stage, SkillCardStage::PreviewReady(_)) {
            return;
        }
        let skill_id = card.manifest.id.clone();
        let args = card.args.clone();
        card.stage = SkillCardStage::Executing;
        cx.notify();

        cx.spawn(async move |this, cx| {
            let result = match crate::skills::find_skill(&skill_id) {
                Some(skill) => skill.execute(args, None).await,
                None => Err(anyhow::anyhow!("skill disappeared")),
            };
            let _ = this.update(cx, |state, cx| {
                if let Some(card) = state.skill_card.as_mut() {
                    if card.manifest.id != skill_id {
                        return;
                    }
                    card.stage = match result {
                        Ok(exec) => SkillCardStage::Done(exec),
                        Err(e) => SkillCardStage::Failed(format!("执行失败：{}", e)),
                    };
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(crate) fn cancel_skill_card(&mut self, cx: &mut Context<Self>) {
        if self.skill_card.take().is_some() {
            cx.notify();
        }
    }

    pub(crate) fn get_active_workspace(&self) -> Option<&Workspace> {
        self.active_workspace_id
            .and_then(|id| self.workspaces.iter().find(|w| w.id == id))
    }

    pub(crate) fn get_active_task(&self) -> Option<&TaskItem> {
        self.get_active_workspace()
            .and_then(|w| w.tasks.iter().find(|t| Some(t.id) == self.active_task_id))
    }

    pub(crate) fn get_work_dir(&self) -> String {
        if let Some(task_dir) = self.get_active_task_dir_path() {
            return task_dir.to_string_lossy().to_string();
        }
        if let Some(ws) = self.get_active_workspace() {
            return ws.path.to_string_lossy().to_string();
        }
        if let Some(task_id) = self.active_task_id {
            format!("/tmp/one_task_{}", task_id)
        } else {
            self.default_work_dir.to_string_lossy().to_string()
        }
    }

    pub(crate) fn add_workspace(&mut self, path: PathBuf, name: String) {
        let path_str = path.to_string_lossy().to_string();
        let id = task_db::insert_workspace(&self.db.conn, &name, &path_str)
            .unwrap_or(self.workspaces.len() + 1);
        let workspace = Workspace {
            id,
            name,
            path,
            tasks: vec![],
            expanded: true,
            default_task_id: None,
        };
        self.workspaces.push(workspace);
        self.active_workspace_id = Some(id);
    }

    pub(crate) fn ensure_workspace_draft_task(&mut self, workspace_id: usize) -> Option<usize> {
        let ws_index = self.workspaces.iter().position(|w| w.id == workspace_id)?;
        let draft_id = task_db::ensure_draft_task(&self.db.conn, workspace_id).ok()?;

        if let Ok(rows) = task_db::load_tasks(&self.db.conn, workspace_id) {
            self.workspaces[ws_index].tasks = rows
                .into_iter()
                .map(|t| TaskItem {
                    id: t.id,
                    title: t.title,
                    is_draft: t.is_draft,
                    messages: vec![],
                    pending_summarize: false,
                    needs_auto_scroll: false,
                    think_collapsed: HashMap::new(),
                })
                .collect();
        }

        if let Some(title) = self.workspaces[ws_index]
            .tasks
            .iter()
            .find(|t| t.id == draft_id)
            .map(|t| t.title.clone())
        {
            let _ = self.ensure_task_storage_dir(workspace_id, draft_id, &title);
        }
        Some(draft_id)
    }

    pub(crate) fn select_task(&mut self, workspace_id: usize, task_id: Option<usize>) {
        self.active_workspace_id = Some(workspace_id);
        self.active_task_id = task_id;
        if let Some(ws) = self.workspaces.iter_mut().find(|w| w.id == workspace_id) {
            ws.default_task_id = task_id;
        }
        let _ = task_db::update_workspace_default_task(&self.db.conn, workspace_id, task_id);
        self.persist_last_open_task(workspace_id, task_id);
        self.restore_task_context();
    }

    fn persist_last_open_task(&self, workspace_id: usize, task_id: Option<usize>) {
        let mut config = crate::services::load_config();
        config.last_workspace_id = Some(workspace_id);
        config.last_task_id = task_id;
        if let Err(error) = crate::services::save_config(&config) {
            eprintln!("Failed to save last open task: {}", error);
        }
    }

    fn restore_last_open_task(
        &mut self,
        last_workspace_id: Option<usize>,
        last_task_id: Option<usize>,
    ) {
        let workspace_index = last_workspace_id
            .and_then(|workspace_id| {
                self.workspaces
                    .iter()
                    .position(|workspace| workspace.id == workspace_id)
            })
            .filter(|workspace_index| {
                last_task_id.is_none_or(|task_id| {
                    self.workspaces[*workspace_index]
                        .tasks
                        .iter()
                        .any(|task| task.id == task_id)
                })
            })
            .or_else(|| {
                self.workspaces.iter().position(|workspace| {
                    workspace.default_task_id.is_some_and(|task_id| {
                        workspace.tasks.iter().any(|task| task.id == task_id)
                    })
                })
            });

        let Some(workspace_index) = workspace_index else {
            return;
        };
        let workspace_id = self.workspaces[workspace_index].id;
        let task_id = if last_workspace_id == Some(workspace_id) {
            last_task_id
        } else {
            self.workspaces[workspace_index].default_task_id
        };
        self.active_workspace_id = Some(workspace_id);
        self.active_task_id = task_id;
        self.workspaces[workspace_index].expanded = true;
        let _ = task_db::update_workspace_expanded(&self.db.conn, workspace_id, true);
        self.restore_task_context();
    }

    pub(crate) fn stop_preview_process(&mut self) {
        if let Some(mut handle) = self.preview_process.take() {
            let _ = handle.child.kill();
            let _ = handle.child.wait();
        }
    }

    pub(crate) fn open_url_in_browser(&self, url: &str) {
        let _ = Command::new("open")
            .arg(url)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
    }

    pub(crate) fn open_folder_in_finder(&self, path: &str) {
        let _ = Command::new("open")
            .arg(path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
    }

    pub(crate) fn reveal_file_in_finder(&self, path: &str) {
        let _ = Command::new("open")
            .arg("-R")
            .arg(path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
    }

    pub(crate) fn try_prepare_preview(
        &mut self,
        work_dir: &str,
        hint_text: &str,
    ) -> PreviewLaunchResult {
        self.stop_preview_process();
        let lang = self.current_lang;

        let root = PathBuf::from(work_dir);
        if !root.exists() {
            return PreviewLaunchResult::Failed {
                note: format!(
                    "{}: {}",
                    t(lang, Translations::PREVIEW_DIR_MISSING),
                    root.display()
                ),
            };
        }

        let html_files = util::collect_html_files(&root);
        if html_files.is_empty() {
            return PreviewLaunchResult::NotFound {
                note: t(lang, Translations::NO_PREVIEWABLE_HTML).to_string(),
            };
        }

        let hints = util::extract_html_hints(hint_text);
        let artifacts = self
            .active_task_id
            .and_then(|task_id| task_db::load_task_artifacts(&self.db.conn, task_id).ok())
            .unwrap_or_default();
        let entry = select_preview_entry(&root, &html_files, &hints, &artifacts);

        let relative_entry = entry
            .strip_prefix(&root)
            .unwrap_or(&entry)
            .to_string_lossy()
            .replace('\\', "/");
        let serve_dir = root.clone();
        let port = 4317 + (self.active_task_id.unwrap_or(0) as u16 % 200);
        let url = format!("http://127.0.0.1:{}/{}", port, relative_entry);

        let child = Command::new("python3")
            .args([
                "-m",
                "http.server",
                &port.to_string(),
                "--bind",
                "127.0.0.1",
            ])
            .current_dir(&serve_dir)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();

        match child {
            Ok(child) => {
                self.preview_process = Some(PreviewProcessHandle { child });
                PreviewLaunchResult::Ready {
                    url,
                    entry_file: entry.to_string_lossy().to_string(),
                    note: format!(
                        "{}: {}",
                        t(lang, Translations::SERVING_WORKSPACE_ROOT),
                        serve_dir.display()
                    ),
                }
            }
            Err(error) => PreviewLaunchResult::Failed {
                note: format!(
                    "{}: {}: {}",
                    t(lang, Translations::FAILED_TO_START_PREVIEW_SERVER),
                    serve_dir.display(),
                    error,
                ),
            },
        }
    }

    pub(crate) fn prepare_active_task_preview(&mut self) {
        let Some(workspace_path) = self
            .get_active_workspace()
            .map(|workspace| workspace.path.clone())
        else {
            self.preview_state = None;
            return;
        };
        let result = self.try_prepare_preview(&workspace_path.to_string_lossy(), "");
        self.preview_state = Some(match result {
            PreviewLaunchResult::Ready {
                url,
                entry_file,
                note,
            } => PreviewState {
                status: PreviewStatus::Ready,
                entry_file: Some(entry_file),
                url: Some(url),
                note,
            },
            PreviewLaunchResult::NotFound { note } => PreviewState {
                status: PreviewStatus::Idle,
                entry_file: None,
                url: None,
                note,
            },
            PreviewLaunchResult::Failed { note } => PreviewState {
                status: PreviewStatus::Failed,
                entry_file: None,
                url: None,
                note,
            },
        });
    }

    pub(crate) fn open_model_config_dialog(
        &mut self,
        _: &OpenModelConfigDialog,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.editing_model_name = self.model_name.clone();
        self.editing_base_url = self.model_base_url.clone();
        self.editing_api_key = self.model_api_key.clone();
        self.show_model_config_dialog = true;
        cx.notify();
    }

    pub(crate) fn open_skills_market(&mut self, cx: &mut Context<Self>) {
        self.skills_market.refresh();
        self.main_view = MainView::SkillsMarket;
        cx.notify();
    }

    pub(crate) fn open_capabilities(&mut self, cx: &mut Context<Self>) {
        self.main_view = MainView::Capabilities;
        cx.notify();
    }

    pub(crate) fn save_model_config(
        &mut self,
        _: &SaveModelConfig,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.model_name = self.editing_model_name.clone();
        self.model_base_url = self.editing_base_url.clone();
        self.model_api_key = self.editing_api_key.clone();
        self.show_model_config_dialog = false;

        let mut config = crate::services::load_config();
        config.model_name = self.model_name.clone();
        config.model_base_url = self.model_base_url.clone();
        config.model_api_key = self.model_api_key.clone();
        config.lang = self.current_lang;
        config.theme_mode = self.theme_mode;
        if let Err(e) = save_config(&config) {
            eprintln!("Failed to save config: {}", e);
        }

        cx.notify();
    }

    pub(crate) fn cancel_model_config(
        &mut self,
        _: &CancelModelConfig,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_model_config_dialog = false;
        cx.notify();
    }

    pub(crate) fn toggle_lang(&mut self, _: &ToggleLang, _: &mut Window, cx: &mut Context<Self>) {
        self.current_lang = self.current_lang.toggle();
        let mut config = crate::services::load_config();
        config.lang = self.current_lang;
        config.model_base_url = self.model_base_url.clone();
        config.model_api_key = self.model_api_key.clone();
        config.model_name = self.model_name.clone();
        config.theme_mode = self.theme_mode;
        if let Err(e) = save_config(&config) {
            eprintln!("Failed to save lang config: {}", e);
        }
        cx.notify();
    }

    pub(crate) fn toggle_theme(&mut self, _: &ToggleTheme, _: &mut Window, cx: &mut Context<Self>) {
        self.theme_mode = match self.theme_mode {
            ThemeMode::Dark => ThemeMode::Light,
            ThemeMode::Light => ThemeMode::Dark,
        };
        set_theme_mode(self.theme_mode);
        let mut config = crate::services::load_config();
        config.theme_mode = self.theme_mode;
        config.model_base_url = self.model_base_url.clone();
        config.model_api_key = self.model_api_key.clone();
        config.model_name = self.model_name.clone();
        config.lang = self.current_lang;
        if let Err(e) = save_config(&config) {
            eprintln!("Failed to save theme config: {}", e);
        }
        cx.notify();
    }

    pub(crate) fn open_cipher_dialog(
        &mut self,
        _: &OpenCipherDialog,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cipher_edit_text.clear();
        self.cipher_confirm_text.clear();
        self.cipher_message.clear();
        self.cipher_message_is_error = false;
        self.show_cipher_dialog = true;
        cx.notify();
    }

    pub(crate) fn save_cipher(&mut self, cx: &mut Context<Self>) {
        let c1 = self.cipher_edit_text.trim().to_string();
        let c2 = self.cipher_confirm_text.trim().to_string();

        if c1.is_empty() {
            self.cipher_message = "暗号不能为空".to_string();
            self.cipher_message_is_error = true;
            cx.notify();
            return;
        }
        if c1 != c2 {
            self.cipher_message = "两次输入的暗号不一致".to_string();
            self.cipher_message_is_error = true;
            cx.notify();
            return;
        }
        if c1.len() < 2 {
            self.cipher_message = "暗号长度不能少于 2 个字符".to_string();
            self.cipher_message_is_error = true;
            cx.notify();
            return;
        }

        match crate::agents::remote_auth::RemoteAuth::set_cipher(&c1) {
            Ok(()) => {
                self.cipher_message = "✅ 暗号设置成功".to_string();
                self.cipher_message_is_error = false;
                self.cipher_edit_text.clear();
                self.cipher_confirm_text.clear();
                cx.notify();
            }
            Err(e) => {
                self.cipher_message = format!("设置失败：{}", e);
                self.cipher_message_is_error = true;
                cx.notify();
            }
        }
    }

    pub(crate) fn clear_cipher(&mut self, cx: &mut Context<Self>) {
        match crate::agents::remote_auth::RemoteAuth::clear_cipher() {
            Ok(()) => {
                self.cipher_message = "暗号已清除".to_string();
                self.cipher_message_is_error = false;
                cx.notify();
            }
            Err(e) => {
                self.cipher_message = format!("清除失败：{}", e);
                self.cipher_message_is_error = true;
                cx.notify();
            }
        }
    }

    pub(crate) fn close_cipher_dialog(&mut self, cx: &mut Context<Self>) {
        self.show_cipher_dialog = false;
        cx.notify();
    }

    /// 初始化 MCP 客户端连接
    pub(crate) fn init_mcp(&mut self, cx: &mut Context<Self>) {
        // 初始化 ToolRegistry（含所有已注册 Skill 工具）
        let workspace_name = self
            .get_active_workspace()
            .map(|w| w.name.as_str())
            .unwrap_or("Default");
        crate::agents::core::tool_registry::init_tool_registry(workspace_name);

        let config = match crate::mcp::config::McpConfig::load_default() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[MCP] Failed to load config: {}", e);
                return;
            }
        };

        if config.mcp_servers.is_empty() {
            return;
        }

        eprintln!(
            "[MCP] Loading {} MCP server(s)...",
            config.mcp_servers.len()
        );

        cx.spawn(async move |this, cx| {
            let manager = crate::mcp::McpClientManager::connect(&config).await;
            let tool_count = manager.tool_count();
            let server_count = manager.server_count();

            // 注册 MCP 工具到 ToolRegistry
            let all_tools = manager.all_tools();
            if !all_tools.is_empty() {
                let mcp_tools: Vec<_> = all_tools
                    .into_iter()
                    .map(
                        |t| crate::agents::core::tool_registry::McpToolRegistration {
                            server_name: t.server_name,
                            tool_name: t.tool_name,
                            description: t.description,
                            input_schema: t.input_schema,
                        },
                    )
                    .collect();
                if let Ok(mut treg) = crate::agents::core::tool_registry::tool_registry().lock() {
                    treg.register_mcp_batch(mcp_tools);
                    eprintln!(
                        "[MCP] Registered {} MCP tool(s) to ToolRegistry",
                        treg.mcp_tools().len()
                    );
                }
            }

            let manager = std::sync::Arc::new(std::sync::Mutex::new(manager));
            crate::mcp::set_global_manager(manager.clone());

            let _ = this.update(cx, |state, cx| {
                state.mcp_manager = Some(manager);
                eprintln!(
                    "[MCP] Connected: {} server(s), {} tool(s)",
                    server_count, tool_count
                );
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn export_chat(&mut self, _: &ExportChat, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(task_id) = self.active_task_id {
            if let Some(task) = self.get_active_task() {
                let lang = self.current_lang;
                let md_title = if task.title.trim().is_empty() {
                    t(lang, Translations::NEW_TASK).to_string()
                } else {
                    task.title.clone()
                };
                let json =
                    task_db::export_messages_json(&self.db.conn, task_id).unwrap_or_default();
                let md = task_db::export_messages_markdown(&self.db.conn, task_id, &md_title)
                    .unwrap_or_default();
                self.exported_json = Some(json);
                self.exported_md = Some(md);
                self.show_export_dialog = true;
                cx.notify();
            }
        }
    }
}

impl AppState {
    /// 标记某个 task 正在运行中
    pub(crate) fn mark_task_active(&mut self, task_id: usize) {
        self.task_active_states.insert(task_id, true);
    }

    /// 标记某个 task 已结束运行
    pub(crate) fn mark_task_inactive(&mut self, task_id: Option<usize>) {
        if let Some(id) = task_id {
            self.task_active_states.insert(id, false);
        }
    }

    /// 查询某个 task 是否正在运行中
    pub(crate) fn is_task_active(&self, task_id: Option<usize>) -> bool {
        task_id
            .map(|id| self.task_active_states.get(&id).copied().unwrap_or(false))
            .unwrap_or(false)
    }

    pub(crate) fn start_telegram_bind(&mut self, cx: &mut Context<Self>) {
        let token = self.telegram_bind_token.trim().to_string();
        if token.is_empty() {
            self.telegram_bind_status = "请输入 Bot Token".to_string();
            self.telegram_bind_error = true;
            cx.notify();
            return;
        }

        // 先停止旧 trigger 实例，避免绑定轮询与旧实例竞争
        crate::triggers::telegram::TelegramTrigger::stop_all();

        println!(
            "[telegram_bind] Starting bind process for token: {}...",
            if token.len() > 10 {
                &token[..10]
            } else {
                &token
            }
        );
        self.telegram_bind_status = "正在验证 Bot Token...".to_string();
        self.telegram_bind_error = false;
        cx.notify();

        let token_clone = token.clone();
        let tokio_handle = gpui_tokio::Tokio::handle(cx);
        let validate_handle = tokio_handle.clone();
        let token_for_validate = token_clone.clone();
        let validate_task = cx.background_spawn(async move {
            let join_handle = validate_handle.spawn(async move {
                let client = reqwest::Client::new();
                let url = format!("https://api.telegram.org/bot{}/getMe", token_for_validate);
                client
                    .get(&url)
                    .send()
                    .await?
                    .json::<serde_json::Value>()
                    .await
            });
            join_handle.await
        });

        cx.spawn(async move |this, cx| {
            let me_result = validate_task.await.ok().and_then(|r| r.ok());
            println!("[telegram_bind] Validation result: {:?}", me_result);

            let bind_code = format!(
                "ONE_BIND_{}",
                chrono::Local::now().format("%Y%m%d%H%M%S")
            );

            let mut is_token_valid = false;
            let status = if let Some(me) = me_result {
                if me.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
                    is_token_valid = true;
                    let bot_name = me["result"]["first_name"].as_str().unwrap_or("Unknown Bot");
                    println!("[telegram_bind] Token is valid. Bot name: {}", bot_name);
                    format!(
                        "Token 有效！机器人：{}\n请在 Telegram 中给 Bot 发送消息，内容包含绑定码：\n`{}`\n\n正在等待消息（120秒超时）...",
                        bot_name, bind_code
                    )
                } else {
                    println!("[telegram_bind] Token is invalid according to Telegram API.");
                    "Token 无效，请检查后重试".to_string()
                }
            } else {
                println!("[telegram_bind] Failed to connect to Telegram API or parse response.");
                "无法连接到 Telegram API，请检查网络".to_string()
            };

            let _ = this.update(cx, |state, cx| {
                state.telegram_bind_status = status;
                state.telegram_bind_error = !is_token_valid;
                state.telegram_bind_token = token_clone.clone();
                cx.notify();
            });

            if !is_token_valid {
                return;
            }

            println!("[telegram_bind] Starting polling loop for bind_code: {}", bind_code);
            // Polling for binding message
            let start_time = std::time::Instant::now();
            let mut offset = 0;
            loop {
                if std::time::Instant::now().duration_since(start_time).as_secs() > 120 {
                    println!("[telegram_bind] Polling timeout reached.");
                    let _ = this.update(cx, |state, cx| {
                        state.telegram_bind_status = "等待绑定超时，请重试".to_string();
                        state.telegram_bind_error = true;
                        cx.notify();
                    });
                    break;
                }

                let token_for_poll = token_clone.clone();
                let bind_code_for_poll = bind_code.clone();
                let poll_handle = tokio_handle.clone();
                println!("[telegram_bind] Polling updates with offset: {}", offset);
                let poll_result = cx.background_spawn(async move {
                    let join_handle = poll_handle.spawn(async move {
                        let client = reqwest::Client::new();
                        let url = format!("https://api.telegram.org/bot{}/getUpdates", token_for_poll);
                        client.post(&url)
                            .json(&serde_json::json!({
                                "offset": offset,
                                "timeout": 5,
                            }))
                            .send()
                            .await?
                            .json::<serde_json::Value>()
                            .await
                    });
                    join_handle.await
                }).await;

                if let Ok(Ok(resp)) = poll_result {
                    if let Some(updates) = resp["result"].as_array() {
                        if !updates.is_empty() {
                            println!("[telegram_bind] Received {} updates", updates.len());
                        }
                        for update in updates {
                            if let Some(update_id) = update["update_id"].as_i64() {
                                offset = update_id + 1;
                            }
                            if let Some(msg_text) = update["message"]["text"].as_str() {
                                let msg_text_trimmed = msg_text.trim();
                                println!("[telegram_bind] Processing message: \"{}\" (Expected: \"{}\")", msg_text_trimmed, bind_code_for_poll);
                                if msg_text_trimmed.contains(&bind_code_for_poll) {
                                    println!("[telegram_bind] Match found! Binding chat_id...");
                                    if let Some(chat_id) = update["message"]["chat"]["id"].as_i64() {
                                        let chat_id_str = chat_id.to_string();
                                        println!("[telegram_bind] Found chat_id: {}", chat_id_str);

                                        // Send confirmation message to Telegram
                                        let token_for_send = token_clone.clone();
                                        let send_handle = tokio_handle.clone();
                                        let _ = cx.background_spawn(async move {
                                            let _ = send_handle.spawn(async move {
                                                println!("[telegram_bind] Sending confirmation message to Telegram...");
                                                let client = reqwest::Client::new();
                                                let url = format!("https://api.telegram.org/bot{}/sendMessage", token_for_send);
                                                let _ = client.post(&url)
                                                    .json(&serde_json::json!({
                                                        "chat_id": chat_id,
                                                        "text": "✅ 绑定成功！你可以开始使用远程控制功能了。\n\n尝试发送：/help",
                                                    }))
                                                    .send()
                                                    .await;
                                            }).await;
                                        });

                                        let _ = this.update(cx, |state, cx| {
                                            state.telegram_bind_status = format!("✅ 绑定成功！Chat ID: {}", chat_id_str);
                                            state.telegram_bind_error = false;

                                            let mut config = crate::services::load_config();
                                            config.telegram_bot_token = Some(token_clone.clone());
                                            config.telegram_chat_id = Some(chat_id_str);
                                            config.telegram_bound_at = Some(chrono::Local::now().to_rfc3339());
                                            let _ = crate::services::save_config(&config);
                                            println!("[telegram_bind] Config saved successfully.");

                                            // Hot-activate the Telegram trigger
                                            crate::triggers::telegram::TelegramTrigger::spawn_in_background(&config);
                                            println!("[telegram_bind] Telegram trigger hot-activated.");

                                            cx.notify();
                                        });
                                        return;
                                    }
                                }
                            }
                        }
                    }
                } else {
                    println!("[telegram_bind] Polling request failed: {:?}", poll_result);
                }
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(1))
                    .await;
            }
        })
        .detach();
    }

    pub(crate) fn handle_telegram_unbind(&mut self, cx: &mut Context<Self>) {
        // 停止当前正在运行的 trigger 实例
        crate::triggers::telegram::TelegramTrigger::stop_all();
        // 清除 config 中的 Telegram 配置
        let mut config = crate::services::load_config();
        config.telegram_bot_token = None;
        config.telegram_chat_id = None;
        config.telegram_bound_at = None;
        if let Err(e) = crate::services::save_config(&config) {
            eprintln!("Failed to save config: {}", e);
        }
        self.telegram_bind_token.clear();
        self.telegram_bind_status = "已解除绑定".to_string();
        self.telegram_bind_error = false;
        cx.notify();
    }

    pub(crate) fn handle_new_workspace_click(&mut self, cx: &mut Context<Self>) {
        if let Some((path, name)) = util::pick_folder_dialog() {
            if let Some(existing_ws) = self.workspaces.iter().find(|w| w.path == path) {
                let ws_id = existing_ws.id;
                let task_id = existing_ws
                    .default_task_id
                    .filter(|id| existing_ws.tasks.iter().any(|task| task.id == *id))
                    .or_else(|| existing_ws.tasks.iter().find(|t| t.is_draft).map(|t| t.id))
                    .or_else(|| existing_ws.tasks.first().map(|t| t.id));
                self.select_task(ws_id, task_id);
                cx.notify();
            } else {
                self.add_workspace(path, name);
                if let Some(ws_id) = self.active_workspace_id {
                    let task_id = self.ensure_workspace_draft_task(ws_id);
                    self.select_task(ws_id, task_id);
                    cx.notify();
                }
            }
        }
    }

    pub(crate) fn ensure_default_workspace(&mut self) {
        if self.workspaces.is_empty() {
            let default_ws = Workspace {
                id: 1,
                name: "Default".to_string(),
                path: self.default_work_dir.clone(),
                tasks: vec![],
                expanded: true,
                default_task_id: None,
            };
            self.workspaces.push(default_ws);
        }
    }
}
