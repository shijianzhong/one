use gpui::{
    svg,
    App, AppContext as _, Bounds, Context, DragMoveEvent,
    Hsla, IntoElement, ParentElement, Point, Pixels, px, size, Render,
    Styled, StatefulInteractiveElement, Window, WindowOptions, WindowBounds, div, prelude::*,
    Focusable, ScrollHandle,
};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::mpsc::{self, TryRecvError};
use std::time::Duration;
use dirs;

use gpui_platform::application;
use editor::Editor;
use menu::Confirm;
use crate::services::summarize_conversation_sync;
use settings::{KeymapFile, DEFAULT_KEYMAP_PATH};
use theme;
use theme_settings;

use gpui::FontWeight;

mod i18n;
mod memory;
mod assets;
mod sandbox;
mod services;
mod task_db;
mod agents;

use i18n::{t, Lang, Translations};
use memory::types::ChatMessage;
use sandbox::backend::{Backend, SandboxBackend};
use services::{Config, load_config, save_config};
use services::api::call_chat_api_sync;
use agents::claude_code::ClaudeStreamEvent;
use agents::router::AgentRouter;

struct DraggedResizer;

impl Render for DraggedResizer {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size(px(0.0)).into_element()
    }
}

gpui::actions!(
    app,
    [
        OpenModelConfigDialog,
        SaveModelConfig,
        CancelModelConfig,
        SendMessage,
        ToggleLang,
        ExportChat,
        OpenRegister,
        SubmitRegister,
        CancelRegister,
    ]
);

const NAV_BG: Hsla = Hsla { h: 0.0, s: 0.0, l: 0.98, a: 1.0 };
const CARD_BG: Hsla = Hsla { h: 0.0, s: 0.0, l: 1.0, a: 1.0 };
const PRIMARY_TEXT: Hsla = Hsla { h: 0.0, s: 0.0, l: 0.07, a: 1.0 };
const SECONDARY_TEXT: Hsla = Hsla { h: 0.0, s: 0.03, l: 0.35, a: 1.0 };
const TERTIARY_TEXT: Hsla = Hsla { h: 0.0, s: 0.02, l: 0.60, a: 1.0 };
const MUTED_TEXT: Hsla = Hsla { h: 0.0, s: 0.02, l: 0.55, a: 1.0 };
const BRAND_BLUE: Hsla = Hsla { h: 0.62, s: 1.0, l: 0.52, a: 1.0 };
const BORDER_LIGHT: Hsla = Hsla { h: 0.0, s: 0.03, l: 0.90, a: 1.0 };
const ACTIVE_BG: Hsla = Hsla { h: 0.62, s: 0.3, l: 0.95, a: 1.0 };
const WORKSPACE_BG: Hsla = Hsla { h: 0.0, s: 0.0, l: 0.96, a: 1.0 };

const NAV_WIDTH: f32 = 240.0;
const DEFAULT_WINDOW_WIDTH: f32 = 1200.0;
const DEFAULT_WINDOW_HEIGHT: f32 = 760.0;

struct AppState {
    db: task_db::Database,
    workspaces: Vec<Workspace>,
    active_workspace_id: Option<usize>,
    active_task_id: Option<usize>,
    default_work_dir: PathBuf,
    sidebar_visible: bool,
    terminal_visible: bool,
    terminal_width: f32,
    terminal_resize_initial_mouse_x: Option<f32>,
    terminal_resize_initial_width: Option<f32>,
    show_model_config_dialog: bool,
    show_export_dialog: bool,
    exported_json: Option<String>,
    exported_md: Option<String>,
    // Register dialog
    show_register_dialog: bool,
    editing_username: String,
    editing_email: String,
    editing_password: String,
    model_base_url: String,
    model_api_key: String,
    model_name: String,
    current_lang: Lang,
    editing_model_name: String,
    editing_base_url: String,
    editing_api_key: String,
    messages: Vec<ChatMessage>,
    chat_scroll_handle: ScrollHandle,
    needs_auto_scroll: bool,
    pending_summarize: bool,
    sandbox_backend: Backend,
    hovered_workspace_id: Option<usize>,
    delete_confirm_workspace_id: Option<usize>,
    popup_position: Point<Pixels>,
    // Terminal state
    terminal_output: Vec<TerminalLine>,
    current_claude_run: Option<ClaudeRunPanelState>,
    preview_process: Option<PreviewProcessHandle>,
    next_claude_run_id: u64,
    request_in_flight: bool,
    request_status_text: Option<String>,
    request_kind: Option<RequestKind>,
    // Agent system
    agent_router: AgentRouter,
}

#[derive(Debug, Clone)]
struct TerminalLine {
    command: Option<String>,
    output: String,
}

#[derive(Debug, Clone)]
enum RequestKind {
    GeneralAi,
    ClaudeCode,
}

#[derive(Debug, Clone)]
enum ClaudeRunStatus {
    Running,
    Completed,
    Failed,
}

impl ClaudeRunStatus {
    fn label(&self) -> &'static str {
        match self {
            Self::Running => "Running",
            Self::Completed => "Completed",
            Self::Failed => "Failed",
        }
    }

    fn color(&self) -> Hsla {
        match self {
            Self::Running => BRAND_BLUE,
            Self::Completed => Hsla { h: 0.36, s: 0.65, l: 0.42, a: 1.0 },
            Self::Failed => Hsla { h: 0.0, s: 0.72, l: 0.52, a: 1.0 },
        }
    }
}

#[derive(Debug, Clone)]
enum ClaudeRunTone {
    Info,
    Success,
    Error,
}

impl ClaudeRunTone {
    fn color(&self) -> Hsla {
        match self {
            Self::Info => SECONDARY_TEXT,
            Self::Success => Hsla { h: 0.36, s: 0.65, l: 0.42, a: 1.0 },
            Self::Error => Hsla { h: 0.0, s: 0.72, l: 0.52, a: 1.0 },
        }
    }
}

#[derive(Debug, Clone)]
enum FormattedContent {
    Plain(String),
    Json(String),
    Code(String),
}

#[derive(Debug, Clone)]
struct ClaudeRunEvent {
    title: String,
    tone: ClaudeRunTone,
    formatted_detail: FormattedContent,
}

impl ClaudeRunEvent {
    fn info(title: impl Into<String>, detail: impl Into<String>) -> Self {
        let detail = detail.into();
        Self {
            title: title.into(),
            formatted_detail: format_event_detail(&detail),
            tone: ClaudeRunTone::Info,
        }
    }

    fn success(title: impl Into<String>, detail: impl Into<String>) -> Self {
        let detail = detail.into();
        Self {
            title: title.into(),
            formatted_detail: format_event_detail(&detail),
            tone: ClaudeRunTone::Success,
        }
    }

    fn error(title: impl Into<String>, detail: impl Into<String>) -> Self {
        let detail = detail.into();
        Self {
            title: title.into(),
            formatted_detail: format_event_detail(&detail),
            tone: ClaudeRunTone::Error,
        }
    }
}

#[derive(Debug, Clone)]
struct ClaudeRunPanelState {
    run_id: u64,
    task_id: Option<usize>,
    instruction: String,
    work_dir: String,
    command_preview: String,
    status: ClaudeRunStatus,
    status_message: String,
    live_text: String,
    final_text: Option<String>,
    stderr_lines: Vec<String>,
    events: Vec<ClaudeRunEvent>,
    show_live_bubble: bool,
    preview: Option<PreviewState>,
}

#[derive(Debug)]
struct PreviewProcessHandle {
    child: Child,
}

#[derive(Debug, Clone)]
enum PreviewStatus {
    Idle,
    Ready,
    Failed,
}

impl PreviewStatus {
    fn label(&self) -> &'static str {
        match self {
            Self::Idle => "Idle",
            Self::Ready => "Ready",
            Self::Failed => "Failed",
        }
    }

    fn color(&self) -> Hsla {
        match self {
            Self::Idle => MUTED_TEXT,
            Self::Ready => Hsla { h: 0.36, s: 0.65, l: 0.42, a: 1.0 },
            Self::Failed => Hsla { h: 0.0, s: 0.72, l: 0.52, a: 1.0 },
        }
    }
}

#[derive(Debug, Clone)]
struct PreviewState {
    status: PreviewStatus,
    entry_file: Option<String>,
    url: Option<String>,
    note: String,
}

#[derive(Debug, Clone)]
enum PreviewLaunchResult {
    Ready {
        url: String,
        entry_file: String,
        note: String,
    },
    NotFound {
        note: String,
    },
    Failed {
        note: String,
    },
}

fn format_event_detail(detail: &str) -> FormattedContent {
    let trimmed = detail.trim();

    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(trimmed) {
            if let Ok(pretty) = serde_json::to_string_pretty(&json) {
                return FormattedContent::Json(pretty);
            }
        }
    }

    let lower = trimmed.to_lowercase();
    let code_markers = [
        "<html",
        "<!doctype",
        "function ",
        "const ",
        "let ",
        "import ",
        "export ",
        "body {",
        "div {",
        "return (",
    ];
    if trimmed.contains('\n') && code_markers.iter().any(|marker| lower.contains(marker)) {
        return FormattedContent::Code(trimmed.to_string());
    }

    FormattedContent::Plain(detail.to_string())
}

fn render_formatted_content(
    content: &FormattedContent,
    plain_color: Hsla,
    block_color: Hsla,
) -> gpui::AnyElement {
    match content {
        FormattedContent::Plain(text) => div()
            .text_xs()
            .text_color(plain_color)
            .whitespace_normal()
            .child(text.clone())
            .into_any_element(),
        FormattedContent::Json(text) => div()
            .p_2()
            .rounded_md()
            .bg(Hsla { h: 0.0, s: 0.0, l: 0.98, a: 1.0 })
            .border_1()
            .border_color(BORDER_LIGHT)
            .child(
                div()
                    .text_xs()
                    .text_color(block_color)
                    .font_family("Menlo")
                    .whitespace_normal()
                    .child(text.clone())
            )
            .into_any_element(),
        FormattedContent::Code(text) => div()
            .p_2()
            .rounded_md()
            .bg(Hsla { h: 0.62, s: 0.15, l: 0.97, a: 1.0 })
            .border_1()
            .border_color(BORDER_LIGHT)
            .child(
                div()
                    .text_xs()
                    .text_color(block_color)
                    .font_family("Menlo")
                    .whitespace_normal()
                    .child(text.clone())
            )
            .into_any_element(),
    }
}

#[derive(Debug, Clone)]
struct ContentPart {
    text: String,
    is_think: bool,
}

fn strip_think_tags(content: &str) -> String {
    let mut result = content.to_string();
    while let Some(start) = result.find("<think>") {
        if let Some(end) = result[start..].find("</think>") {
            result = format!("{}{}", &result[..start], &result[start+end+"</think>".len()..]);
        } else {
            break;
        }
    }
    result
}

fn parse_think_content(content: &str) -> Vec<ContentPart> {
    let mut parts = Vec::new();
    let mut current_pos = 0;

    for cap in content.match_indices("<think>") {
        let start = cap.0;
        if current_pos < start {
            parts.push(ContentPart {
                text: content[current_pos..start].to_string(),
                is_think: false,
            });
        }

        if let Some(end) = content[start..].find("</think>") {
            let end = start + end + "</think>".len();
            let think_content = &content[start..end];
            parts.push(ContentPart {
                text: think_content.to_string(),
                is_think: true,
            });
            current_pos = end;
        }
    }

    if current_pos < content.len() {
        parts.push(ContentPart {
            text: content[current_pos..].to_string(),
            is_think: false,
        });
    }

    if parts.is_empty() {
        parts.push(ContentPart {
            text: content.to_string(),
            is_think: false,
        });
    }

    parts
}

#[derive(Debug, Clone)]
struct Workspace {
    id: usize,
    name: String,
    path: PathBuf,
    tasks: Vec<TaskItem>,
    expanded: bool,
}

#[derive(Debug, Clone)]
struct TaskItem {
    id: usize,
    title: String,
}

