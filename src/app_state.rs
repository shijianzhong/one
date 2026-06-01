use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use gpui::{Context, Pixels, Point, ScrollHandle, Window};

use crate::agents;
use crate::agents::types::{
    ClaudeRunPanelState, PreviewLaunchResult, RequestKind, SubagentMessageState,
};

/// A question from Claude Code waiting for user interaction
#[derive(Debug, Clone)]
pub(crate) struct PendingClaudeQuestion {
    pub prompt: String,
    pub options: Vec<String>,
    /// Which sub-agent run_id this question belongs to (for answer routing)
    pub source_run_id: u64,
    /// session_id for continue_claude_with_answer routing
    pub session_id: Option<String>,
}

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
    CancelModelConfig, ExportChat, OpenModelConfigDialog, SaveModelConfig, ToggleLang, ToggleTheme,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MainView {
    Chat,
    SkillsMarket,
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
    pub(crate) messages: Vec<ChatMessage>,
    pub(crate) chat_scroll_handle: ScrollHandle,
    pub(crate) needs_auto_scroll: bool,
    pub(crate) pending_summarize: bool,
    pub(crate) next_summarize_job_id: u64,
    pub(crate) summarize_job_id: Option<u64>,
    pub(crate) sandbox_backend: Backend,
    pub(crate) hovered_workspace_id: Option<usize>,
    pub(crate) delete_confirm_workspace_id: Option<usize>,
    pub(crate) popup_position: Point<Pixels>,
    pub(crate) terminal_output: Vec<TerminalLine>,
    pub(crate) current_claude_run: Option<ClaudeRunPanelState>,
    pub(crate) preview_process: Option<PreviewProcessHandle>,
    pub(crate) next_claude_run_id: u64,
    pub(crate) request_in_flight: bool,
    pub(crate) request_status_text: Option<String>,
    pub(crate) request_kind: Option<RequestKind>,
    pub(crate) think_collapsed: HashMap<String, bool>,
    pub(crate) next_general_ai_run_id: u64,
    pub(crate) general_ai_run_id: Option<u64>,
    pub(crate) general_ai_task_id: Option<usize>,
    pub(crate) general_ai_live_text: String,
    pub(crate) general_ai_show_live_bubble: bool,
    pub(crate) titlebar_should_move: bool,
    pub(crate) pending_confirmation_tools: Option<(Vec<system_tools::Tool>, String)>,
    pub(crate) intent_router: agents::intent_router::IntentRouter,
    pub(crate) subagent_messages: HashMap<u64, SubagentMessageState>,
    /// Maps orchestrator agent_id -> subagent card run_id for live stream routing
    pub(crate) orchestrator_agent_run_map: HashMap<String, u64>,
    /// Active Claude Code question waiting for user answer (from any path)
    pub(crate) pending_claude_question: Option<PendingClaudeQuestion>,
}

#[derive(Debug, Clone)]
pub(crate) struct TerminalLine {
    pub(crate) command: Option<String>,
    pub(crate) output: String,
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

    pub(crate) fn get_active_plan_steps(&self) -> Vec<String> {
        self.subagent_messages
            .values()
            .filter(|s| s.task_id == self.active_task_id)
            .flat_map(|s| s.events.iter().map(|e| format!("[{}] {}", e.title, e.detail)))
            .take(30)
            .collect()
    }

    pub(crate) fn begin_general_ai_run(&mut self) -> u64 {
        self.next_general_ai_run_id += 1;
        let run_id = self.next_general_ai_run_id;
        self.request_in_flight = true;
        self.request_status_text =
            Some(t(self.current_lang, Translations::WAITING_FOR_AI_RESPONSE).to_string());
        self.request_kind = Some(RequestKind::GeneralAi);
        self.general_ai_run_id = Some(run_id);
        self.general_ai_task_id = self.active_task_id;
        self.general_ai_live_text.clear();
        self.general_ai_show_live_bubble = true;
        run_id
    }
}