impl AppState {
    fn new(_window: &mut Window, _cx: &mut Context<Self>, config: Config) -> Self {
        let db = task_db::Database::new().expect("Failed to initialize database");

        // Load workspaces and tasks from database
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
            terminal_width: 500.0,
            terminal_resize_initial_mouse_x: None,
            terminal_resize_initial_width: None,
            show_model_config_dialog: false,
            show_export_dialog: false,
            exported_json: None,
            exported_md: None,
            // Register dialog
            show_register_dialog: false,
            editing_username: "".to_string(),
            editing_email: "".to_string(),
            editing_password: "".to_string(),
            model_base_url: config.model_base_url,
            model_api_key: config.model_api_key,
            model_name: config.model_name,
            current_lang: config.lang,
            editing_model_name: "gpt-4".to_string(),
            editing_base_url: "https://api.openai.com/v1".to_string(),
            editing_api_key: "".to_string(),
            messages: vec![],
            needs_auto_scroll: false,
            pending_summarize: false,
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
            agent_router: AgentRouter::new(),
        };

        // Ensure default workspace exists if no workspaces loaded
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

    fn get_active_workspace(&self) -> Option<&Workspace> {
        self.active_workspace_id.and_then(|id| self.workspaces.iter().find(|w| w.id == id))
    }

    fn get_active_task(&self) -> Option<&TaskItem> {
        self.get_active_workspace()
            .and_then(|w| w.tasks.iter().find(|t| Some(t.id) == self.active_task_id))
    }

    fn get_work_dir(&self) -> String {
        // 优先使用 active workspace 的真实路径
        if let Some(ws) = self.get_active_workspace() {
            return ws.path.to_string_lossy().to_string();
        }
        // fallback 到临时目录（只有当没有任何 workspace 时）
        if let Some(task_id) = self.active_task_id {
            format!("/tmp/one_task_{}", task_id)
        } else {
            self.default_work_dir.to_string_lossy().to_string()
        }
    }

    fn add_workspace(&mut self, path: PathBuf, name: String) {
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

    fn add_task_to_workspace(&mut self, workspace_id: usize, title: String, cx: &mut Context<Self>) {
        if let Some(workspace) = self.workspaces.iter_mut().find(|w| w.id == workspace_id) {
            let id = task_db::insert_task(&self.db.conn, workspace_id, &title)
                .unwrap_or(workspace.tasks.len() + 1);

            workspace.tasks.push(TaskItem {
                id,
                title,
            });
            self.active_task_id = Some(id);
            cx.notify();
        }
    }

    fn begin_claude_run(&mut self, instruction: &str) -> u64 {
        self.next_claude_run_id += 1;
        let run_id = self.next_claude_run_id;
        self.sidebar_visible = true;
        self.request_in_flight = true;
        self.request_status_text = Some("Claude Code is running...".to_string());
        self.request_kind = Some(RequestKind::ClaudeCode);
        self.current_claude_run = Some(ClaudeRunPanelState {
            run_id,
            task_id: self.active_task_id,
            instruction: instruction.to_string(),
            work_dir: self.get_work_dir(),
            command_preview: String::new(),
            status: ClaudeRunStatus::Running,
            status_message: "Waiting for Claude Code to start...".to_string(),
            live_text: String::new(),
            final_text: None,
            stderr_lines: vec![],
            events: vec![ClaudeRunEvent::info(
                "Run queued",
                format!("Instruction submitted: {}", instruction),
            )],
            show_live_bubble: true,
            preview: Some(PreviewState {
                status: PreviewStatus::Idle,
                entry_file: None,
                url: None,
                note: "Preview will be prepared after the run completes".to_string(),
            }),
        });
        run_id
    }

    fn stop_preview_process(&mut self) {
        if let Some(mut handle) = self.preview_process.take() {
            let _ = handle.child.kill();
            let _ = handle.child.wait();
        }
    }

    fn open_url_in_browser(&self, url: &str) {
        let _ = Command::new("open")
            .arg(url)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
    }

    fn collect_html_files(root: &std::path::Path) -> Vec<PathBuf> {
        fn walk(dir: &std::path::Path, out: &mut Vec<PathBuf>, depth: usize) {
            if depth > 4 {
                return;
            }
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        if [".git", "node_modules", "target"].contains(&name) {
                            continue;
                        }
                    }
                    walk(&path, out, depth + 1);
                } else if path
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.eq_ignore_ascii_case("html"))
                    .unwrap_or(false)
                {
                    out.push(path);
                }
            }
        }

        let mut out = Vec::new();
        walk(root, &mut out, 0);
        out
    }

    fn extract_html_hints(text: &str) -> Vec<String> {
        let mut hints = Vec::new();
        let mut token = String::new();

        for ch in text.chars() {
            if ch.is_ascii_alphanumeric() || ch == '.' || ch == '_' || ch == '-' || ch == '/' {
                token.push(ch);
            } else if !token.is_empty() {
                if token.to_ascii_lowercase().ends_with(".html") {
                    hints.push(token.clone());
                }
                token.clear();
            }
        }
        if !token.is_empty() && token.to_ascii_lowercase().ends_with(".html") {
            hints.push(token);
        }

        hints
    }

    fn try_prepare_preview(&mut self, work_dir: &str, hint_text: &str) -> PreviewLaunchResult {
        self.stop_preview_process();

        let root = PathBuf::from(work_dir);
        if !root.exists() {
            return PreviewLaunchResult::Failed {
                note: format!("Preview directory does not exist: {}", root.display()),
            };
        }

        let html_files = Self::collect_html_files(&root);
        if html_files.is_empty() {
            return PreviewLaunchResult::NotFound {
                note: "No previewable HTML file found in this workspace.".to_string(),
            };
        }

        let hints = Self::extract_html_hints(hint_text);
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
            .args(["-m", "http.server", &port.to_string(), "--bind", "127.0.0.1"])
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
                    note: format!("Serving workspace root: {}", serve_dir.display()),
                }
            }
            Err(error) => PreviewLaunchResult::Failed {
                note: format!(
                    "Failed to start preview server in {}: {}",
                    serve_dir.display(),
                    error
                ),
            },
        }
    }

    fn apply_claude_run_event(&mut self, run_id: u64, event: ClaudeStreamEvent) {
        let mut final_message: Option<String> = None;
        let mut persist_task_id: Option<usize> = None;
        let mut finished_work_dir: Option<String> = None;

        {
            let Some(run) = self.current_claude_run.as_mut() else {
                return;
            };

            if run.run_id != run_id {
                return;
            }

            match event {
                ClaudeStreamEvent::Started { command, workdir } => {
                    run.command_preview = command.clone();
                    run.work_dir = workdir.clone();
                    run.status_message = "Claude Code is running".to_string();
                    run.events.push(ClaudeRunEvent::info(
                        "Process started",
                        format!("Workdir: {}\nCommand: {}", workdir, command),
                    ));
                }
                ClaudeStreamEvent::AssistantText(text) => {
                    if run.live_text.is_empty() {
                        run.events.push(ClaudeRunEvent::info(
                            "Streaming response",
                            "Claude Code started returning live content",
                        ));
                    }
                    if !run.live_text.is_empty() {
                        run.live_text.push('\n');
                    }
                    run.live_text.push_str(&text);
                    run.status_message = "Generating response".to_string();
                }
                ClaudeStreamEvent::Progress { label, detail } => {
                    run.status_message = format!("{}...", label);
                    run.events.push(ClaudeRunEvent::info(label, detail));
                }
                ClaudeStreamEvent::Stderr(line) => {
                    run.stderr_lines.push(line.clone());
                    let tone = if line.to_lowercase().contains("error") {
                        ClaudeRunEvent::error("stderr", line)
                    } else {
                        ClaudeRunEvent::info("stderr", line)
                    };
                    run.events.push(tone);
                }
                ClaudeStreamEvent::Finished { result } => {
                    run.status = ClaudeRunStatus::Completed;
                    run.status_message = "Claude Code completed".to_string();
                    self.request_in_flight = false;
                    self.request_status_text = None;
                    self.request_kind = None;
                    if run.live_text.trim().is_empty() {
                        run.live_text = result.clone();
                    }
                    run.final_text = Some(result);
                    run.show_live_bubble = false;
                    run.events.push(ClaudeRunEvent::success(
                        "Run completed",
                        format!("Generated {} characters", run.live_text.chars().count()),
                    ));
                    final_message = Some(format!("[Claude Code]\n{}", run.live_text));
                    persist_task_id = run.task_id;
                    finished_work_dir = Some(run.work_dir.clone());
                }
                ClaudeStreamEvent::Failed { error } => {
                    run.status = ClaudeRunStatus::Failed;
                    run.status_message = "Claude Code failed".to_string();
                    self.request_in_flight = false;
                    self.request_status_text = None;
                    self.request_kind = None;
                    run.show_live_bubble = false;
                    run.events.push(ClaudeRunEvent::error("Run failed", error.clone()));
                    let mut message = String::from("Claude Code execution error: ");
                    message.push_str(&error);
                    if !run.live_text.trim().is_empty() {
                        message = format!(
                            "[Claude Code]\n{}\n\n[Run failed]\n{}",
                            run.live_text, error
                        );
                    }
                    final_message = Some(message);
                    persist_task_id = run.task_id;
                }
            }
        }

        let mut auto_open_url: Option<String> = None;
        if let Some(work_dir) = finished_work_dir {
            let hint_text = if let Some(run) = self.current_claude_run.as_ref() {
                format!(
                    "{}\n{}\n{}",
                    run.instruction,
                    run.live_text,
                    run.final_text.clone().unwrap_or_default()
                )
            } else {
                String::new()
            };
            let preview_result = self.try_prepare_preview(&work_dir, &hint_text);
            if let Some(run) = self.current_claude_run.as_mut() {
                match preview_result {
                    PreviewLaunchResult::Ready {
                        url,
                        entry_file,
                        note,
                    } => {
                        run.preview = Some(PreviewState {
                            status: PreviewStatus::Ready,
                            entry_file: Some(entry_file.clone()),
                            url: Some(url.clone()),
                            note: note.clone(),
                        });
                        run.events.push(ClaudeRunEvent::success(
                            "Preview ready",
                            format!("{}\n{}", url, note),
                        ));
                        auto_open_url = Some(url);
                        run.events.push(ClaudeRunEvent::info(
                            "Browser opened",
                            "Opened preview URL in external browser",
                        ));
                    }
                    PreviewLaunchResult::NotFound { note } => {
                        run.preview = Some(PreviewState {
                            status: PreviewStatus::Idle,
                            entry_file: None,
                            url: None,
                            note: note.clone(),
                        });
                        run.events.push(ClaudeRunEvent::info("Preview skipped", note));
                    }
                    PreviewLaunchResult::Failed { note } => {
                        run.preview = Some(PreviewState {
                            status: PreviewStatus::Failed,
                            entry_file: None,
                            url: None,
                            note: note.clone(),
                        });
                        run.events.push(ClaudeRunEvent::error("Preview failed", note));
                    }
                }
            }
        }
        if let Some(url) = auto_open_url {
            self.open_url_in_browser(&url);
        }

        if let Some(message) = final_message {
            if persist_task_id == self.active_task_id {
                self.messages.push(ChatMessage {
                    role: "assistant".to_string(),
                    content: message.clone(),
                });
                self.needs_auto_scroll = true;
            }
            if let Some(task_id) = persist_task_id {
                task_db::insert_message(&self.db.conn, task_id, "assistant", &message).ok();
            }
        }
    }

    fn spawn_claude_code_run(
        &mut self,
        instruction: String,
        session_id: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let run_id = self.begin_claude_run(&instruction);
        let project_dir = std::path::PathBuf::from(self.get_work_dir());

        let (sender, receiver) = mpsc::channel::<ClaudeStreamEvent>();
        let worker_sender = sender.clone();
        let final_sender = sender.clone();
        let instruction_for_worker = instruction.clone();
        let session_id_for_worker = session_id.clone();
        let project_dir_for_worker = project_dir.clone();

        gpui_tokio::Tokio::spawn(cx, async move {
            let result = tokio::task::spawn_blocking(move || {
                agents::claude_code::ClaudeCodeAgent::execute_instruction_stream(
                    &project_dir_for_worker,
                    &instruction_for_worker,
                    session_id_for_worker.as_deref(),
                    worker_sender,
                )
            })
            .await;

            match result {
                Ok(Ok(output)) => {
                    let _ = final_sender.send(ClaudeStreamEvent::Finished { result: output });
                }
                Ok(Err(error)) => {
                    let _ = final_sender.send(ClaudeStreamEvent::Failed {
                        error: error.to_string(),
                    });
                }
                Err(error) => {
                    let _ = final_sender.send(ClaudeStreamEvent::Failed {
                        error: format!("Tokio join error: {}", error),
                    });
                }
            }
        })
        .detach();

        cx.spawn(async move |this, cx| {
            loop {
                let mut disconnected = false;

                loop {
                    match receiver.try_recv() {
                        Ok(event) => {
                            let _ = this.update(cx, |this, cx| {
                                this.apply_claude_run_event(run_id, event);
                                cx.notify();
                            });
                        }
                        Err(TryRecvError::Empty) => break,
                        Err(TryRecvError::Disconnected) => {
                            disconnected = true;
                            break;
                        }
                    }
                }

                if disconnected {
                    break;
                }

                cx.background_executor()
                    .timer(Duration::from_millis(60))
                    .await;
            }
        })
        .detach();
    }

    // Action handlers for model config dialog
    fn open_model_config_dialog(&mut self, _: &OpenModelConfigDialog, _: &mut Window, cx: &mut Context<Self>) {
        self.editing_model_name = self.model_name.clone();
        self.editing_base_url = self.model_base_url.clone();
        self.editing_api_key = self.model_api_key.clone();
        self.show_model_config_dialog = true;
        cx.notify();
    }

    fn save_model_config(&mut self, _: &SaveModelConfig, _: &mut Window, cx: &mut Context<Self>) {
        self.model_name = self.editing_model_name.clone();
        self.model_base_url = self.editing_base_url.clone();
        self.model_api_key = self.editing_api_key.clone();
        self.show_model_config_dialog = false;

        // Save config to file
        let config = Config {
            model_base_url: self.model_base_url.clone(),
            model_api_key: self.model_api_key.clone(),
            model_name: self.model_name.clone(),
            lang: self.current_lang,
        };
        if let Err(e) = save_config(&config) {
            eprintln!("Failed to save config: {}", e);
        }

        cx.notify();
    }

    fn cancel_model_config(&mut self, _: &CancelModelConfig, _: &mut Window, cx: &mut Context<Self>) {
        self.show_model_config_dialog = false;
        cx.notify();
    }

    fn toggle_lang(&mut self, _: &ToggleLang, _: &mut Window, cx: &mut Context<Self>) {
        self.current_lang = self.current_lang.toggle();
        let config = Config {
            model_base_url: self.model_base_url.clone(),
            model_api_key: self.model_api_key.clone(),
            model_name: self.model_name.clone(),
            lang: self.current_lang,
        };
        if let Err(e) = save_config(&config) {
            eprintln!("Failed to save lang config: {}", e);
        }
        cx.notify();
    }

    fn export_chat(&mut self, _: &ExportChat, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(task_id) = self.active_task_id {
            if let Some(task) = self.get_active_task() {
                let json = task_db::export_messages_json(&self.db.conn, task_id).unwrap_or_default();
                let md = task_db::export_messages_markdown(&self.db.conn, task_id, &task.title).unwrap_or_default();
                self.exported_json = Some(json);
                self.exported_md = Some(md);
                self.show_export_dialog = true;
                cx.notify();
            }
        }
    }

    fn open_register(&mut self, _: &OpenRegister, _: &mut Window, cx: &mut Context<Self>) {
        self.editing_username = "".to_string();
        self.editing_email = "".to_string();
        self.editing_password = "".to_string();
        self.show_register_dialog = true;
        cx.notify();
    }

    fn submit_register(&mut self, _: &SubmitRegister, _: &mut Window, cx: &mut Context<Self>) {
        // Handle registration logic here (e.g., API call, validation)
        // For now, just close the dialog
        self.show_register_dialog = false;
        cx.notify();
    }

    fn cancel_register(&mut self, _: &CancelRegister, _: &mut Window, cx: &mut Context<Self>) {
        self.show_register_dialog = false;
        cx.notify();
    }

}

impl Render for AppState {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let sidebar = if self.sidebar_visible {
            Some(self.render_sidebar(cx).into_any_element())
        } else {
            None
        };
        div()
            .flex()
            .size_full()
            .bg(CARD_BG)
            .child(self.render_nav(cx))
            .child(div().w(px(1.0)).bg(BORDER_LIGHT))
            .child(self.render_chat(_window, cx))
            .when_some(sidebar, |this, sidebar| {
                this.child(div().w(px(1.0)).bg(BORDER_LIGHT))
                    .child(sidebar)
            })
            .when(self.terminal_visible, |this| {
                this.child(self.render_terminal_resizer(cx))
                    .child(self.render_terminal(_window, cx))
            })
            .when(self.show_model_config_dialog, |this| {
                this.child(self.render_model_config_dialog(_window, cx))
            })
            .when(self.show_register_dialog, |this| {
                this.child(self.render_register_dialog(_window, cx))
            })
            .when(self.show_export_dialog, |this| {
                this.child(self.render_export_dialog(cx))
            })
            .when(self.delete_confirm_workspace_id.is_some(), |this| {
                this.child(self.render_workspace_popup(cx))
            })
    }
}

impl AppState {
    fn render_nav(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .w(px(NAV_WIDTH))
            .h_full()
            .bg(NAV_BG)
            .child(self.render_nav_header(cx))
            .child(div().h(px(1.0)).bg(BORDER_LIGHT))
            .child(self.render_nav_buttons(cx))
            .child(div().h(px(1.0)).bg(BORDER_LIGHT))
            .child(self.render_task_list(cx))
    }

    fn render_nav_header(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let lang = self.current_lang;
        div()
            .flex()
            .items_center()
            .h(px(40.0))
            .px_4()
            .child(div().text_base().text_color(PRIMARY_TEXT).font_weight(FontWeight::BOLD).child(t(lang, Translations::NAV_ONE)))
            .child(
                div()
                    .ml_3()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .text_xs()
                    .text_color(MUTED_TEXT)
                    .cursor_pointer()
                    .on_mouse_down(gpui::MouseButton::Left, cx.listener(|this, _: &gpui::MouseDownEvent, _window, cx| {
                        this.toggle_lang(&ToggleLang, _window, cx);
                    }))
                    .child(lang.label())
            )
    }

    fn render_nav_buttons(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let lang = self.current_lang;
        let mut nav = div()
            .flex()
            .flex_col()
            .gap_1()
            .p_2();

        nav = nav.child(self.make_nav_item(t(lang, Translations::NEW_WORKSPACE).to_string(), "⌘N".to_string(), cx));
        nav = nav.child(self.make_nav_item(t(lang, Translations::MODEL_CONFIG).to_string(), "⌘M".to_string(), cx));
        nav = nav.child(self.make_nav_register_item(cx));

        nav
    }

    fn make_nav_item(&mut self, label: String, shortcut: String, cx: &mut Context<Self>) -> impl IntoElement {
        let is_new_workspace = label == t(self.current_lang, Translations::NEW_WORKSPACE);
        let is_model_config = label == t(self.current_lang, Translations::MODEL_CONFIG);

        div()
            .flex()
            .items_center()
            .gap_3()
            .px_3()
            .py_2()
            .rounded_md()
            .cursor_pointer()
            .when(is_new_workspace, |this| {
                this.on_mouse_down(gpui::MouseButton::Left, cx.listener(|this, _: &gpui::MouseDownEvent, _window, cx| {
                    this.handle_new_workspace_click(cx);
                }))
            })
            .when(is_model_config, |this| {
                this.on_mouse_down(gpui::MouseButton::Left, cx.listener(|this, _: &gpui::MouseDownEvent, _window, cx| {
                    this.open_model_config_dialog(&OpenModelConfigDialog, _window, cx);
                }))
            })
            .child(div().text_sm().text_color(SECONDARY_TEXT).child(label))
            .child(div().text_xs().text_color(MUTED_TEXT).ml_auto().child(shortcut))
    }

    fn make_nav_register_item(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .gap_3()
            .px_3()
            .py_2()
            .rounded_md()
            .cursor_pointer()
            .on_mouse_down(gpui::MouseButton::Left, cx.listener(|this, _: &gpui::MouseDownEvent, _window, cx| {
                this.open_register(&OpenRegister, _window, cx);
            }))
            .child(div().text_sm().text_color(SECONDARY_TEXT).child("Register"))
            .child(div().text_xs().text_color(MUTED_TEXT).ml_auto().child("⌘R"))
    }

    fn pick_folder_dialog() -> Option<(PathBuf, String)> {
        use std::process::Command;
        let output = Command::new("osascript")
            .args(["-e", "POSIX path of (choose folder)"])
            .output()
            .ok()?;
        let path_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if path_str.is_empty() {
            return None;
        }
        let path = PathBuf::from(&path_str);
        let name = path.file_name()?.to_string_lossy().to_string();
        Some((path, name))
    }

    fn handle_new_workspace_click(&mut self, cx: &mut Context<Self>) {
        if let Some((path, name)) = Self::pick_folder_dialog() {
            // 检查是否已存在相同路径的 workspace
            if let Some(existing_ws) = self.workspaces.iter().find(|w| w.path == path) {
                // 已存在 → 查找是否有 "New Task"，有则定位，无则新建
                self.active_workspace_id = Some(existing_ws.id);
                if let Some(new_task) = existing_ws.tasks.iter().find(|t| t.title == "New Task") {
                    self.active_task_id = Some(new_task.id);
                } else {
                    self.active_task_id = None;
                    self.add_task_to_workspace(existing_ws.id, "New Task".to_string(), cx);
                }
            } else {
                // 不存在 → 创建新 workspace + New Task
                self.add_workspace(path, name);
                if let Some(ws_id) = self.active_workspace_id {
                    self.add_task_to_workspace(ws_id, "New Task".to_string(), cx);
                }
            }
        }
    }