impl AppState {
    pub(crate) fn new(_window: &mut Window, _cx: &mut Context<Self>, config: Config) -> Self {
        let db = task_db::Database::new().expect("Failed to initialize database");
        let theme_mode = config.theme_mode;
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
                        })
                        .collect();
                    Workspace {
                        id: w.id,
                        name: w.name,
                        path: PathBuf::from(w.path),
                        tasks,
                        expanded: w.expanded,
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
            right_panel_width: 500.0,
            right_panel_resize_initial_mouse_x: None,
            right_panel_resize_initial_width: None,
            main_view: MainView::Chat,
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
            messages: vec![],
            needs_auto_scroll: false,
            pending_summarize: false,
            next_summarize_job_id: 0,
            summarize_job_id: None,
            chat_scroll_handle: ScrollHandle::default(),
            sandbox_backend: futures::executor::block_on(Backend::detect()),
            terminal_output: vec![],
            current_claude_run: None,
            preview_process: None,
            next_claude_run_id: 0,
            request_in_flight: false,
            request_status_text: None,
            request_kind: None,
            hovered_workspace_id: None,
            delete_confirm_workspace_id: None,
            popup_position: Point::default(),
            think_collapsed: HashMap::new(),
            next_general_ai_run_id: 0,
            general_ai_run_id: None,
            general_ai_task_id: None,
            general_ai_live_text: String::new(),
            general_ai_show_live_bubble: false,
            titlebar_should_move: false,
            pending_confirmation_tools: None,
            intent_router: agents::intent_router::IntentRouter::new(),
            subagent_messages: HashMap::new(),
            orchestrator_agent_run_map: HashMap::new(),
            pending_claude_question: None,
        };

        if state.workspaces.is_empty() {
            let default_ws = Workspace {
                id: 1,
                name: "Default".to_string(),
                path: state.default_work_dir.clone(),
                tasks: vec![],
                expanded: true,
            };
            state.workspaces.push(default_ws);
        }

        state
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
        let entry = if let Some(found) = hints.iter().find_map(|hint| {
            let hint_lower = hint.to_ascii_lowercase();
            html_files.iter().find(|file| {
                file.file_name()
                    .and_then(|n| n.to_str())
                    .map(|name| name.to_ascii_lowercase() == hint_lower)
                    .unwrap_or(false)
                    || file
                        .to_string_lossy()
                        .to_ascii_lowercase()
                        .ends_with(&hint_lower)
            })
        }) {
            found.clone()
        } else if let Some(index_file) = html_files.iter().find(|file| {
            file.file_name()
                .and_then(|n| n.to_str())
                .map(|name| name.eq_ignore_ascii_case("index.html"))
                .unwrap_or(false)
        }) {
            index_file.clone()
        } else {
            html_files[0].clone()
        };

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

        let config = Config {
            model_base_url: self.model_base_url.clone(),
            model_api_key: self.model_api_key.clone(),
            model_name: self.model_name.clone(),
            light_model: None,
            coding_model: None,
            system_model: None,
            lang: self.current_lang,
            theme_mode: self.theme_mode,
        };
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
        let config = Config {
            model_base_url: self.model_base_url.clone(),
            model_api_key: self.model_api_key.clone(),
            model_name: self.model_name.clone(),
            light_model: None,
            coding_model: None,
            system_model: None,
            lang: self.current_lang,
            theme_mode: self.theme_mode,
        };
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
        let config = Config {
            model_base_url: self.model_base_url.clone(),
            model_api_key: self.model_api_key.clone(),
            model_name: self.model_name.clone(),
            light_model: None,
            coding_model: None,
            system_model: None,
            lang: self.current_lang,
            theme_mode: self.theme_mode,
        };
        if let Err(e) = save_config(&config) {
            eprintln!("Failed to save theme config: {}", e);
        }
        cx.notify();
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
    pub(crate) fn handle_new_workspace_click(&mut self, cx: &mut Context<Self>) {
        if let Some((path, name)) = util::pick_folder_dialog() {
            if let Some(existing_ws) = self.workspaces.iter().find(|w| w.path == path) {
                self.active_workspace_id = Some(existing_ws.id);
                self.active_task_id = existing_ws
                    .tasks
                    .iter()
                    .find(|t| t.is_draft)
                    .map(|t| t.id)
                    .or_else(|| existing_ws.tasks.first().map(|t| t.id));
                self.restore_task_context();
                cx.notify();
            } else {
                self.add_workspace(path, name);
                if let Some(ws_id) = self.active_workspace_id {
                    self.active_task_id = self.ensure_workspace_draft_task(ws_id);
                    self.restore_task_context();
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
            };
            self.workspaces.push(default_ws);
        }
    }
}