    fn ensure_default_workspace(&mut self) {
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

    fn render_task_list(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        // Ensure default workspace exists before rendering
        self.ensure_default_workspace();

        let workspaces = self.workspaces.clone();
        let active_workspace_id = self.active_workspace_id;
        let active_task_id = self.active_task_id;

        let mut result = div()
            .flex()
            .flex_col()
            .flex_1()
            .p_3()
            .id("task-list")
            .overflow_scroll()
            .on_mouse_down(gpui::MouseButton::Left, cx.listener(move |this, _: &gpui::MouseDownEvent, _window, _cx| {
                // Close dropdown when clicking elsewhere
                this.delete_confirm_workspace_id = None;
            }));

        result = result.child(div().text_xs().text_color(MUTED_TEXT).mb_3().child("WORKSPACES"));

        for workspace in workspaces {
            let is_active_ws = active_workspace_id == Some(workspace.id);
            let ws_bg = if is_active_ws { CARD_BG } else { CARD_BG };
            let ws_id = workspace.id;

            // Workspace row - clicking toggles expand/collapse
            let ws_row = div()
                .flex()
                .items_center()
                .gap_2()
                .px_3()
                .py_2()
                .rounded_md()
                .bg(ws_bg)
                .cursor_pointer()
                .on_mouse_move(cx.listener(move |this, _: &gpui::MouseMoveEvent, _window, _cx| {
                    this.hovered_workspace_id = Some(ws_id);
                }))
                .on_mouse_down(gpui::MouseButton::Left, cx.listener(move |this, _: &gpui::MouseDownEvent, _window, _cx| {
                    this.active_workspace_id = Some(ws_id);
                    if let Some(ws) = this.workspaces.iter_mut().find(|w| w.id == ws_id) {
                        ws.expanded = !ws.expanded;
                        task_db::update_workspace_expanded(&this.db.conn, ws_id, ws.expanded).ok();
                    }
                }));

            // Workspace expand/collapse icon - visual only, click handled by ws_row
            let expand_btn = div()
                .text_base()
                .text_color(MUTED_TEXT)
                .px_1()
                .py_1()
                .size(px(16.0));
            // Add button (+)
            // Add button (+) - stops propagation so only adds task, doesn't toggle expand
            let add_btn = div()
                .text_base()
                .text_color(MUTED_TEXT)
                .px_1()
                .py_1()
                .cursor_pointer()
                .id(format!("add-btn-{}", ws_id))
                .on_mouse_down(gpui::MouseButton::Left, cx.listener(move |this, _: &gpui::MouseDownEvent, _window, cx| {
                    cx.stop_propagation();
                    this.active_workspace_id = Some(ws_id);
                    // Check if there's already a "New Task" in this workspace
                    if let Some(ws) = this.workspaces.iter().find(|w| w.id == ws_id) {
                        if let Some(new_task) = ws.tasks.iter().find(|t| t.title == "New Task") {
                            this.active_task_id = Some(new_task.id);
                            return;
                        }
                    }
                    // No New Task found, create one
                    this.add_task_to_workspace(ws_id, "New Task".to_string(), cx);
                }));

            let ws_label = workspace.name.clone();

            let more_btn = div()
                .id(format!("more-btn-{}", ws_id))
                .on_mouse_down(gpui::MouseButton::Left, cx.listener(move |this, event: &gpui::MouseDownEvent, _window: &mut Window, cx: &mut Context<Self>| {
                    cx.stop_propagation();
                    this.delete_confirm_workspace_id = Some(ws_id);
                    this.popup_position = event.position;
                }))
                .child(
                    svg()
                        .path("more.svg")
                        .size(px(16.0))
                        .flex_none()
                        .text_color(MUTED_TEXT),
                );

            let action_div = div().ml_auto().flex().items_center().gap_3()
                .child(more_btn)
                .child(add_btn.child("+"));

            result = result.child(
                ws_row.child(
                    if workspace.expanded {
                        expand_btn.child(
                            svg()
                                .path("expand.svg")
                                .size(px(16.0))
                                .flex_none()
                                .text_color(MUTED_TEXT)
                        )
                    } else {
                        expand_btn.child(
                            svg()
                                .path("fold.svg")
                                .size(px(16.0))
                                .flex_none()
                                .text_color(MUTED_TEXT)
                        )
                    }
                ).child(
                    svg()
                        .path("folder.svg")
                        .size(px(16.0))
                        .flex_none()
                        .text_color(MUTED_TEXT)
                ).child(
                    div().text_sm().ml_1().text_color(if is_active_ws { BRAND_BLUE } else { PRIMARY_TEXT }).child(ws_label)
                ).child(
                    div().ml_auto().flex().items_center().gap_3()
                        .child(action_div)
                )
            );

            // Tasks under workspace (if expanded) - each workspace's tasks are in their own scrollable container
            if workspace.expanded {
                let mut tasks_container = div()
                    .flex_col()
                    .ml_2();

                for task in &workspace.tasks {
                    let is_active_task = active_task_id == Some(task.id) && active_workspace_id == Some(workspace.id);

                    let mut task_div = div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .w_full()
                        .px_3()
                        .py_2()
                        .ml_2()
                        .rounded_md()
                        .cursor_pointer();

                    let task_id = task.id;
                    let ws_id = workspace.id;
                    let title_display = task.title.trim().to_string();

                    task_div = task_div
                        .on_mouse_down(gpui::MouseButton::Left, cx.listener(move |this, _: &gpui::MouseDownEvent, _window, cx| {
                            this.active_workspace_id = Some(ws_id);
                            this.active_task_id = Some(task_id);
                            // Load messages for the selected task
                            if let Some(tid) = this.active_task_id {
                                let msgs = task_db::load_messages(&this.db.conn, tid).unwrap_or_default();
                                this.messages = msgs.into_iter().map(|m| ChatMessage {
                                    role: m.role,
                                    content: m.content,
                                }).collect();
                            } else {
                                this.messages.clear();
                            }
                            cx.notify();
                        }));

                    tasks_container = tasks_container.child(
                        task_div.child(
                            div().flex_1().overflow_hidden().text_size(px(10.0)).text_color(if is_active_task { BRAND_BLUE } else { PRIMARY_TEXT }).text_ellipsis().child(title_display.clone())
                        ).child(
                            div()
                                .ml_auto()
                                .text_xs()
                                .text_color(MUTED_TEXT)
                                .cursor_pointer()
                                .on_mouse_down(gpui::MouseButton::Left, cx.listener(move |this, _: &gpui::MouseDownEvent, _window, cx| {
                                    cx.stop_propagation();
                                    if let Some(ws) = this.workspaces.iter_mut().find(|w| w.id == ws_id) {
                                        ws.tasks.retain(|t| t.id != task_id);
                                        task_db::delete_task(&this.db.conn, task_id).ok();
                                        if this.active_task_id == Some(task_id) {
                                            this.active_task_id = None;
                                        }
                                        cx.notify();
                                    }
                                }))
                                .child("×")
                        )
                    );
                }

                result = result.child(tasks_container);
            }
        }

        result
    }

    fn render_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title = self.get_active_task().map(|t| t.title.clone()).unwrap_or_else(|| "No task selected".to_string());
        let work_dir = self.get_work_dir();
        let sidebar_visible = self.sidebar_visible;
        let terminal_visible = self.terminal_visible;
        let scroll_handle = self.chat_scroll_handle.clone();

        div()
            .flex()
            .flex_col()
            .flex_1()
            .h_full()
            .min_w(px(350.0))
            .child(self.render_chat_header(title, work_dir, sidebar_visible, terminal_visible, cx))
            .child(
                div()
                    .id("chat_container")
                    .flex_1()
                    .w_full()
                    .overflow_scroll()
                    .track_scroll(&scroll_handle)
                    .p_4()
                    .child(self.render_chat_messages(&scroll_handle, window, cx))
            )
            .child(div().h(px(1.0)).bg(BORDER_LIGHT))
            .child(self.render_composer(window, cx))
    }

    fn render_chat_header(&mut self, title: String, work_dir: String, sidebar_visible: bool, terminal_visible: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let lang = self.current_lang;
        div()
            .flex()
            .items_center()
            .justify_between()
            .h(px(40.0))
            .px_4()
            .bg(NAV_BG)
            .child(div().text_base().text_color(PRIMARY_TEXT).child(title))
            .child(
                div()
                    .flex()
                    .gap_3()
                    .items_center()
                    .child(
                        div()
                            .text_xs()
                            .text_color(TERTIARY_TEXT)
                            .child(format!("📁 {}", work_dir))
                    )
                    .child(
                        div()
                            .id("export-btn")
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .cursor_pointer()
                            .text_xs()
                            .text_color(MUTED_TEXT)
                            .on_mouse_down(gpui::MouseButton::Left, cx.listener(|this, _: &gpui::MouseDownEvent, _window, cx| {
                                this.export_chat(&ExportChat, _window, cx);
                            }))
                            .child(t(lang, Translations::EXPORT))
                    )
                    .child(
                        div()
                            .id("terminal-toggle")
                            .flex()
                            .items_center()
                            .gap_1()
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .cursor_pointer()
                            .bg(if terminal_visible { Hsla { h: 0.0, s: 0.0, l: 0.85, a: 1.0 } } else { CARD_BG })
                            .on_click(cx.listener(|this, _event, _window, _cx| {
                                this.terminal_visible = !this.terminal_visible;
                            }))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(if terminal_visible { BRAND_BLUE } else { MUTED_TEXT })
                                    .child(">_")
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(if terminal_visible { MUTED_TEXT } else { CARD_BG })
                                    .child("×")
                            )
                    )
                    .child(
                        div()
                            .id("sidebar-toggle")
                            .flex()
                            .items_center()
                            .gap_1()
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .cursor_pointer()
                            .bg(if sidebar_visible { Hsla { h: 0.0, s: 0.0, l: 0.85, a: 1.0 } } else { CARD_BG })
                            .on_click(cx.listener(|this, _event, _window, _cx| {
                                this.sidebar_visible = !this.sidebar_visible;
                            }))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(if sidebar_visible { BRAND_BLUE } else { MUTED_TEXT })
                                    .child("☰")
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(if sidebar_visible { MUTED_TEXT } else { CARD_BG })
                                    .child("×")
                            )
                    )
            )
    }

    fn render_model_config_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let app = &mut *cx;

        let model_name_editor = window.use_keyed_state("model_name_editor", app, |window, cx| {
            let mut editor = Editor::single_line(window, cx);
            editor.set_text(self.editing_model_name.clone(), window, cx);
            editor
        });

        let base_url_editor = window.use_keyed_state("base_url_editor", app, |window, cx| {
            let mut editor = Editor::single_line(window, cx);
            editor.set_text(self.editing_base_url.clone(), window, cx);
            editor
        });

        let api_key_editor = window.use_keyed_state("api_key_editor", app, |window, cx| {
            let mut editor = Editor::single_line(window, cx);
            editor.set_placeholder_text("sk-...", window, cx);
            editor.set_text(self.editing_api_key.clone(), window, cx);
            editor
        });

        let model_name_focus = model_name_editor.read(cx).focus_handle(cx);
        let base_url_focus = base_url_editor.read(cx).focus_handle(cx);
        let api_key_focus = api_key_editor.read(cx).focus_handle(cx);

        let weak_model_name = model_name_editor.downgrade();
        let weak_base_url = base_url_editor.downgrade();
        let weak_api_key = api_key_editor.downgrade();

        // Overlay
        div()
            .absolute()
            .inset_0()
            .bg(gpui::hsla(0., 0., 0., 0.5))
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_down(gpui::MouseButton::Left, cx.listener(|this, _: &gpui::MouseDownEvent, _window, cx| {
                this.cancel_model_config(&CancelModelConfig, _window, cx);
            }))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .w(px(400.0))
                    .p_5()
                    .bg(CARD_BG)
                    .rounded_lg()
                    .border_1()
                    .border_color(BORDER_LIGHT)
                    .shadow_md()
                    .on_mouse_down(gpui::MouseButton::Left, cx.listener(|_, _: &gpui::MouseDownEvent, _window, _cx| {}))
                    .child(
                        div()
                            .text_base()
                            .text_color(PRIMARY_TEXT)
                            .font_weight(FontWeight::BOLD)
                            .child("Model Service Config")
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(div().text_sm().text_color(SECONDARY_TEXT).child("Model Name"))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .h(px(36.0))
                                    .px_3()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(BORDER_LIGHT)
                                    .bg(gpui::white())
                                    .track_focus(&model_name_focus)
                                    .child(model_name_editor.clone()),
                            )
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(div().text_sm().text_color(SECONDARY_TEXT).child("Base URL"))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .h(px(36.0))
                                    .px_3()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(BORDER_LIGHT)
                                    .bg(gpui::white())
                                    .track_focus(&base_url_focus)
                                    .child(base_url_editor.clone()),
                            )
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(div().text_sm().text_color(SECONDARY_TEXT).child("API Key"))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .h(px(36.0))
                                    .px_3()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(BORDER_LIGHT)
                                    .bg(gpui::white())
                                    .track_focus(&api_key_focus)
                                    .child(api_key_editor.clone()),
                            )
                    )
                    .child(
                        div()
                            .flex()
                            .gap_3()
                            .mt_2()
                            .child(
                                div()
                                    .flex_1()
                                    .h(px(36.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(BORDER_LIGHT)
                                    .cursor_pointer()
                                    .on_mouse_down(gpui::MouseButton::Left, cx.listener(|this, _: &gpui::MouseDownEvent, _window, cx| {
                                        this.cancel_model_config(&CancelModelConfig, _window, cx);
                                    }))
                                    .child(div().text_sm().text_color(PRIMARY_TEXT).child("Cancel"))
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .h(px(36.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_md()
                                    .bg(BRAND_BLUE)
                                    .cursor_pointer()
                                    .on_mouse_down(gpui::MouseButton::Left, cx.listener(move |this, _: &gpui::MouseDownEvent, _window, cx| {
                                        if let Some(editor) = weak_model_name.upgrade() {
                                            this.editing_model_name = editor.read_with(cx, |editor, cx| editor.text(cx)).trim().to_string();
                                        }
                                        if let Some(editor) = weak_base_url.upgrade() {
                                            this.editing_base_url = editor.read_with(cx, |editor, cx| editor.text(cx)).trim().to_string();
                                        }
                                        if let Some(editor) = weak_api_key.upgrade() {
                                            this.editing_api_key = editor.read_with(cx, |editor, cx| editor.text(cx)).trim().to_string();
                                        }
                                        this.save_model_config(&SaveModelConfig, _window, cx);
                                    }))
                                    .child(div().text_sm().text_color(gpui::white()).child("Save"))
                            )
                    )
            )
    }

    fn render_register_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let username_editor = window.use_keyed_state("register_username_editor", cx, |window, cx| {
            let mut editor = Editor::single_line(window, cx);
            editor.set_text(self.editing_username.clone(), window, cx);
            editor
        });

        let email_editor = window.use_keyed_state("register_email_editor", cx, |window, cx| {
            let mut editor = Editor::single_line(window, cx);
            editor.set_text(self.editing_email.clone(), window, cx);
            editor
        });

        let password_editor = window.use_keyed_state("register_password_editor", cx, |window, cx| {
            let mut editor = Editor::single_line(window, cx);
            editor.set_placeholder_text("••••••••", window, cx);
            editor.set_text(self.editing_password.clone(), window, cx);
            editor
        });

        let username_focus = username_editor.read(cx).focus_handle(cx);
        let email_focus = email_editor.read(cx).focus_handle(cx);
        let password_focus = password_editor.read(cx).focus_handle(cx);

        let weak_username = username_editor.downgrade();
        let weak_email = email_editor.downgrade();
        let weak_password = password_editor.downgrade();

        // Overlay
        div()
            .absolute()
            .inset_0()
            .bg(gpui::hsla(0., 0., 0., 0.5))
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_down(gpui::MouseButton::Left, cx.listener(|this, _: &gpui::MouseDownEvent, _window, cx| {
                this.cancel_register(&CancelRegister, _window, cx);
            }))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .w(px(400.0))
                    .p_5()
                    .bg(CARD_BG)
                    .rounded_lg()
                    .border_1()
                    .border_color(BORDER_LIGHT)
                    .shadow_md()
                    .on_mouse_down(gpui::MouseButton::Left, cx.listener(|_, _: &gpui::MouseDownEvent, _window, _cx| {}))
                    .child(
                        div()
                            .text_base()
                            .text_color(PRIMARY_TEXT)
                            .font_weight(FontWeight::BOLD)
                            .child("Create Account")
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(div().text_sm().text_color(SECONDARY_TEXT).child("Username"))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .h(px(36.0))
                                    .px_3()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(BORDER_LIGHT)
                                    .bg(gpui::white())
                                    .track_focus(&username_focus)
                                    .child(username_editor.clone()),
                            )
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(div().text_sm().text_color(SECONDARY_TEXT).child("Email"))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .h(px(36.0))
                                    .px_3()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(BORDER_LIGHT)
                                    .bg(gpui::white())
                                    .track_focus(&email_focus)
                                    .child(email_editor.clone()),
                            )
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(div().text_sm().text_color(SECONDARY_TEXT).child("Password"))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .h(px(36.0))
                                    .px_3()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(BORDER_LIGHT)
                                    .bg(gpui::white())
                                    .track_focus(&password_focus)
                                    .child(password_editor.clone()),
                            )
                    )
                    .child(
                        div()
                            .flex()
                            .gap_3()
                            .mt_2()
                            .child(
                                div()
                                    .flex_1()
                                    .h(px(36.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(BORDER_LIGHT)
                                    .cursor_pointer()
                                    .on_mouse_down(gpui::MouseButton::Left, cx.listener(|this, _: &gpui::MouseDownEvent, _window, cx| {
                                        this.cancel_register(&CancelRegister, _window, cx);
                                    }))
                                    .child(div().text_sm().text_color(PRIMARY_TEXT).child("Cancel"))
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .h(px(36.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_md()
                                    .bg(BRAND_BLUE)
                                    .cursor_pointer()
                                    .on_mouse_down(gpui::MouseButton::Left, cx.listener(move |this, _: &gpui::MouseDownEvent, _window, cx| {
                                        if let Some(editor) = weak_username.upgrade() {
                                            this.editing_username = editor.read_with(cx, |editor, cx| editor.text(cx)).trim().to_string();
                                        }
                                        if let Some(editor) = weak_email.upgrade() {
                                            this.editing_email = editor.read_with(cx, |editor, cx| editor.text(cx)).trim().to_string();
                                        }
                                        if let Some(editor) = weak_password.upgrade() {
                                            this.editing_password = editor.read_with(cx, |editor, cx| editor.text(cx)).trim().to_string();
                                        }
                                        this.submit_register(&SubmitRegister, _window, cx);
                                    }))
                                    .child(div().text_sm().text_color(gpui::white()).child("Register"))
                            )
                    )
            )
    }

    fn render_workspace_popup(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let ws_id = self.delete_confirm_workspace_id.unwrap_or(0);
        let pos = self.popup_position;
        div()
            .absolute()
            .left(pos.x)
            .top(pos.y)
            .on_mouse_down(gpui::MouseButton::Left, cx.listener(|this, _: &gpui::MouseDownEvent, _window, _cx| {
                this.delete_confirm_workspace_id = None;
            }))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .w(px(180.0))
                    .p_3()
                    .bg(CARD_BG)
                    .rounded_lg()
                    .border_1()
                    .border_color(BORDER_LIGHT)
                    .shadow_md()
                    .on_mouse_down(gpui::MouseButton::Left, cx.listener(|_, _: &gpui::MouseDownEvent, _window, _cx| {}))
                    .child(
                        div()
                            .px_3()
                            .py_2()
                            .text_sm()
                            .text_color(PRIMARY_TEXT)
                            .rounded_md()
                            .cursor_pointer()
                            .hover(|this| this.bg(ACTIVE_BG))
                            .on_mouse_down(gpui::MouseButton::Left, cx.listener(move |this, _: &gpui::MouseDownEvent, _window, cx| {
                                cx.stop_propagation();
                                this.active_workspace_id = Some(ws_id);
                                // Check if there's already a "New Task" in this workspace
                                if let Some(ws) = this.workspaces.iter().find(|w| w.id == ws_id) {
                                    if let Some(new_task) = ws.tasks.iter().find(|t| t.title == "New Task") {
                                        this.active_task_id = Some(new_task.id);
                                        this.delete_confirm_workspace_id = None;
                                        return;
                                    }
                                }
                                // No New Task found, create one
                                this.add_task_to_workspace(ws_id, "New Task".to_string(), cx);
                                this.delete_confirm_workspace_id = None;
                            }))
                            .child("添加新任务")
                    )
                    .child(
                        div()
                            .px_3()
                            .py_2()
                            .text_sm()
                            .text_color(PRIMARY_TEXT)
                            .rounded_md()
                            .cursor_pointer()
                            .hover(|this| this.bg(ACTIVE_BG))
                            .on_mouse_down(gpui::MouseButton::Left, cx.listener(move |this, _: &gpui::MouseDownEvent, _window, cx| {
                                cx.stop_propagation();
                                this.workspaces.retain(|w| w.id != ws_id);
                                task_db::delete_workspace(&this.db.conn, ws_id).ok();
                                if this.active_workspace_id == Some(ws_id) {
                                    this.active_workspace_id = None;
                                }
                                if this.active_task_id.is_some() {
                                    this.active_task_id = None;
                                }
                                this.delete_confirm_workspace_id = None;
                                cx.notify();
                            }))
                            .child("删除 Workspace")
                    )
            )
    }

    fn render_export_dialog(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let lang = self.current_lang;
        let json_content = self.exported_json.clone().unwrap_or_default();
        let md_content = self.exported_md.clone().unwrap_or_default();

        div()
            .absolute()
            .inset_0()
            .bg(gpui::hsla(0., 0., 0., 0.5))
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_down(gpui::MouseButton::Left, cx.listener(|this, _: &gpui::MouseDownEvent, _window, cx| {
                this.show_export_dialog = false;
                this.exported_json = None;
                this.exported_md = None;
                cx.notify();
            }))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .w(px(500.0))
                    .h(px(400.0))
                    .p_5()
                    .bg(CARD_BG)
                    .rounded_lg()
                    .border_1()
                    .border_color(BORDER_LIGHT)
                    .shadow_md()
                    .on_mouse_down(gpui::MouseButton::Left, cx.listener(|_, _: &gpui::MouseDownEvent, _window, _cx| {}))
                    .child(
                        div()
                            .text_base()
                            .text_color(PRIMARY_TEXT)
                            .font_weight(FontWeight::BOLD)
                            .child(t(lang, Translations::EXPORT))
                    )
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .h(px(200.0))
                                    .p_3()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(BORDER_LIGHT)
                                    .overflow_hidden()
                                    .text_xs()
                                    .text_color(PRIMARY_TEXT)
                                    .child(format!("JSON:\n{}", json_content))
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .h(px(200.0))
                                    .p_3()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(BORDER_LIGHT)
                                    .overflow_hidden()
                                    .text_xs()
                                    .text_color(PRIMARY_TEXT)
                                    .child(format!("Markdown:\n{}", md_content))
                            )
                    )
                    .child(
                        div()
                            .flex()
                            .gap_3()
                            .mt_2()
                            .child(
                                div()
                                    .flex_1()
                                    .h(px(36.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(BORDER_LIGHT)
                                    .cursor_pointer()
                                    .on_mouse_down(gpui::MouseButton::Left, cx.listener(|this, _: &gpui::MouseDownEvent, _window, cx| {
                                        if let Some(json) = this.exported_json.clone() {
                                            if let Some(path) = rfd::FileDialog::new()
                                                .set_title("Export JSON")
                                                .add_filter("JSON", &["json"])
                                                .save_file() {
                                                std::fs::write(&path, json).ok();
                                            }
                                        }
                                        this.show_export_dialog = false;
                                        this.exported_json = None;
                                        this.exported_md = None;
                                        cx.notify();
                                    }))
                                    .child("Save JSON")
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .h(px(36.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(BORDER_LIGHT)
                                    .cursor_pointer()
                                    .on_mouse_down(gpui::MouseButton::Left, cx.listener(|this, _: &gpui::MouseDownEvent, _window, cx| {
                                        if let Some(md) = this.exported_md.clone() {
                                            if let Some(path) = rfd::FileDialog::new()
                                                .set_title("Export Markdown")
                                                .add_filter("Markdown", &["md"])
                                                .save_file() {
                                                std::fs::write(&path, md).ok();
                                            }
                                        }
                                        this.show_export_dialog = false;
                                        this.exported_json = None;
                                        this.exported_md = None;
                                        cx.notify();
                                    }))
                                    .child("Save Markdown")
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .h(px(36.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_md()
                                    .bg(BRAND_BLUE)
                                    .cursor_pointer()
                                    .on_mouse_down(gpui::MouseButton::Left, cx.listener(|this, _: &gpui::MouseDownEvent, _window, _cx| {
                                        this.show_export_dialog = false;
                                        this.exported_json = None;
                                        this.exported_md = None;
                                    }))
                                    .child(t(lang, Translations::CANCEL))
                            )
                    )
            )
    }

    fn render_chat_messages(&mut self, scroll_handle: &ScrollHandle, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let messages = self.messages.clone();
        let live_run = self
            .current_claude_run
            .as_ref()
            .filter(|run| run.task_id == self.active_task_id && run.show_live_bubble)
            .cloned();
        let general_ai_pending = self.request_in_flight
            && matches!(self.request_kind, Some(RequestKind::GeneralAi))
            && live_run.is_none();
        let is_user = |role: &str| role == "user";
        let lang = self.current_lang;

        // Auto-scroll to bottom only when needs_auto_scroll is set
        if self.needs_auto_scroll && !messages.is_empty() {
            scroll_handle.scroll_to_bottom();
            self.needs_auto_scroll = false;
        }

        let mut message_list = div()
            .flex_col()
            .gap_5()
            .w_full()
            .children(messages.iter().map(|msg| {
                let is_user_msg = is_user(&msg.role);
                let bubble_bg = if is_user_msg {
                    Hsla { h: 0.58, s: 0.75, l: 0.45, a: 1.0 } // Blue bg for user
                } else {
                    Hsla { h: 0.0, s: 0.0, l: 0.95, a: 1.0 } // Light gray bg for assistant
                };
                let text_color = if is_user_msg { gpui::white() } else { Hsla { h: 0.0, s: 0.0, l: 0.15, a: 1.0 } };
                let avatar_icon = if is_user_msg { "👤" } else { "🤖" };
                let role_label = if is_user_msg { t(lang, Translations::YOU) } else { t(lang, Translations::ASSISTANT) };

                // Parse content for think tags
                let parts = parse_think_content(&msg.content);

                // User messages: right aligned, Assistant messages: left aligned
                let message_container = if is_user_msg {
                    div()
                        .flex()
                        .justify_end()
                        .w_full()
                        .mb_3()
                        .child(
                            div()
                                .flex_col()
                                .items_end()
                                .gap_2()
                                .p_4()
                                .rounded_2xl()
                                .bg(bubble_bg)
                                .max_w(px(520.0))
                                .min_w(px(35.0))
                                .child(
                                    div()
                                        .text_base()
                                        .text_color(text_color)
                                        .whitespace_normal()
                                        .child(msg.content.clone())
                                )
                        )
                } else {
                    div()
                        .flex_col()
                        .items_start()
                        .gap_2()
                        .w_full()
                        .mb_3()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(MUTED_TEXT)
                                        .child(avatar_icon)
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(MUTED_TEXT)
                                        .child(role_label)
                                )
                        )
                        .child(
                            div()
                                .flex_col()
                                .items_start()
                                .gap_2()
                                .p_4()
                                .rounded_2xl()
                                .bg(bubble_bg)
                                .max_w(px(520.0))
                                .min_w(px(35.0))
                                .w_full()
                                .children(parts.iter().map(|part| {
                                    if part.is_think {
                                        // Think content: small, muted, indented
                                        div()
                                            .pl_4()
                                            .py_1()
                                            .text_sm()
                                            .text_color(TERTIARY_TEXT)
                                            .whitespace_normal()
                                            .child(part.text.clone())
                                    } else {
                                        div()
                                            .text_base()
                                            .text_color(text_color)
                                            .whitespace_normal()
                                            .child(part.text.clone())
                                    }
                                }))
                        )
                };

                message_container
            }));

        if let Some(run) = live_run.as_ref() {
            message_list = message_list.child(self.render_claude_live_message(run));
        }

        if general_ai_pending {
            message_list = message_list.child(self.render_general_ai_pending_message());
        }

        message_list
    }

    fn render_claude_live_message(&self, run: &ClaudeRunPanelState) -> impl IntoElement {
        let preview = if run.live_text.trim().is_empty() {
            run.status_message.clone()
        } else {
            run.live_text.clone()
        };

        div()
            .flex_col()
            .items_start()
            .gap_2()
            .w_full()
            .mb_3()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().text_sm().text_color(MUTED_TEXT).child("🤖"))
                    .child(
                        div()
                            .text_xs()
                            .text_color(MUTED_TEXT)
                            .child(format!("Claude Code · {}", run.status.label()))
                    )
            )
            .child(
                div()
                    .flex_col()
                    .items_start()
                    .gap_2()
                    .p_4()
                    .rounded_2xl()
                    .bg(Hsla { h: 0.0, s: 0.0, l: 0.95, a: 1.0 })
                    .max_w(px(520.0))
                    .min_w(px(35.0))
                    .w_full()
                    .child(
                        div()
                            .text_xs()
                            .text_color(TERTIARY_TEXT)
                            .child(run.status_message.clone())
                    )
                    .child(
                        div()
                            .text_base()
                            .text_color(PRIMARY_TEXT)
                            .whitespace_normal()
                            .child(preview)
                    )
            )
    }

    fn render_general_ai_pending_message(&self) -> impl IntoElement {
        let status_text = self
            .request_status_text
            .clone()
            .unwrap_or_else(|| "AI is thinking...".to_string());

        div()
            .flex_col()
            .items_start()
            .gap_2()
            .w_full()
            .mb_3()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().text_sm().text_color(MUTED_TEXT).child("🤖"))
                    .child(
                        div()
                            .text_xs()
                            .text_color(MUTED_TEXT)
                            .child("Assistant")
                    )
            )
            .child(
                div()
                    .flex_col()
                    .items_start()
                    .gap_2()
                    .p_4()
                    .rounded_2xl()
                    .bg(Hsla { h: 0.0, s: 0.0, l: 0.95, a: 1.0 })
                    .max_w(px(520.0))
                    .min_w(px(35.0))
                    .w_full()
                    .child(
                        div()
                            .text_xs()
                            .text_color(TERTIARY_TEXT)
                            .child(status_text)
                    )
                    .child(
                        div()
                            .text_base()
                            .text_color(PRIMARY_TEXT)
                            .whitespace_normal()
                            .child("Thinking...")
                    )
            )
    }

    fn render_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let composer_editor = window.use_keyed_state("composer_editor", &mut *cx, |window, cx| {
            let mut editor = Editor::single_line(window, cx);
            editor.set_placeholder_text("Type a message...", window, cx);
            editor
        });

        let composer_focus = composer_editor.read(cx).focus_handle(cx);
        let weak_composer = composer_editor.downgrade();
        let weak_composer_for_action = weak_composer.clone();

        let request_in_flight = self.request_in_flight;
        let request_status_text = self.request_status_text.clone();
        let send_bg = if request_in_flight {
            Hsla { h: 0.0, s: 0.0, l: 0.78, a: 1.0 }
        } else {
            BRAND_BLUE
        };
        let send_label = if request_in_flight {
            "Sending..."
        } else {
            "Send"
        };

        div()
            .flex()
            .gap_3()
            .p_4()
            .bg(NAV_BG)
            .child(
                div()
                    .flex_1()
                    .px_4()
                    .py_3()
                    .rounded_lg()
                    .bg(CARD_BG)
                    .border_1()
                    .border_color(BORDER_LIGHT)
                    .track_focus(&composer_focus)
                    .on_action(cx.listener(move |this, _: &Confirm, _window, cx| {
                        if this.request_in_flight {
                            return;
                        }
                        if let Some(editor) = weak_composer_for_action.upgrade() {
                            let text = editor.read_with(cx, |editor, cx| editor.text(cx)).trim().to_string();
                            if !text.is_empty() {
                                let user_message = text.clone();
                                // Check if this is the first message
                                let is_first_message = this.messages.is_empty();
                                if is_first_message {
                                    this.pending_summarize = true;
                                }

                                // Route message to appropriate agent
                                let decision = this.agent_router.classify_intent(&user_message, &this.messages);
                                eprintln!("[ROUTER] Decision: {:?}", decision);

                                match decision {
                                    agents::types::RoutingDecision::ClaudeCode { instruction, session_id } => {
                                        // Execute via Claude Code
                                        eprintln!("[ROUTER] Routing to Claude Code");
                                        this.messages.push(ChatMessage {
                                            role: "user".to_string(),
                                            content: user_message.clone(),
                                        });
                                        if let Some(task_id) = this.active_task_id {
                                            task_db::insert_message(&this.db.conn, task_id, "user", &user_message).ok();
                                        }
                                        this.request_in_flight = true;
                                        this.request_status_text = Some("Claude Code is running...".to_string());
                                        this.request_kind = Some(RequestKind::ClaudeCode);
                                        this.needs_auto_scroll = true;
                                        editor.update(cx, |editor, cx| {
                                            editor.set_text("", _window, cx);
                                        });
                                        cx.notify();
                                        this.spawn_claude_code_run(instruction, session_id, cx);
                                    }
                                    _ => {
                                        // Default: send to general AI
                                        this.messages.push(ChatMessage {
                                            role: "user".to_string(),
                                            content: user_message.clone(),
                                        });
                                        // Save user message to database
                                        if let Some(task_id) = this.active_task_id {
                                            task_db::insert_message(&this.db.conn, task_id, "user", &user_message).ok();
                                        }
                                        this.request_in_flight = true;
                                        this.request_status_text = Some("Waiting for AI response...".to_string());
                                        this.request_kind = Some(RequestKind::GeneralAi);
                                        this.needs_auto_scroll = true;
                                        editor.update(cx, |editor, cx| {
                                            editor.set_text("", _window, cx);
                                        });
                                        cx.notify();

                                        // Use tokio runtime to spawn blocking task
                                        let base_url = this.model_base_url.clone();
                                        let api_key = this.model_api_key.clone();
                                        let model = this.model_name.clone();
                                        let messages = this.messages.clone();

                                        eprintln!("[DEBUG] Spawning tokio async task");

                                        cx.spawn(async move |this, cx| {
                                            eprintln!("[DEBUG] Inside cx.spawn");

                                            // Spawn async work on tokio runtime
                                            let result = gpui_tokio::Tokio::spawn(cx, async move {
                                                eprintln!("[DEBUG] Tokio task started");
                                                // Use spawn_blocking for the synchronous HTTP call
                                                tokio::task::spawn_blocking(move || {
                                                    eprintln!("[DEBUG] Thread started");
                                                    call_chat_api_sync(&base_url, &api_key, &model, &messages)
                                                }).await
                                            }).await;

                                            eprintln!("[DEBUG] Received result");

                                            match result {
                                                Ok(Ok(Ok(resp))) => {
                                                    eprintln!("[DEBUG] HTTP OK, updating UI");
                                                    let _ = this.update(cx, |this, cx| {
                                                        this.messages.push(ChatMessage {
                                                            role: "assistant".to_string(),
                                                            content: resp.clone(),
                                                        });
                                                        this.request_in_flight = false;
                                                        this.request_status_text = None;
                                                        this.request_kind = None;
                                                        // Save assistant message to database
                                                        if let Some(task_id) = this.active_task_id {
                                                            task_db::insert_message(&this.db.conn, task_id, "assistant", &resp).ok();
                                                        }
                                                        this.needs_auto_scroll = true;

                                                        // AI summarization: summarize and update task title
                                                        if this.pending_summarize {
                                                            this.pending_summarize = false;
                                                            let task_id = this.active_task_id;
                                                            let all_messages = this.messages.clone();
                                                            let db_conn = &this.db.conn;
                                                            let base_url = this.model_base_url.clone();
                                                            let api_key = this.model_api_key.clone();
                                                            let model = this.model_name.clone();
                                                            if let Some(tid) = task_id {
                                                                if let Ok(sum) = summarize_conversation_sync(&base_url, &api_key, &model, &all_messages) {
                                                                    let clean_sum = strip_think_tags(&sum);
                                                                    let short_title: String = clean_sum.chars().take(10).collect();
                                                                    task_db::update_task_title(db_conn, tid, &short_title).ok();
                                                                    for ws in &mut this.workspaces {
                                                                        for t in &mut ws.tasks {
                                                                            if t.id == tid {
                                                                                t.title = short_title.clone();
                                                                                break;
                                                                            }
                                                                        }
                                                                    }
                                                                }
                                                            }
                                                        }

                                                        cx.notify();
                                                    });
                                                    eprintln!("[DEBUG] UI updated");
                                                }
                                                Ok(Ok(Err(e))) => {
                                                    eprintln!("API error: {}", e);
                                                    let error_message = format!(
                                                        "AI request failed: {}\n\nPlease check network connectivity, Base URL, and API key.",
                                                        e
                                                    );
                                                    let _ = this.update(cx, |this, cx| {
                                                        this.request_in_flight = false;
                                                        this.request_status_text = None;
                                                        this.request_kind = None;
                                                        this.messages.push(ChatMessage {
                                                            role: "assistant".to_string(),
                                                            content: error_message.clone(),
                                                        });
                                                        this.needs_auto_scroll = true;
                                                        if let Some(task_id) = this.active_task_id {
                                                            task_db::insert_message(&this.db.conn, task_id, "assistant", &error_message).ok();
                                                        }
                                                        cx.notify();
                                                    });
                                                }
                                                Ok(Err(e)) => {
                                                    eprintln!("Spawn error: {:?}", e);
                                                    let error_message = format!("AI runtime spawn error: {}", e);
                                                    let _ = this.update(cx, |this, cx| {
                                                        this.request_in_flight = false;
                                                        this.request_status_text = None;
                                                        this.request_kind = None;
                                                        this.messages.push(ChatMessage {
                                                            role: "assistant".to_string(),
                                                            content: error_message.clone(),
                                                        });
                                                        this.needs_auto_scroll = true;
                                                        if let Some(task_id) = this.active_task_id {
                                                            task_db::insert_message(&this.db.conn, task_id, "assistant", &error_message).ok();
                                                        }
                                                        cx.notify();
                                                    });
                                                }
                                                Err(e) => {
                                                    eprintln!("Tokio error: {:?}", e);
                                                    let error_message = format!("AI runtime error: {}", e);
                                                    let _ = this.update(cx, |this, cx| {
                                                        this.request_in_flight = false;
                                                        this.request_status_text = None;
                                                        this.request_kind = None;
                                                        this.messages.push(ChatMessage {
                                                            role: "assistant".to_string(),
                                                            content: error_message.clone(),
                                                        });
                                                        this.needs_auto_scroll = true;
                                                        if let Some(task_id) = this.active_task_id {
                                                            task_db::insert_message(&this.db.conn, task_id, "assistant", &error_message).ok();
                                                        }
                                                        cx.notify();
                                                    });
                                                }
                                            }
                                        }).detach();
                                    }
                                }
                            }
                        }
                    }))
                    .child(composer_editor)
            )
            .child(
                div()
                    .px_5()
                    .py_3()
                    .rounded_lg()
                    .bg(send_bg)
                    .cursor_pointer()
                    .text_color(gpui::white())
                    .text_base()
                    .on_mouse_down(gpui::MouseButton::Left, cx.listener(move |this, _: &gpui::MouseDownEvent, _window, cx| {
                        if this.request_in_flight {
                            return;
                        }
                        if let Some(editor) = weak_composer.upgrade() {
                            let text = editor.read_with(cx, |editor, cx| editor.text(cx)).trim().to_string();
                            if !text.is_empty() {
                                let user_message = text.clone();
                                // Check if this is the first message
                                let is_first_message = this.messages.is_empty();
                                if is_first_message {
                                    this.pending_summarize = true;
                                }

                                // Route message to appropriate agent
                                let decision = this.agent_router.classify_intent(&user_message, &this.messages);
                                eprintln!("[ROUTER] Decision: {:?}", decision);

                                match decision {
                                    agents::types::RoutingDecision::ClaudeCode { instruction, session_id } => {
                                        // Execute via Claude Code
                                        eprintln!("[ROUTER] Routing to Claude Code");
                                        this.messages.push(ChatMessage {
                                            role: "user".to_string(),
                                            content: user_message.clone(),
                                        });
                                        if let Some(task_id) = this.active_task_id {
                                            task_db::insert_message(&this.db.conn, task_id, "user", &user_message).ok();
                                        }
                                        this.request_in_flight = true;
                                        this.request_status_text = Some("Claude Code is running...".to_string());
                                        this.request_kind = Some(RequestKind::ClaudeCode);
                                        this.needs_auto_scroll = true;
                                        editor.update(cx, |editor, cx| {
                                            editor.set_text("", _window, cx);
                                        });
                                        cx.notify();
                                        this.spawn_claude_code_run(instruction, session_id, cx);
                                    }
                                    _ => {
                                        // Default: send to general AI
                                        this.messages.push(ChatMessage {
                                            role: "user".to_string(),
                                            content: user_message.clone(),
                                        });
                                        // Save user message to database
                                        if let Some(task_id) = this.active_task_id {
                                            task_db::insert_message(&this.db.conn, task_id, "user", &user_message).ok();
                                        }
                                        this.request_in_flight = true;
                                        this.request_status_text = Some("Waiting for AI response...".to_string());
                                        this.request_kind = Some(RequestKind::GeneralAi);
                                        this.needs_auto_scroll = true;
                                        editor.update(cx, |editor, cx| {
                                            editor.set_text("", _window, cx);
                                        });
                                        cx.notify();

                                        // Use tokio runtime to spawn blocking task
                                        let base_url = this.model_base_url.clone();
                                        let api_key = this.model_api_key.clone();
                                        let model = this.model_name.clone();
                                        let messages = this.messages.clone();

                                        eprintln!("[DEBUG] Spawning tokio async task");

                                        cx.spawn(async move |this, cx| {
                                            eprintln!("[DEBUG] Inside cx.spawn");

                                            // Spawn async work on tokio runtime
                                            let result = gpui_tokio::Tokio::spawn(cx, async move {
                                                eprintln!("[DEBUG] Tokio task started");
                                                // Use spawn_blocking for the synchronous HTTP call
                                                tokio::task::spawn_blocking(move || {
                                                    eprintln!("[DEBUG] Thread started");
                                                    call_chat_api_sync(&base_url, &api_key, &model, &messages)
                                                }).await
                                            }).await;

                                            eprintln!("[DEBUG] Received result");

                                            match result {
                                                Ok(Ok(Ok(resp))) => {
                                                    eprintln!("[DEBUG] HTTP OK, updating UI");
                                                    let _ = this.update(cx, |this, cx| {
                                                        this.messages.push(ChatMessage {
                                                            role: "assistant".to_string(),
                                                            content: resp.clone(),
                                                        });
                                                        this.request_in_flight = false;
                                                        this.request_status_text = None;
                                                        this.request_kind = None;
                                                        // Save assistant message to database
                                                        if let Some(task_id) = this.active_task_id {
                                                            task_db::insert_message(&this.db.conn, task_id, "assistant", &resp).ok();
                                                        }
                                                        this.needs_auto_scroll = true;

                                                        // AI summarization: summarize and update task title
                                                        if this.pending_summarize {
                                                            this.pending_summarize = false;
                                                            let task_id = this.active_task_id;
                                                            let all_messages = this.messages.clone();
                                                            let db_conn = &this.db.conn;
                                                            let base_url = this.model_base_url.clone();
                                                            let api_key = this.model_api_key.clone();
                                                            let model = this.model_name.clone();
                                                            if let Some(tid) = task_id {
                                                                if let Ok(sum) = summarize_conversation_sync(&base_url, &api_key, &model, &all_messages) {
                                                                    let clean_sum = strip_think_tags(&sum);
                                                                    let short_title: String = clean_sum.chars().take(10).collect();
                                                                    task_db::update_task_title(db_conn, tid, &short_title).ok();
                                                                    for ws in &mut this.workspaces {
                                                                        for t in &mut ws.tasks {
                                                                            if t.id == tid {
                                                                                t.title = short_title.clone();
                                                                                break;
                                                                            }
                                                                        }
                                                                    }
                                                                }
                                                            }
                                                        }

                                                        cx.notify();
                                                    });
                                                    eprintln!("[DEBUG] UI updated");
                                                }
                                                Ok(Ok(Err(e))) => {
                                                    eprintln!("API error: {}", e);
                                                    let error_message = format!(
                                                        "AI request failed: {}\n\nPlease check network connectivity, Base URL, and API key.",
                                                        e
                                                    );
                                                    let _ = this.update(cx, |this, cx| {
                                                        this.request_in_flight = false;
                                                        this.request_status_text = None;
                                                        this.request_kind = None;
                                                        this.messages.push(ChatMessage {
                                                            role: "assistant".to_string(),
                                                            content: error_message.clone(),
                                                        });
                                                        this.needs_auto_scroll = true;
                                                        if let Some(task_id) = this.active_task_id {
                                                            task_db::insert_message(&this.db.conn, task_id, "assistant", &error_message).ok();
                                                        }
                                                        cx.notify();
                                                    });
                                                }
                                                Ok(Err(e)) => {
                                                    eprintln!("Spawn error: {:?}", e);
                                                    let error_message = format!("AI runtime spawn error: {}", e);
                                                    let _ = this.update(cx, |this, cx| {
                                                        this.request_in_flight = false;
                                                        this.request_status_text = None;
                                                        this.request_kind = None;
                                                        this.messages.push(ChatMessage {
                                                            role: "assistant".to_string(),
                                                            content: error_message.clone(),
                                                        });
                                                        this.needs_auto_scroll = true;
                                                        if let Some(task_id) = this.active_task_id {
                                                            task_db::insert_message(&this.db.conn, task_id, "assistant", &error_message).ok();
                                                        }
                                                        cx.notify();
                                                    });
                                                }
                                                Err(e) => {
                                                    eprintln!("Tokio error: {:?}", e);
                                                    let error_message = format!("AI runtime error: {}", e);
                                                    let _ = this.update(cx, |this, cx| {
                                                        this.request_in_flight = false;
                                                        this.request_status_text = None;
                                                        this.request_kind = None;
                                                        this.messages.push(ChatMessage {
                                                            role: "assistant".to_string(),
                                                            content: error_message.clone(),
                                                        });
                                                        this.needs_auto_scroll = true;
                                                        if let Some(task_id) = this.active_task_id {
                                                            task_db::insert_message(&this.db.conn, task_id, "assistant", &error_message).ok();
                                                        }
                                                        cx.notify();
                                                    });
                                                }
                                            }
                                        }).detach();
                                    }
                                }
                            }
                        }
                    }))
                    .child(send_label)
            )
            .when_some(request_status_text, |this, status| {
                this.child(
                    div()
                        .text_xs()
                        .text_color(MUTED_TEXT)
                        .child(status)
                )
            })
    }

    fn render_sidebar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let sidebar_bg = Hsla { h: 0.0, s: 0.0, l: 0.96, a: 1.0 };
        let run = self
            .current_claude_run
            .as_ref()
            .filter(|run| run.task_id == self.active_task_id)
            .cloned();

        let mut sidebar = div()
            .flex()
            .flex_col()
            .w(px(320.0))
            .h_full()
            .bg(sidebar_bg)
            .child(div().h(px(1.0)).bg(BORDER_LIGHT))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .h(px(44.0))
                    .px_4()
                    .bg(WORKSPACE_BG)
                    .child(
                        div()
                            .text_sm()
                            .text_color(PRIMARY_TEXT)
                            .font_weight(FontWeight::BOLD)
                            .child("Claude Code Run")
                    )
            );

        if let Some(run) = run {
            let status_color = run.status.color();
            let live_output = if let Some(final_text) = run.final_text.clone() {
                final_text
            } else if run.live_text.trim().is_empty() {
                run.status_message.clone()
            } else {
                run.live_text.clone()
            };
            let preview = run.preview.clone();
            let preview_label = preview
                .as_ref()
                .map(|preview| preview.status.label().to_string())
                .unwrap_or_else(|| "Idle".to_string());
            let preview_color = preview
                .as_ref()
                .map(|preview| preview.status.color())
                .unwrap_or(MUTED_TEXT);

            let mut timeline = div().flex().flex_col().gap_2();
            for event in run.events.iter().rev() {
                let detail_block =
                    render_formatted_content(&event.formatted_detail, SECONDARY_TEXT, PRIMARY_TEXT);
                timeline = timeline.child(
                    div()
                        .flex_col()
                        .gap_1()
                        .p_3()
                        .rounded_lg()
                        .bg(CARD_BG)
                        .border_1()
                        .border_color(BORDER_LIGHT)
                        .child(
                            div()
                                .text_xs()
                                .text_color(event.tone.color())
                                .font_weight(FontWeight::BOLD)
                                .child(event.title.clone())
                        )
                        .child(detail_block)
                );
            }

            let stderr_preview = if run.stderr_lines.is_empty() {
                "No stderr output".to_string()
            } else {
                run.stderr_lines
                    .iter()
                    .rev()
                    .take(8)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("\n")
            };

            sidebar = sidebar.child(
                div()
                    .id("claude-run-panel-content")
                    .overflow_scroll()
                    .flex_1()
                    .p_4()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(
                        div()
                            .flex_col()
                            .gap_2()
                            .p_3()
                            .rounded_lg()
                            .bg(CARD_BG)
                            .border_1()
                            .border_color(BORDER_LIGHT)
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(gpui::white())
                                            .px_2()
                                            .py_1()
                                            .rounded_md()
                                            .bg(status_color)
                                            .child(run.status.label())
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(MUTED_TEXT)
                                            .child(run.status_message.clone())
                                    )
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(PRIMARY_TEXT)
                                    .font_weight(FontWeight::BOLD)
                                    .whitespace_normal()
                                    .child(run.instruction.clone())
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(MUTED_TEXT)
                                    .whitespace_normal()
                                    .child(format!("Workdir: {}", run.work_dir))
                            )
                    )
                    .child(
                        div()
                            .flex_col()
                            .gap_2()
                            .p_3()
                            .rounded_lg()
                            .bg(CARD_BG)
                            .border_1()
                            .border_color(BORDER_LIGHT)
                            .child(div().text_xs().text_color(MUTED_TEXT).child("Progress"))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(PRIMARY_TEXT)
                                    .whitespace_normal()
                                    .child(run.status_message.clone())
                            )
                    )
                    .child(
                        div()
                            .flex_col()
                            .gap_2()
                            .p_3()
                            .rounded_lg()
                            .bg(CARD_BG)
                            .border_1()
                            .border_color(BORDER_LIGHT)
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(div().text_xs().text_color(MUTED_TEXT).child("Preview"))
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(preview_color)
                                            .font_weight(FontWeight::BOLD)
                                            .child(preview_label)
                                    )
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(SECONDARY_TEXT)
                                    .whitespace_normal()
                                    .child(
                                        preview
                                            .as_ref()
                                            .map(|preview| preview.note.clone())
                                            .unwrap_or_else(|| "No preview information".to_string())
                                    )
                            )
                            .when_some(preview.clone().and_then(|preview| preview.entry_file), |this, entry_file| {
                                this.child(
                                    div()
                                        .text_xs()
                                        .text_color(MUTED_TEXT)
                                        .whitespace_normal()
                                        .child(format!("Entry: {}", entry_file))
                                )
                            })
                            .when_some(preview.clone().and_then(|preview| preview.url), |this, url| {
                                this.child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap_2()
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(BRAND_BLUE)
                                                .whitespace_normal()
                                                .child(url.clone())
                                        )
                                        .child(
                                            div()
                                                .px_3()
                                                .py_2()
                                                .rounded_md()
                                                .bg(BRAND_BLUE)
                                                .text_xs()
                                                .text_color(gpui::white())
                                                .cursor_pointer()
                                                .on_mouse_down(gpui::MouseButton::Left, cx.listener({
                                                    let url = url.clone();
                                                    move |this, _: &gpui::MouseDownEvent, _window, _cx| {
                                                        this.open_url_in_browser(&url);
                                                    }
                                                }))
                                                .child("Open In Browser")
                                        )
                                )
                            })
                    )
                    .child(
                        div()
                            .flex_col()
                            .gap_2()
                            .p_3()
                            .rounded_lg()
                            .bg(CARD_BG)
                            .border_1()
                            .border_color(BORDER_LIGHT)
                            .child(div().text_xs().text_color(MUTED_TEXT).child("Live Output"))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(PRIMARY_TEXT)
                                    .whitespace_normal()
                                    .child(live_output)
                            )
                    )
                    .child(
                        div()
                            .flex_col()
                            .gap_2()
                            .p_3()
                            .rounded_lg()
                            .bg(CARD_BG)
                            .border_1()
                            .border_color(BORDER_LIGHT)
                            .child(div().text_xs().text_color(MUTED_TEXT).child("Command"))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(SECONDARY_TEXT)
                                    .whitespace_normal()
                                    .child(if run.command_preview.is_empty() {
                                        "Claude command has not started yet".to_string()
                                    } else {
                                        run.command_preview.clone()
                                    })
                            )
                    )
                    .child(
                        div()
                            .flex_col()
                            .gap_2()
                            .p_3()
                            .rounded_lg()
                            .bg(CARD_BG)
                            .border_1()
                            .border_color(BORDER_LIGHT)
                            .child(div().text_xs().text_color(MUTED_TEXT).child("stderr"))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(if run.stderr_lines.is_empty() { SECONDARY_TEXT } else { Hsla { h: 0.0, s: 0.72, l: 0.52, a: 1.0 } })
                                    .whitespace_normal()
                                    .child(stderr_preview)
                            )
                    )
                    .child(
                        div()
                            .flex_col()
                            .gap_2()
                            .p_3()
                            .rounded_lg()
                            .bg(CARD_BG)
                            .border_1()
                            .border_color(BORDER_LIGHT)
                            .child(div().text_xs().text_color(MUTED_TEXT).child("Timeline"))
                            .child(timeline)
                    )
            );
        } else {
            sidebar = sidebar.child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .p_6()
                    .child(
                        div()
                            .flex_col()
                            .gap_2()
                            .items_center()
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(PRIMARY_TEXT)
                                    .font_weight(FontWeight::BOLD)
                                    .child("No Claude run yet")
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(MUTED_TEXT)
                                    .whitespace_normal()
                                    .child("Send a Claude Code request and this panel will show live progress, logs, and the final result.")
                            )
                    )
            );
        }

        sidebar
    }

    fn render_terminal_resizer(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("terminal-resizer")
            .w(px(8.0))
            .h_full()
            .cursor_col_resize()
            .bg(BORDER_LIGHT)
            .on_drag(DraggedResizer, |_, _, _, cx| {
                cx.new(|_| DraggedResizer)
            })
            .on_drag_move(cx.listener(|this, e: &DragMoveEvent<DraggedResizer>, _window, _cx| {
                if let (Some(initial_x), Some(initial_width)) = (this.terminal_resize_initial_mouse_x, this.terminal_resize_initial_width) {
                    let current_x: f32 = e.event.position.x.into();
                    let delta = initial_x - current_x;
                    let new_width = initial_width + delta;
                    eprintln!("drag_move: initial_x={}, current_x={}, delta={}, new_width={}", initial_x, current_x, delta, new_width);
                    if new_width >= 200.0 && new_width <= 800.0 {
                        this.terminal_width = new_width;
                    }
                }
            }))
            .on_mouse_down(gpui::MouseButton::Left, cx.listener(|this, event: &gpui::MouseDownEvent, _window, _cx| {
                let initial_mouse_x: f32 = event.position.x.into();
                this.terminal_resize_initial_mouse_x = Some(initial_mouse_x);
                this.terminal_resize_initial_width = Some(this.terminal_width);
                eprintln!("on_mouse_down: initial_mouse_x={}, initial_width={}", initial_mouse_x, this.terminal_width);
            }))
    }

    fn render_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let terminal_bg = CARD_BG;
        let terminal_text = PRIMARY_TEXT;
        let prompt_color = Hsla { h: 0.35, s: 0.8, l: 0.45, a: 1.0 };
        let error_color = Hsla { h: 0.0, s: 0.8, l: 0.45, a: 1.0 };
        let width = self.terminal_width;

        // Get working directory based on active task
        let work_dir = self.get_work_dir();

        // Create terminal input editor
        let terminal_editor = window.use_keyed_state("terminal_editor", &mut *cx, |window, cx| {
            let mut editor = Editor::single_line(window, cx);
            editor.set_placeholder_text("Type a command...", window, cx);
            editor
        });

        let terminal_focus = terminal_editor.read(cx).focus_handle(cx);
        let weak_terminal = terminal_editor.downgrade();

        div()
            .flex()
            .flex_col()
            .w(px(width))
            .bg(terminal_bg)
            .child(
                div()
                    .flex()
                    .items_center()
                    .h(px(36.0))
                    .px_3()
                    .bg(WORKSPACE_BG)
                    .child(div().text_xs().text_color(MUTED_TEXT).child("Terminal"))
                    .child(
                        div()
                            .text_xs()
                            .text_color(TERTIARY_TEXT)
                            .ml_auto()
                            .child(format!("{} | {}", work_dir, match &self.sandbox_backend {
                                Backend::Docker(_) => "docker",
                                Backend::Pty(_) => "pty",
                            }))
                    )
            )
            .child(
                div()
                    .id("terminal-content")
                    .flex_1()
                    .overflow_scroll()
                    .p_3()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .children(self.terminal_output.iter().map(|line| {
                        let prompt_color = prompt_color;
                        let terminal_text = terminal_text;
                        let error_color = error_color;
                        let output = line.output.clone();
                        let is_error = output.contains("Error") || output.contains("error:");
                        div()
                            .flex_col()
                            .gap_1()
                            .children(line.command.iter().map(|cmd| {
                                div()
                                    .flex()
                                    .gap_2()
                                    .child(div().text_sm().text_color(prompt_color).child("➜"))
                                    .child(div().text_sm().text_color(terminal_text).child(cmd.clone()))
                            }))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(if is_error { error_color } else { terminal_text })
                                    .child(output)
                            )
                            .into_any_element()
                    }))
            )
            .child(
                div()
                    .id("terminal-input-line")
                    .h(px(40.0))
                    .px_3()
                    .bg(CARD_BG)
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().text_sm().text_color(prompt_color).child("➜"))
                    .child(
                        div()
                            .flex_1()
                            .px_2()
                            .rounded_lg()
                            .bg(WORKSPACE_BG)
                            .track_focus(&terminal_focus)
                            .on_action(cx.listener(move |this, _: &Confirm, _window, cx| {
                                if let Some(editor) = weak_terminal.upgrade() {
                                    let text = editor.read_with(cx, |editor, cx| editor.text(cx)).trim().to_string();
                                    if !text.is_empty() {
                                        let task_id = this.active_task_id.unwrap_or(0);
                                        let sandbox_backend = this.sandbox_backend.clone();

                                        // Clear editor first
                                        editor.update(cx, |editor, cx| {
                                            editor.set_text("", _window, cx);
                                        });

                                        // Execute command and update UI using gpui_tokio::spawn
                                        cx.spawn(async move |this, cx| {
                                            // Execute command and capture result
                                            let cmd_for_output = text.clone();
                                            let exec_result = gpui_tokio::Tokio::spawn(cx, async move {
                                                match &sandbox_backend {
                                                    Backend::Docker(b) => b.exec_command(task_id, vec![&text]).await,
                                                    Backend::Pty(b) => b.exec_command(task_id, vec![&text]).await,
                                                }
                                            }).await;

                                            let output = match exec_result {
                                                Ok(Ok(out)) => out,
                                                Ok(Err(e)) => format!("Error: {}", e),
                                                Err(e) => format!("Spawn error: {}", e),
                                            };

                                            // Update UI with command output
                                            let _ = this.update(cx, |this, cx| {
                                                this.terminal_output.push(TerminalLine {
                                                    command: Some(cmd_for_output),
                                                    output,
                                                });
                                                cx.notify();
                                            });
                                        }).detach();
                                    }
                                }
                            }))
                            .child(terminal_editor)
                    ))
    }
}

fn main() {
    println!("ONE GUI - Starting...");

    env_logger::init();

    // Load config from file
    let config = load_config();

    // Load app icon
    let icon_path = std::path::Path::new("assets/logo.png");
    let icon_image = if icon_path.exists() {
        image::open(icon_path).ok().map(|img| img.to_rgba8()).map(|rgba| {
            let (width, height) = rgba.dimensions();
            Arc::new(image::RgbaImage::from_raw(width, height, rgba.into_raw()).unwrap())
        })
    } else {
        eprintln!("[App] Icon not found at assets/logo.png");
        None
    };

    application().with_assets(assets::Assets).run(move |cx: &mut App| {
        settings::init(cx);
        theme_settings::init(theme::LoadThemes::JustBase, cx);
        editor::init(cx);
        gpui_tokio::init(cx);

        // Load default keymap so Editor actions like Delete, Backspace work
        cx.bind_keys(
            KeymapFile::load_asset_allow_partial_failure(
                DEFAULT_KEYMAP_PATH,
                cx,
            )
            .expect("failed to load default keymap"),
        );

        let bounds = Bounds::centered(None, size(px(DEFAULT_WINDOW_WIDTH), px(DEFAULT_WINDOW_HEIGHT)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                is_resizable: true,
                window_min_size: Some(size(px(800.0), px(600.0))),
                icon: icon_image.clone(),
                ..Default::default()
            },
            move |window, cx| cx.new(|cx| AppState::new(window, cx, config.clone())),
        ).unwrap();
        cx.activate(true);
    });
}
