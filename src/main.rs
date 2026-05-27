use dirs;
use gpui::{
    div, point, prelude::*, px, size, svg, AnyElement, App, Bounds, Context, DragMoveEvent,
    Focusable, Hsla, InteractiveElement, IntoElement, ParentElement, Pixels, Point, Render,
    ScrollHandle, StatefulInteractiveElement, Styled, TitlebarOptions, Window, WindowBounds,
    WindowOptions,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, TryRecvError};
use std::sync::Arc;
use std::time::Duration;

use crate::services::summarize_conversation_async;
use editor::Editor;
use gpui_platform::application;
use menu::Confirm;
use settings::{KeymapFile, DEFAULT_KEYMAP_PATH};
use theme;
use theme_settings;

use gpui::FontWeight;

mod agents;
mod assets;
mod i18n;
mod memory;
mod sandbox;
mod services;
mod skills_market;
mod task_db;
pub(crate) mod ui_theme;

use agents::{claude_code::ClaudeStreamEvent, intent, types::RoutingDecision};
use system_tools;
use i18n::{t, Lang, Translations};
use memory::types::ChatMessage;
use sandbox::backend::{Backend, SandboxBackend};
use services::api::call_chat_api_stream;
use services::{load_config, save_config, Config};
use skills_market::SkillsMarketState;

pub(crate) use ui_theme::{
    get_theme_mode, set_theme_mode, ThemeMode, ACCENT_TEXT, ACTIVE_BG, ASSISTANT_BUBBLE_BG,
    AVATAR_BG, BORDER_LIGHT, BRAND_BLUE, CANVAS_BG, CARD_BG, FLOATING_PANEL_BG, GHOST_SURFACE_BG,
    HEADER_BG, INPUT_BG, MUTED_TEXT, NAV_BG, PRIMARY_TEXT, SECONDARY_TEXT, SURFACE_ACCENT,
    SURFACE_ELEVATED, SURFACE_PANEL, TERTIARY_TEXT, USER_BUBBLE_BG, WORKSPACE_BG,
};

struct DraggedResizer;

impl Render for DraggedResizer {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size(px(0.0)).into_element()
    }
}

struct HeaderTooltip {
    text: String,
}

impl Render for HeaderTooltip {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .max_w(px(420.0))
            .px_3()
            .py_2()
            .rounded_md()
            .bg(SURFACE_PANEL())
            .border_1()
            .border_color(BORDER_LIGHT())
            .text_xs()
            .text_color(PRIMARY_TEXT())
            .whitespace_normal()
            .child(self.text.clone())
    }
}

pub(crate) struct TitleTooltip {
    pub text: String,
}

impl Render for TitleTooltip {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .max_w(px(420.0))
            .px_3()
            .py_2()
            .rounded_md()
            .bg(SURFACE_PANEL())
            .border_1()
            .border_color(BORDER_LIGHT())
            .text_xs()
            .text_color(PRIMARY_TEXT())
            .whitespace_normal()
            .child(self.text.clone())
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
        ToggleTheme,
        ExportChat,
    ]
);

const NAV_WIDTH: f32 = 280.0;
const DEFAULT_WINDOW_WIDTH: f32 = 1200.0;
const DEFAULT_WINDOW_HEIGHT: f32 = 760.0;
const TITLEBAR_HEIGHT: f32 = 44.0;

fn titlebar_leading_inset() -> f32 {
    if cfg!(target_os = "macos") {
        86.0
    } else {
        16.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MainView {
    Chat,
    SkillsMarket,
}

pub(crate) fn icon_label(key: &str) -> &'static str {
    match key {
        "workspace" => "WS",
        "capabilities" => "CP",
        "models" => "ML",
        "settings" => "ST",
        "support" => "SP",
        "assistant" => "ONE",
        "folder" => "DIR",
        "share" => "SHR",
        "terminal" => "TTY",
        "run-panel" => "RUN",
        "add" => "ADD",
        "mic" => "MIC",
        "skill" => "SKL",
        _ => "UI",
    }
}

pub(crate) fn icon_asset_path(key: &str) -> Option<&'static str> {
    match key {
        "workspace" => Some("thems/workspace.svg"),
        "capabilities" => Some("thems/capabilities.svg"),
        "models" => Some("thems/models.svg"),
        "assistant" => Some("thems/one-ai.svg"),
        "folder" => Some("folder.svg"),
        "share" => Some("thems/attachment.svg"),
        "terminal" => Some("thems/cmd.svg"),
        "run-panel" => Some("thems/side-panel.svg"),
        "add" => Some("thems/attachment.svg"),
        "mic" => Some("thems/mic.svg"),
        "skill" => Some("thems/upload.svg"),
        "upload" => Some("thems/upload.svg"),
        _ => None,
    }
}

fn render_icon_element(icon_key: &str, color: Hsla, size_px: f32) -> AnyElement {
    match icon_asset_path(icon_key) {
        Some(path) => svg()
            .path(path)
            .size(px(size_px))
            .flex_none()
            .text_color(color)
            .into_any_element(),
        None => div()
            .text_xs()
            .text_color(color)
            .child(icon_label(icon_key))
            .into_any_element(),
    }
}

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
    main_view: MainView,
    pub(crate) skills_market: SkillsMarketState,
    show_model_config_dialog: bool,
    show_export_dialog: bool,
    exported_json: Option<String>,
    exported_md: Option<String>,
    model_base_url: String,
    model_api_key: String,
    model_name: String,
    current_lang: Lang,
    theme_mode: ThemeMode,
    editing_model_name: String,
    editing_base_url: String,
    editing_api_key: String,
    messages: Vec<ChatMessage>,
    chat_scroll_handle: ScrollHandle,
    needs_auto_scroll: bool,
    pending_summarize: bool,
    next_summarize_job_id: u64,
    summarize_job_id: Option<u64>,
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
    think_collapsed: HashMap<String, bool>,
    next_general_ai_run_id: u64,
    general_ai_run_id: Option<u64>,
    general_ai_task_id: Option<usize>,
    general_ai_live_text: String,
    general_ai_show_live_bubble: bool,
    titlebar_should_move: bool,
    // SystemTools dangerous operation confirmation
    pending_confirmation_tools: Option<(Vec<system_tools::Tool>, String)>,
    // Intent understanding (only populated when LLM intent analysis is used)
    intent_thinking: String,
    intent_content_parts: Vec<ContentPart>,
    // Intent router for fast routing without LLM
    intent_router: agents::intent_router::IntentRouter,
}

#[derive(Debug, Clone)]
struct TerminalLine {
    command: Option<String>,
    output: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum RequestKind {
    GeneralAi,
    ClaudeCode,
}

#[derive(Debug, Clone)]
enum GeneralAiStreamEvent {
    Delta(String),
    Finished { result: String },
    Failed { error: String },
    ConfirmationRequired { tools: Vec<system_tools::Tool> },
}

#[derive(Debug, Clone)]
enum SummarizeEvent {
    Finished {
        job_id: u64,
        task_id: usize,
        summary: String,
    },
    Failed {
        job_id: u64,
        task_id: usize,
        error: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum ClaudeRunStatus {
    Running,
    Completed,
    Failed,
}

impl ClaudeRunStatus {
    fn label(&self, lang: Lang) -> &'static str {
        match self {
            Self::Running => t(lang, Translations::STATUS_RUNNING),
            Self::Completed => t(lang, Translations::STATUS_COMPLETED),
            Self::Failed => t(lang, Translations::STATUS_FAILED),
        }
    }

    fn color(&self) -> Hsla {
        match self {
            Self::Running => BRAND_BLUE(),
            Self::Completed => Hsla {
                h: 0.36,
                s: 0.65,
                l: 0.42,
                a: 1.0,
            },
            Self::Failed => Hsla {
                h: 0.0,
                s: 0.72,
                l: 0.52,
                a: 1.0,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum ClaudeRunTone {
    Info,
    Success,
    Error,
}

impl ClaudeRunTone {
    fn color(&self) -> Hsla {
        match self {
            Self::Info => SECONDARY_TEXT(),
            Self::Success => Hsla {
                h: 0.36,
                s: 0.65,
                l: 0.42,
                a: 1.0,
            },
            Self::Error => Hsla {
                h: 0.0,
                s: 0.72,
                l: 0.52,
                a: 1.0,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum FormattedContent {
    Plain(String),
    Json(String),
    Code(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    session_id: Option<String>,
    artifacts: Vec<ArtifactEntry>,
    pending_question: Option<PendingQuestion>,
}

#[derive(Debug)]
struct PreviewProcessHandle {
    child: Child,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum PreviewStatus {
    Idle,
    Ready,
    Failed,
}

impl PreviewStatus {
    fn label(&self, lang: Lang) -> &'static str {
        match self {
            Self::Idle => t(lang, Translations::PREVIEW_IDLE),
            Self::Ready => t(lang, Translations::PREVIEW_READY),
            Self::Failed => t(lang, Translations::STATUS_FAILED),
        }
    }

    fn color(&self) -> Hsla {
        match self {
            Self::Idle => MUTED_TEXT(),
            Self::Ready => Hsla {
                h: 0.36,
                s: 0.65,
                l: 0.42,
                a: 1.0,
            },
            Self::Failed => Hsla {
                h: 0.0,
                s: 0.72,
                l: 0.52,
                a: 1.0,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ArtifactEntry {
    name: String,
    relative_path: String,
    absolute_path: String,
    kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PendingQuestion {
    prompt: String,
    options: Vec<String>,
    session_id: Option<String>,
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
            .bg(Hsla {
                h: 0.0,
                s: 0.0,
                l: 0.98,
                a: 1.0,
            })
            .border_1()
            .border_color(BORDER_LIGHT())
            .child(
                div()
                    .text_xs()
                    .text_color(block_color)
                    .font_family("Menlo")
                    .whitespace_normal()
                    .child(text.clone()),
            )
            .into_any_element(),
        FormattedContent::Code(text) => div()
            .p_2()
            .rounded_md()
            .bg(Hsla {
                h: 0.62,
                s: 0.15,
                l: 0.97,
                a: 1.0,
            })
            .border_1()
            .border_color(BORDER_LIGHT())
            .child(
                div()
                    .text_xs()
                    .text_color(block_color)
                    .font_family("Menlo")
                    .whitespace_normal()
                    .child(text.clone()),
            )
            .into_any_element(),
    }
}

#[derive(Debug, Clone)]
enum ContentPart {
    Normal(String),
    Think { text: String, complete: bool },
    IntentUnderstanding { text: String, complete: bool },
    ProcessTable { processes: Vec<ProcessDisplayInfo> },
}

#[derive(Debug, Clone)]
pub struct ProcessDisplayInfo {
    pub name: String,
    pub pid: u32,
    pub cpu_percent: f64,
    pub memory_mb: f64,
    pub is_critical: bool,
}

fn try_parse_process_list(content: &str) -> Option<Vec<ProcessDisplayInfo>> {
    let trimmed = content.trim();
    if !trimmed.starts_with('[') {
        return None;
    }
    let Ok(parsed) = serde_json::from_str::<Vec<serde_json::Value>>(trimmed) else {
        return None;
    };
    if parsed.is_empty() {
        return None;
    }
    let has_expected_fields = parsed.iter().all(|v| {
        v.get("pid").is_some()
            && v.get("name").is_some()
            && v.get("cpu_percent").is_some()
            && v.get("memory_mb").is_some()
    });
    if !has_expected_fields {
        return None;
    }
    let processes: Vec<ProcessDisplayInfo> = parsed
        .iter()
        .filter_map(|v| {
            let name = v.get("name")?.as_str()?.to_string();
            let pid = v.get("pid")?.as_u64()?.try_into().ok()?;
            let cpu_percent = v.get("cpu_percent")?.as_f64()?;
            let memory_mb = v.get("memory_mb")?.as_f64()?;
            let is_critical = cpu_percent > 60.0;
            Some(ProcessDisplayInfo {
                name,
                pid,
                cpu_percent,
                memory_mb,
                is_critical,
            })
        })
        .collect();
    if processes.len() < 3 {
        return None;
    }
    Some(processes)
}

fn strip_think_tags(content: &str) -> String {
    let mut result = content.to_string();
    while let Some(start) = result.find("<think>") {
        if let Some(end) = result[start..].find("</think>") {
            result = format!(
                "{}{}",
                &result[..start],
                &result[start + end + "</think>".len()..]
            );
        } else {
            break;
        }
    }
    result
}

fn parse_think_content(content: &str) -> Vec<ContentPart> {
    let open = "<think>";
    let close = "</think>";

    let mut parts = Vec::new();
    let mut pos = 0;
    let mut prev_was_think = false;

    while pos < content.len() {
        let start_rel = match content[pos..].find(open) {
            Some(idx) => idx,
            None => break,
        };
        let start = pos + start_rel;

        if pos < start {
            let mut text = content[pos..start].to_string();
            if prev_was_think {
                text = text
                    .trim_start_matches(|ch| ch == '\n' || ch == '\r')
                    .to_string();
            }
            text = text
                .trim_end_matches(|ch| ch == '\n' || ch == '\r')
                .to_string();
            if !text.is_empty() {
                parts.push(ContentPart::Normal(text));
            }
        }

        let inner_start = start + open.len();
        if let Some(close_rel) = content[inner_start..].find(close) {
            let close_start = inner_start + close_rel;
            let inner_text = content[inner_start..close_start]
                .trim_matches(|ch| ch == '\n' || ch == '\r')
                .to_string();
            parts.push(ContentPart::Think {
                text: inner_text,
                complete: true,
            });
            prev_was_think = true;
            pos = close_start + close.len();
        } else {
            let inner_text = content[inner_start..]
                .trim_matches(|ch| ch == '\n' || ch == '\r')
                .to_string();
            parts.push(ContentPart::Think {
                text: inner_text,
                complete: false,
            });
            prev_was_think = true;
            pos = content.len();
        }
    }

    if pos < content.len() {
        let mut text = content[pos..].to_string();
        if prev_was_think {
            text = text
                .trim_start_matches(|ch| ch == '\n' || ch == '\r')
                .to_string();
        }
        if !text.is_empty() {
            parts.push(ContentPart::Normal(text));
        }
    }

    if parts.is_empty() {
        parts.push(ContentPart::Normal(content.to_string()));
    } else if let Some(processes) = try_parse_process_list(content) {
        if !processes.is_empty() {
            parts.push(ContentPart::ProcessTable { processes });
        }
    }

    parts
}

fn escape_visible_snippet(text: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for ch in text.chars().take(max_chars) {
        match ch {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out
}

fn format_memory(mb: f64) -> String {
    if mb >= 1024.0 {
        format!("{:.1} GB", mb / 1024.0)
    } else {
        format!("{:.0} MB", mb)
    }
}

fn normalize_single_line_label(text: &str) -> String {
    text.replace(['\r', '\n'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn log_think_boundary_newlines(label: &str, content: &str) {
    if !content.contains("<think>") && !content.contains("</think>") {
        return;
    }

    let mut open_index = 0usize;
    while let Some(rel) = content[open_index..].find("<think>") {
        let i = open_index + rel;
        let after = i + "<think>".len();
        let mut count = 0usize;
        for ch in content[after..].chars() {
            if ch == '\n' || ch == '\r' {
                count += 1;
            } else {
                break;
            }
        }
        let snippet = escape_visible_snippet(&content[after..], 60);
        eprintln!("[THINK-SPACING] {label} open@{i} after_newlines={count} after_snip='{snippet}'");
        open_index = after;
    }

    let mut close_index = 0usize;
    while let Some(rel) = content[close_index..].find("</think>") {
        let i = close_index + rel;
        let after = i + "</think>".len();
        let mut count = 0usize;
        for ch in content[after..].chars() {
            if ch == '\n' || ch == '\r' {
                count += 1;
            } else {
                break;
            }
        }
        let snippet = escape_visible_snippet(&content[after..], 60);
        eprintln!(
            "[THINK-SPACING] {label} close@{i} after_newlines={count} after_snip='{snippet}'"
        );
        close_index = after;
    }
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
    is_draft: bool,
}

impl AppState {
    fn begin_general_ai_run(&mut self) -> u64 {
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

    fn handle_intent_result(&mut self, decision: RoutingDecision, _user_message: String, cx: &mut Context<Self>, _window: Option<&mut Window>) {
        // Mark intent understanding as complete
        if let Some(ContentPart::IntentUnderstanding { complete, .. }) = self.intent_content_parts.first_mut() {
            *complete = true;
        }

        // User message already added in route_message, just trigger scroll here
        self.needs_auto_scroll = true;
        cx.notify();

        match decision {
            RoutingDecision::ClaudeCode { instruction, session_id } => {
                eprintln!("[ROUTER] Routing to Claude Code");
                self.request_in_flight = true;
                self.request_status_text = Some(t(self.current_lang, Translations::CLAUDE_CODE_RUNNING_ELLIPSIS).to_string());
                self.request_kind = Some(RequestKind::ClaudeCode);
                self.spawn_claude_code_run(instruction, session_id, cx);
            }
            RoutingDecision::SystemTools { task } => {
                eprintln!("[ROUTER] Routing to System Tools");
                self.spawn_system_tools_run(task, cx);
            }
            _ => {
                eprintln!("[ROUTER] Routing to General AI");
                self.spawn_general_ai_run(cx);
            }
        }
    }

    fn apply_general_ai_stream_event(
        &mut self,
        run_id: u64,
        event: GeneralAiStreamEvent,
        cx: &mut Context<Self>,
    ) {
        if self.general_ai_run_id != Some(run_id) {
            return;
        }

        let run_task_id = self.general_ai_task_id;
        match event {
            GeneralAiStreamEvent::Delta(delta) => {
                if self.general_ai_live_text.is_empty() {
                    self.request_status_text =
                        Some(t(self.current_lang, Translations::GENERATING_RESPONSE).to_string());
                }
                self.general_ai_live_text.push_str(&delta);
                if delta.contains("<think>") || delta.contains("</think>") {
                    log_think_boundary_newlines("general_ai:delta", &self.general_ai_live_text);
                }
                self.needs_auto_scroll = run_task_id == self.active_task_id;
            }
            GeneralAiStreamEvent::Finished { result } => {
                let content = if result.trim().is_empty() {
                    self.general_ai_live_text.clone()
                } else {
                    result.clone()
                };

                if content.starts_with("CONFIRM_REQUIRED:") {
                    let tools_json = content.strip_prefix("CONFIRM_REQUIRED:").unwrap_or("");
                    let dangerous_msg = "⚠️ 检测到危险操作：\n\n由于包含危险操作，当前已跳过执行。";

                    self.pending_confirmation_tools = Some((Vec::new(), tools_json.to_string()));

                    if run_task_id == self.active_task_id {
                        self.messages.push(ChatMessage {
                            role: "assistant".to_string(),
                            content: dangerous_msg.to_string(),
                        });
                        self.needs_auto_scroll = true;
                    }

                    self.request_in_flight = false;
                    self.request_status_text = None;
                    self.general_ai_run_id = None;
                    self.general_ai_task_id = None;
                    self.general_ai_show_live_bubble = false;
                    self.general_ai_live_text.clear();
                    return;
                }

                log_think_boundary_newlines("general_ai:final", &content);

                self.request_in_flight = false;
                self.request_status_text = None;
                self.request_kind = None;
                self.general_ai_run_id = None;
                self.general_ai_task_id = None;
                self.general_ai_show_live_bubble = false;
                self.general_ai_live_text.clear();

                if run_task_id == self.active_task_id {
                    self.messages.push(ChatMessage {
                        role: "assistant".to_string(),
                        content: content.clone(),
                    });
                    self.needs_auto_scroll = true;
                }
                if let Some(task_id) = run_task_id {
                    task_db::insert_message(&self.db.conn, task_id, "assistant", &content).ok();
                }

                if self.pending_summarize && run_task_id == self.active_task_id {
                    self.pending_summarize = false;
                    let task_id = self.active_task_id;
                    let all_messages = self.messages.clone();
                    let base_url = self.model_base_url.clone();
                    let api_key = self.model_api_key.clone();
                    let model = self.model_name.clone();
                    if let Some(tid) = task_id {
                        self.next_summarize_job_id += 1;
                        let job_id = self.next_summarize_job_id;
                        self.summarize_job_id = Some(job_id);

                        let (sender, receiver) = mpsc::channel::<SummarizeEvent>();
                        let sender_ok = sender.clone();
                        let sender_err = sender;

                        gpui_tokio::Tokio::spawn(cx, async move {
                            match summarize_conversation_async(
                                &base_url,
                                &api_key,
                                &model,
                                &all_messages,
                            )
                            .await
                            {
                                Ok(summary) => {
                                    let _ = sender_ok.send(SummarizeEvent::Finished {
                                        job_id,
                                        task_id: tid,
                                        summary,
                                    });
                                }
                                Err(error) => {
                                    let _ = sender_err.send(SummarizeEvent::Failed {
                                        job_id,
                                        task_id: tid,
                                        error,
                                    });
                                }
                            }
                        })
                        .detach();

                        cx.spawn(async move |this, cx| loop {
                            let mut disconnected = false;

                            loop {
                                match receiver.try_recv() {
                                    Ok(event) => {
                                        let _ = this.update(cx, |this, cx| {
                                            this.apply_summarize_event(event);
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
                        })
                        .detach();
                    }
                }
            }
            GeneralAiStreamEvent::Failed { error } => {
                let error_message = format!(
                    "AI request failed: {}\n\nPlease check network connectivity, Base URL, and API key.",
                    error
                );

                self.request_in_flight = false;
                self.request_status_text = None;
                self.request_kind = None;
                self.general_ai_run_id = None;
                self.general_ai_task_id = None;
                self.general_ai_show_live_bubble = false;
                self.general_ai_live_text.clear();

                if run_task_id == self.active_task_id {
                    self.messages.push(ChatMessage {
                        role: "assistant".to_string(),
                        content: error_message.clone(),
                    });
                    self.needs_auto_scroll = true;
                }
                if let Some(task_id) = run_task_id {
                    task_db::insert_message(&self.db.conn, task_id, "assistant", &error_message)
                        .ok();
                }
            }
            GeneralAiStreamEvent::ConfirmationRequired { tools } => {
                self.request_in_flight = false;
                self.request_status_text = None;
                self.general_ai_run_id = None;
                self.general_ai_task_id = None;
                self.general_ai_show_live_bubble = false;
                self.general_ai_live_text.clear();

                self.pending_confirmation_tools = Some((tools, "".to_string()));

                if run_task_id == self.active_task_id {
                    let msg = "⚠️ 检测到危险操作：\n\n由于包含危险操作，当前已跳过执行。";
                    self.messages.push(ChatMessage {
                        role: "assistant".to_string(),
                        content: msg.to_string(),
                    });
                    self.needs_auto_scroll = true;
                }
            }
        }
    }

    fn apply_summarize_event(&mut self, event: SummarizeEvent) {
        match event {
            SummarizeEvent::Finished {
                job_id,
                task_id,
                summary,
            } => {
                if self.summarize_job_id != Some(job_id) {
                    return;
                }
                self.summarize_job_id = None;
                let clean_sum = strip_think_tags(&summary);
                let normalized = normalize_single_line_label(&clean_sum);
                let short_title: String = normalized.chars().take(10).collect();
                if summary.contains('\n')
                    || summary.contains('\r')
                    || clean_sum.contains('\n')
                    || clean_sum.contains('\r')
                {
                    let raw_snip = escape_visible_snippet(&summary, 120);
                    let clean_snip = escape_visible_snippet(&clean_sum, 120);
                    let norm_snip = escape_visible_snippet(&normalized, 120);
                    eprintln!(
                        "[CHAT-TITLE] raw='{}' clean='{}' normalized='{}' final='{}'",
                        raw_snip, clean_snip, norm_snip, short_title
                    );
                }
                task_db::update_task_title(&self.db.conn, task_id, &short_title).ok();
                for ws in &mut self.workspaces {
                    for t in &mut ws.tasks {
                        if t.id == task_id {
                            t.title = short_title.clone();
                            break;
                        }
                    }
                }
            }
            SummarizeEvent::Failed {
                job_id,
                task_id,
                error,
            } => {
                if self.summarize_job_id != Some(job_id) {
                    return;
                }
                self.summarize_job_id = None;
                eprintln!(
                    "[CHAT-TITLE] summarize failed task_id={} error={}",
                    task_id, error
                );
            }
        }
    }

    /// Route a message using fast rule-based routing, with LLM fallback for complex cases
    fn route_message(&mut self, message: String, cx: &mut Context<Self>) {
        // Immediately add user message to chat for instant display
        self.messages.push(ChatMessage {
            role: "user".to_string(),
            content: message.clone(),
        });
        if let Some(task_id) = self.active_task_id {
            task_db::insert_message(&self.db.conn, task_id, "user", &message).ok();
        }
        self.needs_auto_scroll = true;
        cx.notify();

        // Try fast rule-based routing first
        if let Some(decision) = self.intent_router.quick_route(&message) {
            eprintln!("[ROUTER] Fast route matched: {:?}", decision);
            self.handle_routing_decision(decision, message, cx);
            return;
        }

        // Fall back to LLM-based intent analysis for complex cases
        eprintln!("[ROUTER] No fast match, using LLM intent analysis");
        self.spawn_intent_agent_run(message, cx);
    }

    fn handle_routing_decision(&mut self, decision: RoutingDecision, user_message: String, cx: &mut Context<Self>) {
        match decision {
            RoutingDecision::ClaudeCode { instruction, session_id } => {
                eprintln!("[ROUTER] Routing to Claude Code (fast route)");
                self.request_in_flight = true;
                self.request_status_text = Some(t(self.current_lang, Translations::CLAUDE_CODE_RUNNING_ELLIPSIS).to_string());
                self.request_kind = Some(RequestKind::ClaudeCode);
                self.spawn_claude_code_run(instruction, session_id, cx);
            }
            RoutingDecision::SystemTools { task } => {
                eprintln!("[ROUTER] Routing to System Tools (fast route)");
                self.spawn_system_tools_run(task, cx);
            }
            RoutingDecision::GeneralAI { messages } => {
                eprintln!("[ROUTER] Routing to General AI (fast route)");
                // messages contains the user's message already
                self.spawn_general_ai_run(cx);
            }
            _ => {
                eprintln!("[ROUTER] Unknown decision, defaulting to General AI");
                self.spawn_general_ai_run(cx);
            }
        }
    }

    fn spawn_intent_agent_run(&mut self, message: String, cx: &mut Context<Self>) {
        let base_url = self.model_base_url.clone();
        let api_key = self.model_api_key.clone();
        let model = self.model_name.clone();

        // User message already added in route_message, just init intent tracking

        let (sender, receiver) = mpsc::channel::<intent::IntentEvent>();

        self.intent_thinking.clear();
        self.intent_content_parts = vec![ContentPart::IntentUnderstanding {
            text: String::new(),
            complete: false,
        }];
        cx.notify();

        let message_for_async = message.clone();
        gpui_tokio::Tokio::spawn(cx, async move {
            intent::IntentAgent::classify(message_for_async, base_url, api_key, model, sender).await;
        })
        .detach();

        let message_for_decision = message.clone();
        cx.spawn(async move |this, cx| loop {
            let mut disconnected = false;
            let mut pending_decision: Option<(RoutingDecision, String)> = None;

            loop {
                match receiver.try_recv() {
                    Ok(event) => {
                        match &event {
                            intent::IntentEvent::Thinking(text) => {
                                let _ = this.update(cx, |this, cx| {
                                    this.intent_thinking.push_str(text);
                                    this.intent_thinking.push('\n');
                                    if let Some(ContentPart::IntentUnderstanding { text: intent_text, .. }) = this.intent_content_parts.first_mut() {
                                        intent_text.push_str(text);
                                        intent_text.push('\n');
                                    }
                                    cx.notify();
                                });
                            }
                            intent::IntentEvent::Decision(decision) => {
                                pending_decision = Some((decision.clone(), message_for_decision.clone()));
                            }
                            intent::IntentEvent::Error(err) => {
                                let _ = this.update(cx, |this, cx| {
                                    if let Some(ContentPart::IntentUnderstanding { .. }) = this.intent_content_parts.first_mut() {
                                        this.intent_content_parts = vec![ContentPart::IntentUnderstanding {
                                            text: format!("Error: {}", err),
                                            complete: true,
                                        }];
                                    }
                                    cx.notify();
                                });
                                pending_decision = Some((RoutingDecision::GeneralAI {
                                    messages: vec![ChatMessage {
                                        role: "user".to_string(),
                                        content: "".to_string(),
                                    }],
                                }, err.clone()));
                            }
                        }
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }

            // Handle pending decision if any
            if let Some((decision, user_message)) = pending_decision.take() {
                let msg = if user_message.is_empty() {
                    "".to_string()
                } else {
                    user_message
                };
                let _ = this.update(cx, |this, cx| {
                    this.handle_intent_result(decision, msg, cx, None);
                });
            }

            if disconnected {
                break;
            }

            cx.background_executor()
                .timer(Duration::from_millis(60))
                .await;
        })
        .detach();
    }

    fn spawn_general_ai_run(&mut self, cx: &mut Context<Self>) {
        let run_id = self.begin_general_ai_run();

        let base_url = self.model_base_url.clone();
        let api_key = self.model_api_key.clone();
        let model = self.model_name.clone();
        let messages = self.messages.clone();

        let (sender, receiver) = mpsc::channel::<GeneralAiStreamEvent>();
        let delta_sender = sender.clone();
        let final_sender = sender.clone();

        gpui_tokio::Tokio::spawn(cx, async move {
            let result =
                call_chat_api_stream(&base_url, &api_key, &model, &messages, move |delta| {
                    let _ = delta_sender.send(GeneralAiStreamEvent::Delta(delta));
                })
                .await;

            match result {
                Ok(output) => {
                    let _ = final_sender.send(GeneralAiStreamEvent::Finished { result: output });
                }
                Err(error) => {
                    let _ = final_sender.send(GeneralAiStreamEvent::Failed { error });
                }
            }
        })
        .detach();

        cx.spawn(async move |this, cx| loop {
            let mut disconnected = false;

            loop {
                match receiver.try_recv() {
                    Ok(event) => {
                        let _ = this.update(cx, |this, cx| {
                            this.apply_general_ai_stream_event(run_id, event, cx);
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
        })
        .detach();
    }

    fn spawn_system_tools_run(&mut self, task: String, cx: &mut Context<Self>) {
        let run_id = self.begin_general_ai_run();

        let base_url = self.model_base_url.clone();
        let api_key = self.model_api_key.clone();
        let model = self.model_name.clone();

        let (sender, receiver) = mpsc::channel::<GeneralAiStreamEvent>();
        let delta_sender = sender.clone();
        let final_sender = sender.clone();

        let task_for_async = task.clone();
        gpui_tokio::Tokio::spawn(cx, async move {
            let tools_result = system_tools::Tool::from_task_llm_async(&task_for_async, &base_url, &api_key, &model).await;

            let tools_with_danger = match tools_result {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("[SystemTools] LLM parsing failed: {}, falling back to keyword", e);
                    system_tools::Tool::from_task(&task_for_async)
                        .into_iter()
                        .map(|t| (t, None))
                        .collect()
                }
            };

            if system_tools::requires_confirmation(&tools_with_danger) {
                let dangerous_msg = "⚠️ 检测到危险操作：\n".to_string();
                let mut details = Vec::new();
                let mut tools_for_save = Vec::new();
                for (tool, _) in &tools_with_danger {
                    match tool {
                        system_tools::Tool::KillProcess(pid) => {
                            details.push(format!("  - 终止进程 PID={}", pid));
                            tools_for_save.push(format!("kill:{}", pid));
                        }
                        system_tools::Tool::DeleteFile(path) => {
                            details.push(format!("  - 删除文件 {}", path));
                            tools_for_save.push(format!("delete:{}", path));
                        }
                        _ => {}
                    }
                }
                let msg = dangerous_msg + &details.join("\n") + "\n\n由于包含危险操作，当前已跳过执行。";

                let tools_json = serde_json::to_string(&tools_for_save).unwrap_or_default();
                let _ = delta_sender.send(GeneralAiStreamEvent::Delta(msg));
                let _ = final_sender.send(GeneralAiStreamEvent::Finished { result: format!("CONFIRM_REQUIRED:{}", tools_json) });
                return;
            }

            let mut results = Vec::new();
            for (tool, _) in tools_with_danger {
                match tool.execute() {
                    Ok(output) => results.push(output),
                    Err(e) => results.push(format!("Error: {}", e)),
                }
            }

            let response = if results.is_empty() {
                "No operations needed.".to_string()
            } else {
                results.join("\n---\n")
            };

            let _ = delta_sender.send(GeneralAiStreamEvent::Delta(response.clone()));
            let _ = final_sender.send(GeneralAiStreamEvent::Finished { result: response });
        })
        .detach();

        cx.spawn(async move |this, cx| loop {
            let mut disconnected = false;

            loop {
                match receiver.try_recv() {
                    Ok(event) => {
                        let _ = this.update(cx, |this, cx| {
                            this.apply_general_ai_stream_event(run_id, event, cx);
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
        })
        .detach();
    }

    fn confirm_system_tools_operation(&mut self, confirmed: bool, cx: &mut Context<Self>) {
        if !confirmed {
            self.messages.push(ChatMessage {
                role: "assistant".to_string(),
                content: "操作已取消。".to_string(),
            });
            self.pending_confirmation_tools = None;
            cx.notify();
            return;
        }

        let tools_data = self.pending_confirmation_tools.take();
        if let Some((_tools, task_json)) = tools_data {
            let tools = parse_tools_from_json(&task_json);
            if tools.is_empty() {
                self.messages.push(ChatMessage {
                    role: "assistant".to_string(),
                    content: "无法解析操作指令。".to_string(),
                });
                cx.notify();
                return;
            }

            let run_id = self.begin_general_ai_run();

            let (sender, receiver) = mpsc::channel::<GeneralAiStreamEvent>();
            let delta_sender = sender.clone();
            let final_sender = sender.clone();

            let tools_for_async = tools;
            gpui_tokio::Tokio::spawn(cx, async move {
                let mut results = Vec::new();
                for tool in &tools_for_async {
                    match tool.execute() {
                        Ok(output) => results.push(output),
                        Err(e) => results.push(format!("Error: {}", e)),
                    }
                }

                let response = if results.is_empty() {
                    "操作完成。".to_string()
                } else {
                    results.join("\n")
                };

                let _ = delta_sender.send(GeneralAiStreamEvent::Delta(response.clone()));
                let _ = final_sender.send(GeneralAiStreamEvent::Finished { result: response });
            })
            .detach();

            cx.spawn(async move |this, cx| loop {
                let mut disconnected = false;

                loop {
                    match receiver.try_recv() {
                        Ok(event) => {
                            let _ = this.update(cx, |this, cx| {
                                this.apply_general_ai_stream_event(run_id, event, cx);
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
            })
            .detach();
        }
        cx.notify();
    }
}

fn parse_tools_from_json(json_str: &str) -> Vec<system_tools::Tool> {
    let mut tools = Vec::new();

    if json_str.is_empty() {
        return tools;
    }

    if let Ok(items) = serde_json::from_str::<Vec<String>>(json_str) {
        for item in items {
            let parts: Vec<&str> = item.splitn(2, ':').collect();
            if parts.len() == 2 {
                let action = parts[0];
                let value = parts[1];
                match action {
                    "kill" => {
                        if let Ok(pid) = value.parse::<u32>() {
                            tools.push(system_tools::Tool::KillProcess(pid));
                        }
                    }
                    "delete" => {
                        tools.push(system_tools::Tool::DeleteFile(value.to_string()));
                    }
                    "disk" => {
                        if value == "free" {
                            tools.push(system_tools::Tool::DiskFree);
                        } else {
                            tools.push(system_tools::Tool::DiskUsage(value.to_string()));
                        }
                    }
                    "list_dir" => {
                        tools.push(system_tools::Tool::ListDir(value.to_string()));
                    }
                    "list_processes" => {
                        tools.push(system_tools::Tool::ListProcesses);
                    }
                    "top_memory" => {
                        let n = value.parse().unwrap_or(10);
                        tools.push(system_tools::Tool::TopMemoryProcs(n));
                    }
                    _ => {}
                }
            }
        }
    }

    tools
}

fn slugify_task_title(title: &str) -> String {
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
        .collect::<String>()
}

impl AppState {
    fn get_task_dir_for_ids(
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

    fn get_active_task_location(&self) -> Option<(usize, usize, String)> {
        let workspace_id = self.active_workspace_id?;
        let task_id = self.active_task_id?;
        let task = self.get_active_task()?;
        Some((workspace_id, task_id, task.title.clone()))
    }

    fn get_active_task_dir_path(&self) -> Option<PathBuf> {
        let (workspace_id, task_id, title) = self.get_active_task_location()?;
        Some(self.get_task_dir_for_ids(workspace_id, task_id, &title))
    }

    fn get_claude_meta_dir_for_task_dir(task_dir: &std::path::Path) -> PathBuf {
        task_dir.join(".claude")
    }

    fn ensure_task_storage_dir(
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

    fn load_artifacts_for_task_dir(task_dir: &std::path::Path) -> Vec<ArtifactEntry> {
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

    fn persist_current_claude_state(&self) {
        let Some(run) = self.current_claude_run.as_ref() else {
            return;
        };
        let Some(task_id) = run.task_id else {
            return;
        };
        let Some(workspace_id) = self.active_workspace_id else {
            return;
        };
        let task_title = self
            .workspaces
            .iter()
            .find(|w| w.id == workspace_id)
            .and_then(|w| w.tasks.iter().find(|t| t.id == task_id))
            .map(|t| t.title.clone())
            .unwrap_or_else(|| "task".to_string());
        let task_dir = self.get_task_dir_for_ids(workspace_id, task_id, &task_title);
        let meta_dir = Self::get_claude_meta_dir_for_task_dir(&task_dir);
        let _ = std::fs::create_dir_all(&meta_dir);
        let state_path = meta_dir.join("run_state.json");
        if let Ok(json) = serde_json::to_string_pretty(run) {
            let _ = std::fs::write(state_path, json);
        }
    }

    fn load_claude_state_for_task(
        &self,
        workspace_id: usize,
        task_id: usize,
        task_title: &str,
    ) -> Option<ClaudeRunPanelState> {
        let task_dir = self.get_task_dir_for_ids(workspace_id, task_id, task_title);
        let state_path = Self::get_claude_meta_dir_for_task_dir(&task_dir).join("run_state.json");
        let content = std::fs::read_to_string(state_path).ok()?;
        let mut state = serde_json::from_str::<ClaudeRunPanelState>(&content).ok()?;
        state.artifacts = Self::load_artifacts_for_task_dir(&task_dir);
        Some(state)
    }

    fn restore_task_context(&mut self) {
        if let Some((workspace_id, task_id, title)) = self.get_active_task_location() {
            let _ = self.ensure_task_storage_dir(workspace_id, task_id, &title);
            let msgs = task_db::load_messages(&self.db.conn, task_id).unwrap_or_default();
            self.messages = msgs
                .into_iter()
                .map(|m| ChatMessage {
                    role: m.role,
                    content: m.content,
                })
                .collect();
            self.current_claude_run =
                self.load_claude_state_for_task(workspace_id, task_id, &title);
        } else {
            self.messages.clear();
            self.current_claude_run = None;
        }
    }
    fn new(_window: &mut Window, _cx: &mut Context<Self>, config: Config) -> Self {
        let db = task_db::Database::new().expect("Failed to initialize database");
        let theme_mode = config.theme_mode;
        set_theme_mode(theme_mode);

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
            terminal_width: 500.0,
            terminal_resize_initial_mouse_x: None,
            terminal_resize_initial_width: None,
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
            intent_thinking: String::new(),
            intent_content_parts: Vec::new(),
            intent_router: agents::intent_router::IntentRouter::new(),
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
        self.active_workspace_id
            .and_then(|id| self.workspaces.iter().find(|w| w.id == id))
    }

    fn get_active_task(&self) -> Option<&TaskItem> {
        self.get_active_workspace()
            .and_then(|w| w.tasks.iter().find(|t| Some(t.id) == self.active_task_id))
    }

    fn get_work_dir(&self) -> String {
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

    fn ensure_workspace_draft_task(&mut self, workspace_id: usize) -> Option<usize> {
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

    fn begin_claude_run(&mut self, instruction: &str) -> u64 {
        self.next_claude_run_id += 1;
        let run_id = self.next_claude_run_id;
        self.sidebar_visible = true;
        self.request_in_flight = true;
        self.request_status_text = Some(
            t(
                self.current_lang,
                Translations::CLAUDE_CODE_RUNNING_ELLIPSIS,
            )
            .to_string(),
        );
        self.request_kind = Some(RequestKind::ClaudeCode);
        let lang = self.current_lang;
        self.current_claude_run = Some(ClaudeRunPanelState {
            run_id,
            task_id: self.active_task_id,
            instruction: instruction.to_string(),
            work_dir: self.get_work_dir(),
            command_preview: String::new(),
            status: ClaudeRunStatus::Running,
            status_message: t(lang, Translations::WAITING_FOR_CLAUDE_START).to_string(),
            live_text: String::new(),
            final_text: None,
            stderr_lines: vec![],
            events: vec![ClaudeRunEvent::info(
                t(lang, Translations::RUN_QUEUED),
                format!(
                    "{}: {}",
                    t(lang, Translations::INSTRUCTION_SUBMITTED),
                    instruction
                ),
            )],
            show_live_bubble: true,
            preview: Some(PreviewState {
                status: PreviewStatus::Idle,
                entry_file: None,
                url: None,
                note: t(lang, Translations::PREVIEW_AFTER_RUN_NOTE).to_string(),
            }),
            session_id: None,
            artifacts: self
                .get_active_task_dir_path()
                .map(|dir| Self::load_artifacts_for_task_dir(&dir))
                .unwrap_or_default(),
            pending_question: None,
        });
        self.persist_current_claude_state();
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

    fn open_folder_in_finder(&self, path: &str) {
        let _ = Command::new("open")
            .arg(path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
    }

    fn reveal_file_in_finder(&self, path: &str) {
        let _ = Command::new("open")
            .arg("-R")
            .arg(path)
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

        let html_files = Self::collect_html_files(&root);
        if html_files.is_empty() {
            return PreviewLaunchResult::NotFound {
                note: t(lang, Translations::NO_PREVIEWABLE_HTML).to_string(),
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

    fn apply_claude_run_event(&mut self, run_id: u64, event: ClaudeStreamEvent) {
        let lang = self.current_lang;
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
                    run.status_message = t(lang, Translations::CLAUDE_CODE_RUNNING).to_string();
                    run.events.push(ClaudeRunEvent::info(
                        t(lang, Translations::PROCESS_STARTED),
                        format!(
                            "{}: {}\n{}: {}",
                            t(lang, Translations::WORKDIR),
                            workdir,
                            t(lang, Translations::COMMAND),
                            command
                        ),
                    ));
                }
                ClaudeStreamEvent::AssistantText(text) => {
                    if run.live_text.is_empty() {
                        run.events.push(ClaudeRunEvent::info(
                            t(lang, Translations::STREAMING_RESPONSE),
                            t(lang, Translations::CLAUDE_STARTED_LIVE_CONTENT),
                        ));
                    }
                    if !run.live_text.is_empty() {
                        run.live_text.push('\n');
                    }
                    run.live_text.push_str(&text);
                    run.status_message = t(lang, Translations::GENERATING_RESPONSE).to_string();
                }
                ClaudeStreamEvent::Progress { label, detail } => {
                    run.status_message = format!("{}...", label);
                    run.events.push(ClaudeRunEvent::info(label, detail));
                }
                ClaudeStreamEvent::Stderr(line) => {
                    run.stderr_lines.push(line.clone());
                    let stderr_label = t(lang, Translations::STDERR);
                    let tone = if line.to_lowercase().contains("error") {
                        ClaudeRunEvent::error(stderr_label, line)
                    } else {
                        ClaudeRunEvent::info(stderr_label, line)
                    };
                    run.events.push(tone);
                }
                ClaudeStreamEvent::Session { session_id } => {
                    run.session_id = Some(session_id.clone());
                    run.events.push(ClaudeRunEvent::info(
                        t(lang, Translations::SESSION_UPDATED),
                        format!("{}={}", t(lang, Translations::SESSION_ID), session_id),
                    ));
                }
                ClaudeStreamEvent::AskUserQuestion { prompt, options } => {
                    run.pending_question = Some(PendingQuestion {
                        prompt: prompt.clone(),
                        options: options.clone(),
                        session_id: run.session_id.clone(),
                    });
                    run.status_message =
                        t(lang, Translations::CLAUDE_WAITING_FOR_ANSWER).to_string();
                    self.request_in_flight = false;
                    self.request_status_text = None;
                    self.request_kind = None;
                    run.events.push(ClaudeRunEvent::info(
                        t(lang, Translations::QUESTION),
                        if options.is_empty() {
                            prompt
                        } else {
                            format!(
                                "{}\n{}: {}",
                                prompt,
                                t(lang, Translations::OPTIONS),
                                options.join(", ")
                            )
                        },
                    ));
                }
                ClaudeStreamEvent::Finished { result } => {
                    run.status = ClaudeRunStatus::Completed;
                    run.status_message = t(lang, Translations::CLAUDE_COMPLETED).to_string();
                    self.request_in_flight = false;
                    self.request_status_text = None;
                    self.request_kind = None;
                    if run.live_text.trim().is_empty() {
                        run.live_text = result.clone();
                    }
                    run.final_text = Some(result);
                    run.show_live_bubble = false;
                    run.events.push(ClaudeRunEvent::success(
                        t(lang, Translations::RUN_COMPLETED),
                        format!(
                            "{}: {}",
                            t(lang, Translations::GENERATED_CHARACTERS),
                            run.live_text.chars().count()
                        ),
                    ));
                    run.artifacts =
                        Self::load_artifacts_for_task_dir(&PathBuf::from(&run.work_dir));
                    final_message = Some(format!(
                        "{}\n{}",
                        t(lang, Translations::CLAUDE_CODE_TAG),
                        run.live_text
                    ));
                    persist_task_id = run.task_id;
                    finished_work_dir = Some(run.work_dir.clone());
                }
                ClaudeStreamEvent::Failed { error } => {
                    run.status = ClaudeRunStatus::Failed;
                    run.status_message = t(lang, Translations::CLAUDE_FAILED).to_string();
                    self.request_in_flight = false;
                    self.request_status_text = None;
                    self.request_kind = None;
                    run.show_live_bubble = false;
                    run.events.push(ClaudeRunEvent::error(
                        t(lang, Translations::RUN_FAILED),
                        error.clone(),
                    ));
                    let mut message = t(lang, Translations::CLAUDE_EXECUTION_ERROR).to_string();
                    message.push_str(&error);
                    if !run.live_text.trim().is_empty() {
                        message = format!(
                            "{}\n{}\n\n{}\n{}",
                            t(lang, Translations::CLAUDE_CODE_TAG),
                            run.live_text,
                            t(lang, Translations::RUN_FAILED_TAG),
                            error
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
                            t(lang, Translations::PREVIEW_READY_EVENT),
                            format!("{}\n{}", url, note),
                        ));
                        auto_open_url = Some(url);
                        run.events.push(ClaudeRunEvent::info(
                            t(lang, Translations::BROWSER_OPENED),
                            t(lang, Translations::OPENED_PREVIEW_URL),
                        ));
                    }
                    PreviewLaunchResult::NotFound { note } => {
                        run.preview = Some(PreviewState {
                            status: PreviewStatus::Idle,
                            entry_file: None,
                            url: None,
                            note: note.clone(),
                        });
                        run.events.push(ClaudeRunEvent::info(
                            t(lang, Translations::PREVIEW_SKIPPED),
                            note,
                        ));
                    }
                    PreviewLaunchResult::Failed { note } => {
                        run.preview = Some(PreviewState {
                            status: PreviewStatus::Failed,
                            entry_file: None,
                            url: None,
                            note: note.clone(),
                        });
                        run.events.push(ClaudeRunEvent::error(
                            t(lang, Translations::PREVIEW_FAILED_EVENT),
                            note,
                        ));
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
        self.persist_current_claude_state();
    }

    fn spawn_claude_code_run(
        &mut self,
        instruction: String,
        session_id: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let run_id = self.begin_claude_run(&instruction);
        let project_dir =
            if let Some((workspace_id, task_id, title)) = self.get_active_task_location() {
                self.ensure_task_storage_dir(workspace_id, task_id, &title)
            } else {
                std::path::PathBuf::from(self.get_work_dir())
            };

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

        cx.spawn(async move |this, cx| loop {
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
        })
        .detach();
    }

    fn continue_claude_with_answer(&mut self, answer: String, cx: &mut Context<Self>) {
        let lang = self.current_lang;
        let session_id = self
            .current_claude_run
            .as_ref()
            .and_then(|run| run.session_id.clone());
        if let Some(run) = self.current_claude_run.as_mut() {
            run.pending_question = None;
            run.status = ClaudeRunStatus::Running;
            run.status_message = t(lang, Translations::CONTINUING_CLAUDE_RUN).to_string();
            run.events.push(ClaudeRunEvent::info(
                t(lang, Translations::USER_ANSWERED),
                answer.clone(),
            ));
        }
        self.request_in_flight = true;
        self.request_kind = Some(RequestKind::ClaudeCode);
        self.request_status_text =
            Some(t(lang, Translations::CLAUDE_CODE_CONTINUING_ELLIPSIS).to_string());
        self.persist_current_claude_state();
        self.spawn_claude_code_run(answer, session_id, cx);
    }

    // Action handlers for model config dialog
    fn open_model_config_dialog(
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
            theme_mode: self.theme_mode,
        };
        if let Err(e) = save_config(&config) {
            eprintln!("Failed to save config: {}", e);
        }

        cx.notify();
    }

    fn cancel_model_config(
        &mut self,
        _: &CancelModelConfig,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
            theme_mode: self.theme_mode,
        };
        if let Err(e) = save_config(&config) {
            eprintln!("Failed to save lang config: {}", e);
        }
        cx.notify();
    }

    fn toggle_theme(&mut self, _: &ToggleTheme, _: &mut Window, cx: &mut Context<Self>) {
        self.theme_mode = match self.theme_mode {
            ThemeMode::Dark => ThemeMode::Light,
            ThemeMode::Light => ThemeMode::Dark,
        };
        set_theme_mode(self.theme_mode);
        let config = Config {
            model_base_url: self.model_base_url.clone(),
            model_api_key: self.model_api_key.clone(),
            model_name: self.model_name.clone(),
            lang: self.current_lang,
            theme_mode: self.theme_mode,
        };
        if let Err(e) = save_config(&config) {
            eprintln!("Failed to save theme config: {}", e);
        }
        cx.notify();
    }

    fn export_chat(&mut self, _: &ExportChat, _: &mut Window, cx: &mut Context<Self>) {
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

impl Render for AppState {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let sidebar = if self.sidebar_visible {
            Some(self.render_sidebar(window, cx).into_any_element())
        } else {
            None
        };
        div()
            .flex_col()
            .size_full()
            .bg(CARD_BG())
            .child(self.render_window_titlebar(window, cx))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .h_full()
                    .overflow_hidden()
                    .child(self.render_nav(cx))
                    .child(div().w(px(1.0)).bg(BORDER_LIGHT()))
                    .child(
                        div()
                            .flex_1()
                            .h_full()
                            .overflow_hidden()
                            .child(self.render_main_content(window, cx)),
                    )
                    .when_some(sidebar, |this, sidebar| {
                        this.child(div().w(px(1.0)).bg(BORDER_LIGHT()))
                            .child(div().h_full().child(sidebar))
                    })
                    .when(self.terminal_visible, |this| {
                        this.child(self.render_terminal_resizer(cx))
                            .child(self.render_terminal(window, cx))
                    }),
            )
            .when(self.show_model_config_dialog, |this| {
                this.child(self.render_model_config_dialog(window, cx))
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
    fn render_main_content(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        match self.main_view {
            MainView::Chat => self.render_chat(window, cx).into_any_element(),
            MainView::SkillsMarket => skills_market::render_skills_market(&*self, window, cx),
        }
    }

    fn render_nav(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let lang = self.current_lang;
        let workspaces_heading = div()
            .px_4()
            .pt_4()
            .pb_1()
            .text_xs()
            .text_color(MUTED_TEXT())
            .font_weight(FontWeight::BOLD)
            .child(t(lang, Translations::WORKSPACES_HEADING))
            .into_element();

        div()
            .flex()
            .flex_col()
            .w(px(NAV_WIDTH))
            .h_full()
            .bg(NAV_BG())
            .child(div().flex_none().child(self.render_nav_buttons(cx)))
            .child(div().flex_none().child(workspaces_heading).into_element())
            .child(
                div()
                    .id("task-list-container")
                    .flex_1()
                    .overflow_y_scroll()
                    .child(self.render_task_list(cx)),
            )
            .child(div().flex_none().h(px(1.0)).bg(BORDER_LIGHT()))
            .child(div().flex_none().child(self.render_nav_footer_actions()))
    }

    fn render_titlebar_leading(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let lang = self.current_lang;
        let theme_mode = get_theme_mode();
        let theme_label = match (lang, theme_mode) {
            (Lang::Zh, ThemeMode::Dark) => "深色",
            (Lang::Zh, ThemeMode::Light) => "浅色",
            (Lang::En, ThemeMode::Dark) => "Dark",
            (Lang::En, ThemeMode::Light) => "Light",
        };
        div()
            .flex()
            .items_center()
            .justify_between()
            .h_full()
            .pl(px(titlebar_leading_inset()))
            .pr_4()
            .child(
                div().flex().items_center().gap_3().child(
                    div()
                        .text_size(px(20.0))
                        .text_color(PRIMARY_TEXT())
                        .font_weight(FontWeight::BOLD)
                        .child(t(lang, Translations::NAV_ONE)),
                ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .bg(GHOST_SURFACE_BG())
                            .border_1()
                            .border_color(BORDER_LIGHT())
                            .text_xs()
                            .text_color(SECONDARY_TEXT())
                            .cursor_pointer()
                            .on_mouse_down(
                                gpui::MouseButton::Left,
                                cx.listener(|this, _: &gpui::MouseDownEvent, _window, cx| {
                                    this.toggle_theme(&ToggleTheme, _window, cx);
                                }),
                            )
                            .child(theme_label),
                    )
                    .child(
                        div()
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .bg(GHOST_SURFACE_BG())
                            .border_1()
                            .border_color(BORDER_LIGHT())
                            .text_xs()
                            .text_color(SECONDARY_TEXT())
                            .cursor_pointer()
                            .on_mouse_down(
                                gpui::MouseButton::Left,
                                cx.listener(|this, _: &gpui::MouseDownEvent, _window, cx| {
                                    this.toggle_lang(&ToggleLang, _window, cx);
                                }),
                            )
                            .child(lang.label()),
                    ),
            )
    }

    fn render_window_titlebar(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let header_content = match self.main_view {
            MainView::Chat => {
                let lang = self.current_lang;
                let title = if let Some(task) = self.get_active_task() {
                    if task.title.trim().is_empty() {
                        t(lang, Translations::NEW_TASK).to_string()
                    } else {
                        task.title.clone()
                    }
                } else {
                    t(lang, Translations::NO_TASK_SELECTED).to_string()
                };
                self.render_chat_header(
                    title,
                    self.get_work_dir(),
                    self.sidebar_visible,
                    self.terminal_visible,
                    cx,
                )
                .into_any_element()
            }
            MainView::SkillsMarket => skills_market::render_skills_market_titlebar(&*self, window, cx)
        };

        div()
            .flex()
            .flex_none()
            .h(px(TITLEBAR_HEIGHT))
            .border_b_1()
            .border_color(BORDER_LIGHT())
            .on_mouse_down_out(cx.listener(|this, _ev, _window, _cx| {
                this.titlebar_should_move = false;
            }))
            .on_mouse_up(
                gpui::MouseButton::Left,
                cx.listener(|this, _ev, _window, _cx| {
                    this.titlebar_should_move = false;
                }),
            )
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _ev, _window, _cx| {
                    this.titlebar_should_move = true;
                }),
            )
            .on_mouse_move(cx.listener(|this, _ev, window, _cx| {
                if this.titlebar_should_move {
                    this.titlebar_should_move = false;
                    window.start_window_move();
                }
            }))
            .child(
                div()
                    .w(px(NAV_WIDTH))
                    .h_full()
                    .bg(NAV_BG())
                    .child(self.render_titlebar_leading(cx)),
            )
            .child(div().w(px(1.0)).bg(BORDER_LIGHT()))
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .bg(HEADER_BG())
                    .child(header_content),
            )
    }

    fn render_nav_buttons(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let lang = self.current_lang;
        let skills_active = matches!(self.main_view, MainView::SkillsMarket);
        let models_active = self.show_model_config_dialog;
        let mut nav = div().flex().flex_col().gap_1().px_4().py_3();

        nav = nav.child(self.make_nav_item(
            t(lang, Translations::NEW_WORKSPACE).to_string(),
            "⌘N".to_string(),
            "workspace",
            false,
            cx,
        ));
        nav = nav.child(self.make_nav_item(
            t(lang, Translations::CAPABILITIES).to_string(),
            "⌘K".to_string(),
            "capabilities",
            skills_active,
            cx,
        ));
        nav = nav.child(self.make_nav_item(
            t(lang, Translations::MODELS).to_string(),
            "⌘M".to_string(),
            "models",
            models_active,
            cx,
        ));

        nav
    }

    fn make_nav_item(
        &mut self,
        title: String,
        shortcut: String,
        icon_key: &'static str,
        active: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let is_new_workspace = title == t(self.current_lang, Translations::NEW_WORKSPACE);
        let is_skills = title == t(self.current_lang, Translations::CAPABILITIES);
        let is_model_config = title == t(self.current_lang, Translations::MODELS);

        div()
            .flex()
            .items_center()
            .gap_3()
            .px_1()
            .py_1()
            .cursor_pointer()
            .hover(|this| this.opacity(0.92))
            .when(is_new_workspace, |this| {
                this.on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(|this, _: &gpui::MouseDownEvent, _window, cx| {
                        this.handle_new_workspace_click(cx);
                    }),
                )
            })
            .when(is_skills, |this| {
                this.on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(|this, _: &gpui::MouseDownEvent, _window, cx| {
                        this.open_skills_market(cx);
                    }),
                )
            })
            .when(is_model_config, |this| {
                this.on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(|this, _: &gpui::MouseDownEvent, _window, cx| {
                        this.open_model_config_dialog(&OpenModelConfigDialog, _window, cx);
                    }),
                )
            })
            .child(self.make_icon_slot(icon_key, active))
            .child(
                div()
                    .flex_1()
                    .overflow_hidden()
                    .text_sm()
                    .text_color(if active {
                        PRIMARY_TEXT()
                    } else {
                        SECONDARY_TEXT()
                    })
                    .font_weight(FontWeight::BOLD)
                    .text_ellipsis()
                    .child(title),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(if active {
                        TERTIARY_TEXT()
                    } else {
                        MUTED_TEXT()
                    })
                    .child(shortcut),
            )
    }

    fn make_footer_action_item(
        &mut self,
        title: String,
        icon_key: &'static str,
    ) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .gap_3()
            .px_1()
            .py_1()
            .opacity(0.88)
            .child(self.make_icon_slot(icon_key, false))
            .child(
                div()
                    .flex_1()
                    .text_sm()
                    .text_color(SECONDARY_TEXT())
                    .child(title),
            )
    }

    fn render_nav_footer_actions(&mut self) -> impl IntoElement {
        let lang = self.current_lang;
        div()
            .flex()
            .flex_col()
            .gap_1()
            .px_4()
            .py_3()
            .child(
                self.make_footer_action_item(
                    t(lang, Translations::SETTINGS).to_string(),
                    "settings",
                ),
            )
            .child(
                self.make_footer_action_item(t(lang, Translations::SUPPORT).to_string(), "support"),
            )
            .child(div().h(px(40.0)))
    }

    fn make_icon_slot(&mut self, icon_key: &'static str, active: bool) -> impl IntoElement {
        div()
            .w(px(16.0))
            .h(px(16.0))
            .flex()
            .items_center()
            .justify_center()
            .child(render_icon_element(
                icon_key,
                if active {
                    PRIMARY_TEXT()
                } else {
                    SECONDARY_TEXT()
                },
                14.0,
            ))
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
                // 不存在 → 创建新 workspace + 默认 task
                self.add_workspace(path, name);
                if let Some(ws_id) = self.active_workspace_id {
                    self.active_task_id = self.ensure_workspace_draft_task(ws_id);
                    self.restore_task_context();
                    cx.notify();
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
        self.ensure_default_workspace();

        let workspaces = self.workspaces.clone();
        let active_workspace_id = self.active_workspace_id;
        let active_task_id = self.active_task_id;

        let mut result = div()
            .flex()
            .flex_col()
            .px_4()
            .pb_3()
            .gap_3()
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(move |this, _: &gpui::MouseDownEvent, _window, _cx| {
                    this.delete_confirm_workspace_id = None;
                }),
            );

        for workspace in workspaces {
            let is_active_ws = active_workspace_id == Some(workspace.id);
            let ws_id = workspace.id;

            let ws_row = div()
                .flex()
                .items_center()
                .gap_2()
                .px_2()
                .py_1()
                .cursor_pointer()
                .hover(|this| this.opacity(0.94))
                .on_mouse_move(
                    cx.listener(move |this, _: &gpui::MouseMoveEvent, _window, _cx| {
                        this.hovered_workspace_id = Some(ws_id);
                    }),
                )
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(move |this, _: &gpui::MouseDownEvent, _window, _cx| {
                        this.active_workspace_id = Some(ws_id);
                        if let Some(ws) = this.workspaces.iter_mut().find(|w| w.id == ws_id) {
                            ws.expanded = !ws.expanded;
                            task_db::update_workspace_expanded(&this.db.conn, ws_id, ws.expanded)
                                .ok();
                        }
                    }),
                );

            let expand_btn = div().size(px(16.0)).flex().items_center().justify_center();

            let add_btn = div()
                .text_sm()
                .text_color(MUTED_TEXT())
                .px_1()
                .py_1()
                .opacity(0.72)
                .cursor_pointer()
                .id(format!("add-btn-{}", ws_id))
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(move |this, _: &gpui::MouseDownEvent, _window, cx| {
                        cx.stop_propagation();
                        this.active_workspace_id = Some(ws_id);
                        this.active_task_id = this.ensure_workspace_draft_task(ws_id);
                        this.restore_task_context();
                        cx.notify();
                    }),
                );

            let ws_label = workspace.name.clone();

            let more_btn = div()
                .id(format!("more-btn-{}", ws_id))
                .px_1()
                .py_1()
                .opacity(0.72)
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(
                        move |this,
                              event: &gpui::MouseDownEvent,
                              _window: &mut Window,
                              cx: &mut Context<Self>| {
                            cx.stop_propagation();
                            this.delete_confirm_workspace_id = Some(ws_id);
                            this.popup_position = event.position;
                        },
                    ),
                )
                .child(
                    svg()
                        .path("more.svg")
                        .size(px(16.0))
                        .flex_none()
                        .text_color(MUTED_TEXT()),
                );

            let action_div = div()
                .ml_auto()
                .flex()
                .items_center()
                .gap_2()
                .child(more_btn)
                .child(add_btn.child("+"));

            result = result.child(
                ws_row
                    .child(
                        svg()
                            .path("folder.svg")
                            .size(px(16.0))
                            .flex_none()
                            .text_color(if is_active_ws {
                                BRAND_BLUE()
                            } else {
                                SECONDARY_TEXT()
                            }),
                    )
                    .child(if is_active_ws {
                        div()
                            .text_sm()
                            .ml_1()
                            .text_color(PRIMARY_TEXT())
                            .font_weight(FontWeight::BOLD)
                            .child(ws_label)
                    } else {
                        div()
                            .text_sm()
                            .ml_1()
                            .text_color(SECONDARY_TEXT())
                            .child(ws_label)
                    })
                    .child(if workspace.expanded {
                        expand_btn.child(
                            svg()
                                .path("expand.svg")
                                .size(px(16.0))
                                .flex_none()
                                .text_color(MUTED_TEXT()),
                        )
                    } else {
                        expand_btn.child(
                            svg()
                                .path("fold.svg")
                                .size(px(16.0))
                                .flex_none()
                                .text_color(MUTED_TEXT()),
                        )
                    })
                    .child(
                        div()
                            .ml_auto()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(action_div),
                    ),
            );

            if workspace.expanded {
                let mut tasks_container = div()
                    .flex_col()
                    .ml_4()
                    .pl_3()
                    .border_l_1()
                    .border_color(GHOST_SURFACE_BG())
                    .gap_1();

                for task in &workspace.tasks {
                    let is_active_task = active_task_id == Some(task.id)
                        && active_workspace_id == Some(workspace.id);

                    let mut task_div = div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .w_full()
                        .px_2()
                        .py_1()
                        .cursor_pointer()
                        .bg(if is_active_task {
                            GHOST_SURFACE_BG()
                        } else {
                            NAV_BG()
                        })
                        .hover(|this| this.opacity(0.94));

                    let task_id = task.id;
                    let ws_id = workspace.id;
                    let lang = self.current_lang;
                    let title_display = if task.title.trim().is_empty() {
                        t(lang, Translations::NEW_TASK).to_string()
                    } else {
                        task.title.trim().to_string()
                    };

                    task_div = task_div.on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(move |this, _: &gpui::MouseDownEvent, _window, cx| {
                            this.active_workspace_id = Some(ws_id);
                            this.active_task_id = Some(task_id);
                            this.restore_task_context();
                            this.main_view = MainView::Chat;
                            cx.notify();
                        }),
                    );

                    tasks_container = tasks_container.child(
                        task_div
                            .child(div().w(px(2.0)).h(px(18.0)).rounded_full().bg(
                                if is_active_task {
                                    BRAND_BLUE()
                                } else {
                                    BORDER_LIGHT()
                                },
                            ))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(if is_active_task {
                                        TERTIARY_TEXT()
                                    } else {
                                        MUTED_TEXT()
                                    })
                                    .child(""),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .overflow_hidden()
                                    .text_sm()
                                    .text_color(if is_active_task {
                                        PRIMARY_TEXT()
                                    } else {
                                        SECONDARY_TEXT()
                                    })
                                    .text_ellipsis()
                                    .child(title_display.clone()),
                            )
                            .child(
                                div()
                                    .ml_auto()
                                    .text_xs()
                                    .text_color(MUTED_TEXT())
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        gpui::MouseButton::Left,
                                        cx.listener(
                                            move |this, _: &gpui::MouseDownEvent, _window, cx| {
                                                cx.stop_propagation();
                                                if let Some(ws) = this
                                                    .workspaces
                                                    .iter_mut()
                                                    .find(|w| w.id == ws_id)
                                                {
                                                    let was_draft = ws
                                                        .tasks
                                                        .iter()
                                                        .find(|t| t.id == task_id)
                                                        .map(|t| t.is_draft)
                                                        .unwrap_or(false);
                                                    let was_active =
                                                        this.active_task_id == Some(task_id);
                                                    ws.tasks.retain(|t| t.id != task_id);
                                                    task_db::delete_task(&this.db.conn, task_id)
                                                        .ok();

                                                    if was_draft || was_active {
                                                        if let Ok(rows) = task_db::load_tasks(
                                                            &this.db.conn,
                                                            ws_id,
                                                        ) {
                                                            ws.tasks = rows
                                                                .into_iter()
                                                                .map(|t| TaskItem {
                                                                    id: t.id,
                                                                    title: t.title,
                                                                    is_draft: t.is_draft,
                                                                })
                                                                .collect();
                                                        }
                                                    }

                                                    if was_active {
                                                        this.active_task_id = ws
                                                            .tasks
                                                            .iter()
                                                            .find(|t| t.is_draft)
                                                            .map(|t| t.id)
                                                            .or_else(|| {
                                                                ws.tasks.first().map(|t| t.id)
                                                            });
                                                        this.restore_task_context();
                                                    }
                                                    cx.notify();
                                                }
                                            },
                                        ),
                                    )
                                    .child("×"),
                            ),
                    );
                }

                result = result.child(tasks_container);
            }
        }

        result
    }

    fn render_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let scroll_handle = self.chat_scroll_handle.clone();

        div()
            .flex()
            .flex_col()
            .flex_1()
            .h_full()
            .min_w(px(350.0))
            .bg(CANVAS_BG())
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .overflow_hidden()
                    .px_8()
                    .pb_5()
                    .child(
                        div()
                            .id("chat_container")
                            .flex_1()
                            .w_full()
                            .overflow_scroll()
                            .track_scroll(&scroll_handle)
                            .pt_8()
                            .pb_6()
                            .child(
                                div().flex().justify_center().w_full().child(
                                    div().w_full().max_w(px(940.0)).child(
                                        self.render_chat_messages(&scroll_handle, window, cx),
                                    ),
                                ),
                            ),
                    )
                    .child(div().flex_none().child(self.render_composer(window, cx))),
            )
    }

    fn render_chat_header(
        &mut self,
        title: String,
        work_dir: String,
        sidebar_visible: bool,
        terminal_visible: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let lang = self.current_lang;
        if title.contains('\n') || title.contains('\r') {
            eprintln!(
                "[CHAT-HEADER] title_has_newline title='{}'",
                escape_visible_snippet(&title, 120)
            );
        }
        let title = normalize_single_line_label(&title);
        div()
            .flex()
            .items_center()
            .justify_between()
            .h_full()
            .px_8()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_6()
                    .flex_1()
                    .child(
                        div()
                            .w(px(320.0))
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap_2()
                            .overflow_hidden()
                            .child(
                                div()
                                    .text_size(px(18.0))
                                    .text_color(PRIMARY_TEXT())
                                    .font_weight(FontWeight::BOLD)
                                    .text_ellipsis()
                                    .child(title),
                            )
                            .child(div().child(render_icon_element(
                                "assistant",
                                SECONDARY_TEXT(),
                                13.0,
                            ))),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_4()
                            .child(self.make_header_tab(t(lang, Translations::EXPLORER), true))
                            .child(
                                self.make_placeholder_header_tab(t(lang, Translations::WORKFLOWS)),
                            )
                            .child(self.make_placeholder_header_tab(t(lang, Translations::API))),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .id("export-btn")
                            .cursor_pointer()
                            .text_xs()
                            .text_color(SECONDARY_TEXT())
                            .font_weight(FontWeight::BOLD)
                            .on_mouse_down(
                                gpui::MouseButton::Left,
                                cx.listener(|this, _: &gpui::MouseDownEvent, _window, cx| {
                                    this.export_chat(&ExportChat, _window, cx);
                                }),
                            )
                            .child(t(lang, Translations::EXPORT)),
                    )
                    .child(self.make_chat_header_button(
                        "folder",
                        false,
                        Some("open-work-dir"),
                        Some(work_dir.clone()),
                        cx,
                    ))
                    .child(self.make_chat_header_button("share", false, None, None, cx))
                    .child(self.make_chat_header_button(
                        "terminal",
                        terminal_visible,
                        Some("terminal"),
                        None,
                        cx,
                    ))
                    .child(self.make_chat_header_button(
                        "run-panel",
                        sidebar_visible,
                        Some("sidebar"),
                        None,
                        cx,
                    ))
                    .child(
                        div()
                            .size(px(34.0))
                            .rounded_full()
                            .bg(AVATAR_BG())
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_xs()
                            .text_color(gpui::white())
                            .font_weight(FontWeight::BOLD)
                            .child("U"),
                    ),
            )
    }

    fn make_header_tab(&mut self, label: &'static str, active: bool) -> impl IntoElement {
        div()
            .pb_3()
            .border_b_1()
            .border_color(if active {
                BRAND_BLUE()
            } else {
                Hsla {
                    h: 0.0,
                    s: 0.0,
                    l: 0.0,
                    a: 0.0,
                }
            })
            .text_xs()
            .text_color(if active {
                ACCENT_TEXT()
            } else {
                SECONDARY_TEXT()
            })
            .font_weight(FontWeight::BOLD)
            .child(label)
    }

    fn make_placeholder_header_tab(&mut self, label: &'static str) -> impl IntoElement {
        div()
            .text_xs()
            .text_color(MUTED_TEXT())
            .opacity(0.88)
            .child(label)
    }

    fn make_chat_header_button(
        &mut self,
        icon_key: &'static str,
        active: bool,
        action: Option<&'static str>,
        tooltip_text: Option<String>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let action_key = action;
        let tooltip = tooltip_text.clone();
        let button = div()
            .id(format!(
                "chat-header-btn-{}-{}",
                icon_key,
                action_key.unwrap_or("none")
            ))
            .size(px(30.0))
            .rounded_full()
            .bg(if active {
                GHOST_SURFACE_BG()
            } else {
                HEADER_BG()
            })
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .text_xs()
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(
                    move |this, _: &gpui::MouseDownEvent, _window, _cx| match action {
                        Some("open-work-dir") => {
                            if let Some(path) = tooltip.as_deref() {
                                if !path.trim().is_empty() {
                                    this.open_folder_in_finder(path);
                                }
                            }
                        }
                        Some("terminal") => this.terminal_visible = !this.terminal_visible,
                        Some("sidebar") => this.sidebar_visible = !this.sidebar_visible,
                        _ => {}
                    },
                ),
            )
            .child(render_icon_element(
                icon_key,
                if active {
                    ACCENT_TEXT()
                } else {
                    SECONDARY_TEXT()
                },
                13.0,
            ));
        if let Some(tooltip_text) = tooltip_text.filter(|text| !text.trim().is_empty()) {
            button.tooltip(move |_, cx| {
                cx.new(|_| HeaderTooltip {
                    text: tooltip_text.clone(),
                })
                .into()
            })
        } else {
            match action_key {
                Some("open-work-dir") => button.tooltip(move |_, cx| {
                    cx.new(|_| HeaderTooltip {
                        text: "No working directory".to_string(),
                    })
                    .into()
                }),
                _ => button,
            }
        }
    }

    fn render_model_config_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let lang = self.current_lang;
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
            editor.set_placeholder_text(t(lang, Translations::API_KEY_PLACEHOLDER), window, cx);
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
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _: &gpui::MouseDownEvent, _window, cx| {
                    this.cancel_model_config(&CancelModelConfig, _window, cx);
                }),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .w(px(400.0))
                    .p_5()
                    .bg(SURFACE_PANEL())
                    .rounded_xl()
                    .border_1()
                    .border_color(BORDER_LIGHT())
                    .shadow_md()
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|_, _: &gpui::MouseDownEvent, _window, _cx| {}),
                    )
                    .child(
                        div()
                            .text_base()
                            .text_color(PRIMARY_TEXT())
                            .font_weight(FontWeight::BOLD)
                            .child(t(lang, Translations::MODEL_SERVICE_CONFIG)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(SECONDARY_TEXT())
                                    .child(t(lang, Translations::MODEL_NAME)),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .h(px(36.0))
                                    .px_3()
                                    .rounded_lg()
                                    .border_1()
                                    .border_color(BORDER_LIGHT())
                                    .bg(CANVAS_BG())
                                    .track_focus(&model_name_focus)
                                    .child(model_name_editor.clone()),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(SECONDARY_TEXT())
                                    .child(t(lang, Translations::BASE_URL)),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .h(px(36.0))
                                    .px_3()
                                    .rounded_lg()
                                    .border_1()
                                    .border_color(BORDER_LIGHT())
                                    .bg(CANVAS_BG())
                                    .track_focus(&base_url_focus)
                                    .child(base_url_editor.clone()),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(SECONDARY_TEXT())
                                    .child(t(lang, Translations::API_KEY)),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .h(px(36.0))
                                    .px_3()
                                    .rounded_lg()
                                    .border_1()
                                    .border_color(BORDER_LIGHT())
                                    .bg(CANVAS_BG())
                                    .track_focus(&api_key_focus)
                                    .child(api_key_editor.clone()),
                            ),
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
                                    .rounded_lg()
                                    .border_1()
                                    .border_color(BORDER_LIGHT())
                                    .bg(CANVAS_BG())
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        gpui::MouseButton::Left,
                                        cx.listener(
                                            |this, _: &gpui::MouseDownEvent, _window, cx| {
                                                this.cancel_model_config(
                                                    &CancelModelConfig,
                                                    _window,
                                                    cx,
                                                );
                                            },
                                        ),
                                    )
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(PRIMARY_TEXT())
                                            .child(t(lang, Translations::CANCEL)),
                                    ),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .h(px(36.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_lg()
                                    .bg(BRAND_BLUE())
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        gpui::MouseButton::Left,
                                        cx.listener(
                                            move |this, _: &gpui::MouseDownEvent, _window, cx| {
                                                if let Some(editor) = weak_model_name.upgrade() {
                                                    this.editing_model_name = editor
                                                        .read_with(cx, |editor, cx| editor.text(cx))
                                                        .trim()
                                                        .to_string();
                                                }
                                                if let Some(editor) = weak_base_url.upgrade() {
                                                    this.editing_base_url = editor
                                                        .read_with(cx, |editor, cx| editor.text(cx))
                                                        .trim()
                                                        .to_string();
                                                }
                                                if let Some(editor) = weak_api_key.upgrade() {
                                                    this.editing_api_key = editor
                                                        .read_with(cx, |editor, cx| editor.text(cx))
                                                        .trim()
                                                        .to_string();
                                                }
                                                this.save_model_config(
                                                    &SaveModelConfig,
                                                    _window,
                                                    cx,
                                                );
                                            },
                                        ),
                                    )
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(gpui::white())
                                            .child(t(lang, Translations::SAVE)),
                                    ),
                            ),
                    ),
            )
    }

    fn render_workspace_popup(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let lang = self.current_lang;
        let ws_id = self.delete_confirm_workspace_id.unwrap_or(0);
        let pos = self.popup_position;
        div()
            .absolute()
            .left(pos.x)
            .top(pos.y)
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _: &gpui::MouseDownEvent, _window, _cx| {
                    this.delete_confirm_workspace_id = None;
                }),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .w(px(180.0))
                    .p_3()
                    .bg(SURFACE_PANEL())
                    .rounded_xl()
                    .border_1()
                    .border_color(BORDER_LIGHT())
                    .shadow_md()
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|_, _: &gpui::MouseDownEvent, _window, _cx| {}),
                    )
                    .child(
                        div()
                            .px_3()
                            .py_2()
                            .text_sm()
                            .text_color(PRIMARY_TEXT())
                            .rounded_md()
                            .cursor_pointer()
                            .hover(|this| this.bg(ACTIVE_BG()))
                            .on_mouse_down(
                                gpui::MouseButton::Left,
                                cx.listener(move |this, _: &gpui::MouseDownEvent, _window, cx| {
                                    cx.stop_propagation();
                                    this.active_workspace_id = Some(ws_id);
                                    this.active_task_id = this.ensure_workspace_draft_task(ws_id);
                                    this.delete_confirm_workspace_id = None;
                                    this.restore_task_context();
                                    cx.notify();
                                }),
                            )
                            .child(t(lang, Translations::NEW_TASK)),
                    )
                    .child(
                        div()
                            .px_3()
                            .py_2()
                            .text_sm()
                            .text_color(PRIMARY_TEXT())
                            .rounded_md()
                            .cursor_pointer()
                            .hover(|this| this.bg(ACTIVE_BG()))
                            .on_mouse_down(
                                gpui::MouseButton::Left,
                                cx.listener(move |this, _: &gpui::MouseDownEvent, _window, cx| {
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
                                }),
                            )
                            .child(t(lang, Translations::DELETE_WORKSPACE)),
                    ),
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
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _: &gpui::MouseDownEvent, _window, cx| {
                    this.show_export_dialog = false;
                    this.exported_json = None;
                    this.exported_md = None;
                    cx.notify();
                }),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .w(px(500.0))
                    .h(px(400.0))
                    .p_5()
                    .bg(SURFACE_PANEL())
                    .rounded_xl()
                    .border_1()
                    .border_color(BORDER_LIGHT())
                    .shadow_md()
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|_, _: &gpui::MouseDownEvent, _window, _cx| {}),
                    )
                    .child(
                        div()
                            .text_base()
                            .text_color(PRIMARY_TEXT())
                            .font_weight(FontWeight::BOLD)
                            .child(t(lang, Translations::EXPORT)),
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
                                    .rounded_lg()
                                    .bg(CANVAS_BG())
                                    .border_1()
                                    .border_color(BORDER_LIGHT())
                                    .overflow_hidden()
                                    .text_xs()
                                    .text_color(PRIMARY_TEXT())
                                    .child(format!(
                                        "{}:\n{}",
                                        t(lang, Translations::JSON),
                                        json_content
                                    )),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .h(px(200.0))
                                    .p_3()
                                    .rounded_lg()
                                    .bg(CANVAS_BG())
                                    .border_1()
                                    .border_color(BORDER_LIGHT())
                                    .overflow_hidden()
                                    .text_xs()
                                    .text_color(PRIMARY_TEXT())
                                    .child(format!(
                                        "{}:\n{}",
                                        t(lang, Translations::MARKDOWN),
                                        md_content
                                    )),
                            ),
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
                                    .rounded_lg()
                                    .border_1()
                                    .border_color(BORDER_LIGHT())
                                    .bg(CANVAS_BG())
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        gpui::MouseButton::Left,
                                        cx.listener(
                                            |this, _: &gpui::MouseDownEvent, _window, cx| {
                                                if let Some(json) = this.exported_json.clone() {
                                                    if let Some(path) = rfd::FileDialog::new()
                                                        .set_title(t(
                                                            this.current_lang,
                                                            Translations::EXPORT_JSON_TITLE,
                                                        ))
                                                        .add_filter(
                                                            t(
                                                                this.current_lang,
                                                                Translations::JSON,
                                                            ),
                                                            &["json"],
                                                        )
                                                        .save_file()
                                                    {
                                                        std::fs::write(&path, json).ok();
                                                    }
                                                }
                                                this.show_export_dialog = false;
                                                this.exported_json = None;
                                                this.exported_md = None;
                                                cx.notify();
                                            },
                                        ),
                                    )
                                    .child(t(lang, Translations::SAVE_JSON)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .h(px(36.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_lg()
                                    .border_1()
                                    .border_color(BORDER_LIGHT())
                                    .bg(CANVAS_BG())
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        gpui::MouseButton::Left,
                                        cx.listener(
                                            |this, _: &gpui::MouseDownEvent, _window, cx| {
                                                if let Some(md) = this.exported_md.clone() {
                                                    if let Some(path) = rfd::FileDialog::new()
                                                        .set_title(t(
                                                            this.current_lang,
                                                            Translations::EXPORT_MARKDOWN_TITLE,
                                                        ))
                                                        .add_filter(
                                                            t(
                                                                this.current_lang,
                                                                Translations::MARKDOWN,
                                                            ),
                                                            &["md"],
                                                        )
                                                        .save_file()
                                                    {
                                                        std::fs::write(&path, md).ok();
                                                    }
                                                }
                                                this.show_export_dialog = false;
                                                this.exported_json = None;
                                                this.exported_md = None;
                                                cx.notify();
                                            },
                                        ),
                                    )
                                    .child(t(lang, Translations::SAVE_MARKDOWN)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .h(px(36.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_lg()
                                    .bg(BRAND_BLUE())
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        gpui::MouseButton::Left,
                                        cx.listener(
                                            |this, _: &gpui::MouseDownEvent, _window, _cx| {
                                                this.show_export_dialog = false;
                                                this.exported_json = None;
                                                this.exported_md = None;
                                            },
                                        ),
                                    )
                                    .child(t(lang, Translations::CANCEL)),
                            ),
                    ),
            )
    }

    fn render_chat_messages(
        &mut self,
        scroll_handle: &ScrollHandle,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let messages = self.messages.clone();
        let live_run = self
            .current_claude_run
            .as_ref()
            .filter(|run| run.task_id == self.active_task_id && run.show_live_bubble)
            .cloned();
        let general_ai_live_run_id = self.general_ai_run_id.filter(|_| {
            self.general_ai_show_live_bubble && self.general_ai_task_id == self.active_task_id
        });
        let general_ai_pending = self.request_in_flight
            && matches!(self.request_kind, Some(RequestKind::GeneralAi))
            && live_run.is_none()
            && general_ai_live_run_id.is_none();
        let is_user = |role: &str| role == "user";
        let lang = self.current_lang;

        // Auto-scroll to bottom only when needs_auto_scroll is set
        if self.needs_auto_scroll && !messages.is_empty() {
            scroll_handle.scroll_to_bottom();
            self.needs_auto_scroll = false;
        }

        let mut message_list = div()
            .flex_col()
            .gap_8()
            .w_full()
            .children(messages.iter().enumerate().map(|(msg_index, msg)| {
                let task_id = self.active_task_id.unwrap_or_default();
                let is_user_msg = is_user(&msg.role);
                let bubble_bg = if is_user_msg {
                    USER_BUBBLE_BG()
                } else {
                    ASSISTANT_BUBBLE_BG()
                };
                let text_color = if is_user_msg { gpui::white() } else { PRIMARY_TEXT() };
                let role_label = if is_user_msg { t(lang, Translations::YOU) } else { "ONE AI" };

                // Parse content for think tags
                let mut parts = parse_think_content(&msg.content);

                // If this is an assistant message and we have intent_content_parts, insert them first
                if !is_user_msg && !self.intent_content_parts.is_empty() {
                    let mut intent_parts = self.intent_content_parts.clone();
                    intent_parts.append(&mut parts);
                    parts = intent_parts;
                }

                // User messages: right aligned, Assistant messages: left aligned
                let message_container = if is_user_msg {
                    div()
                        .flex()
                        .justify_end()
                        .w_full()
                        .mb_4()
                        .child(
                            div()
                                .flex_col()
                                .items_end()
                                .gap_2()
                                .px_4()
                                .py_3()
                                .rounded_xl()
                                .bg(bubble_bg)
                                .border_1()
                                .border_color(SURFACE_ACCENT())
                                .max_w(px(640.0))
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
                    let mut think_index = 0usize;
                    div()
                        .flex_col()
                        .items_start()
                        .gap_2()
                        .w_full()
                        .mb_4()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    div()
                                        .size(px(22.0))
                                        .rounded_full()
                                        .bg(AVATAR_BG())
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(render_icon_element("assistant", gpui::white(), 11.0))
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(SECONDARY_TEXT())
                                        .font_weight(FontWeight::BOLD)
                                        .child(role_label)
                                )
                        )
                        .child(
                            div()
                                .flex_col()
                                .items_start()
                                .gap_4()
                                .max_w(px(780.0))
                                .min_w(px(35.0))
                                .w_full()
                                .pl_8()
                                .pt_4()
                                .children({
                                    let mut rendered_parts: Vec<gpui::AnyElement> = Vec::new();
                                    let mut prev_was_think = false;
                                    for part in &parts {
                                        match part {
                                            ContentPart::Normal(text) => {
                                                let add_top_padding = prev_was_think;
                                                prev_was_think = false;
                                                let el = div()
                                                    .text_base()
                                                    .text_color(text_color)
                                                    .whitespace_normal()
                                                    .child(text.clone());
                                                let el = if add_top_padding { el.pt_1() } else { el };
                                                rendered_parts.push(el.into_any_element());
                                            }
                                            ContentPart::ProcessTable { processes } => {
                                                prev_was_think = false;
                                                let el = self.render_process_table(processes, cx);
                                                rendered_parts.push(el.into_any_element());
                                            }
                                            ContentPart::IntentUnderstanding { text, complete } => {
                                                prev_was_think = true;
                                                let key = format!("task:{}:intent", task_id);
                                                let collapsed = self
                                                    .think_collapsed
                                                    .get(&key)
                                                    .copied()
                                                    .unwrap_or(*complete);
                                                let header_text = if *complete {
                                                    t(lang, Translations::INTENT_UNDERSTOOD)
                                                } else {
                                                    t(lang, Translations::UNDERSTANDING_INTENT)
                                                };
                                                let icon_path = if collapsed { "fold.svg" } else { "expand.svg" };

                                                let el = div()
                                                    .flex_col()
                                                    .w_full()
                                                    .child(
                                                        div()
                                                            .flex()
                                                            .items_center()
                                                            .gap_2()
                                                            .px_2()
                                                            .py_1()
                                                            .rounded_md()
                                                            .bg(GHOST_SURFACE_BG())
                                                            .cursor_pointer()
                                                            .on_mouse_down(gpui::MouseButton::Left, cx.listener(move |this, _: &gpui::MouseDownEvent, _window, cx| {
                                                                let next = !this.think_collapsed.get(&key).copied().unwrap_or(false);
                                                                this.think_collapsed.insert(key.clone(), next);
                                                                cx.notify();
                                                            }))
                                                            .child(
                                                                svg()
                                                                    .path(icon_path)
                                                                    .size(px(14.0))
                                                                    .flex_none()
                                                                    .text_color(MUTED_TEXT())
                                                            )
                                                            .child(
                                                                div()
                                                                    .text_xs()
                                                                    .text_color(MUTED_TEXT())
                                                                    .child(header_text)
                                                            )
                                                    )
                                                    .when(!collapsed, |this| {
                                                        this.child(
                                                            div()
                                                                .pr_2()
                                                                .text_xs()
                                                                .text_color(TERTIARY_TEXT())
                                                                .whitespace_normal()
                                                                .child(text.clone())
                                                        )
                                                    });
                                                rendered_parts.push(el.into_any_element());
                                            }
                                            ContentPart::Think { text, complete } => {
                                                prev_was_think = true;
                                                let current_think_index = think_index;
                                                think_index += 1;
                                                let complete = *complete;
                                                let key = format!("task:{}:msg:{}:think:{}", task_id, msg_index, current_think_index);
                                                let collapsed = self
                                                    .think_collapsed
                                                    .get(&key)
                                                    .copied()
                                                    .unwrap_or(complete);
                                                let header_text = if complete {
                                                    t(lang, Translations::THINKING_DONE)
                                                } else {
                                                    t(lang, Translations::THINKING_IN_PROGRESS)
                                                };
                                                let icon_path = if collapsed { "fold.svg" } else { "expand.svg" };
                                                let default_collapsed = complete;

                                                let el = div()
                                                    .flex_col()
                                                    .w_full()
                                                    .child(
                                                        div()
                                                            .flex()
                                                            .items_center()
                                                            .gap_2()
                                                            .px_2()
                                                            .py_1()
                                                            .rounded_md()
                                                            .bg(GHOST_SURFACE_BG())
                                                            .cursor_pointer()
                                                            .on_mouse_down(gpui::MouseButton::Left, cx.listener(move |this, _: &gpui::MouseDownEvent, _window, cx| {
                                                                let next = !this.think_collapsed.get(&key).copied().unwrap_or(default_collapsed);
                                                                this.think_collapsed.insert(key.clone(), next);
                                                                cx.notify();
                                                            }))
                                                            .child(
                                                                svg()
                                                                    .path(icon_path)
                                                                    .size(px(14.0))
                                                                    .flex_none()
                                                                    .text_color(MUTED_TEXT())
                                                            )
                                                            .child(
                                                                div()
                                                                    .text_xs()
                                                                    .text_color(MUTED_TEXT())
                                                                    .child(header_text)
                                                            )
                                                    )
                                                    .when(!collapsed, |this| {
                                                        this.child(
                                                            div()
                                                                .pr_2()
                                                                .text_xs()
                                                                .text_color(TERTIARY_TEXT())
                                                                .whitespace_normal()
                                                                .child(text.clone())
                                                        )
                                                    });
                                                rendered_parts.push(el.into_any_element());
                                            }
                                        }
                                    }

                                    if !is_user_msg && self.pending_confirmation_tools.is_some() {
                                        let confirm_buttons = div()
                                            .flex()
                                            .gap_3()
                                            .mt_4()
                                            .child(
                                                div()
                                                    .px_4()
                                                    .py_2()
                                                    .rounded_md()
                                                    .bg(gpui::green())
                                                    .text_color(gpui::white())
                                                    .text_sm()
                                                    .font_weight(FontWeight::BOLD)
                                                    .cursor_pointer()
                                                    .on_mouse_down(gpui::MouseButton::Left, cx.listener(move |this, _: &gpui::MouseDownEvent, _window, cx| {
                                                        this.confirm_system_tools_operation(true, cx);
                                                    }))
                                                    .child("确认执行")
                                            )
                                            .child(
                                                div()
                                                    .px_4()
                                                    .py_2()
                                                    .rounded_md()
                                                    .border_1()
                                                    .border_color(BORDER_LIGHT())
                                                    .text_color(SECONDARY_TEXT())
                                                    .text_sm()
                                                    .font_weight(FontWeight::BOLD)
                                                    .cursor_pointer()
                                                    .on_mouse_down(gpui::MouseButton::Left, cx.listener(move |this, _: &gpui::MouseDownEvent, _window, cx| {
                                                        this.confirm_system_tools_operation(false, cx);
                                                    }))
                                                    .child("取消")
                                            );
                                        rendered_parts.push(confirm_buttons.into_any_element());
                                    }

                                    rendered_parts
                                })
                        )
                };

                message_container
            }));

        if let Some(run) = live_run.as_ref() {
            message_list = message_list.child(self.render_claude_live_message(run, cx));
        }

        if let Some(run_id) = general_ai_live_run_id {
            message_list = message_list.child(self.render_general_ai_live_message(run_id, cx));
        }

        if general_ai_pending {
            message_list = message_list.child(self.render_general_ai_pending_message());
        }

        message_list
    }

    fn render_general_ai_live_message(
        &mut self,
        run_id: u64,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let lang = self.current_lang;
        let status_text = self
            .request_status_text
            .clone()
            .unwrap_or_else(|| t(lang, Translations::AI_IS_THINKING).to_string());
        let waiting = self.general_ai_live_text.trim().is_empty();
        let parts = if waiting {
            Vec::new()
        } else {
            parse_think_content(&self.general_ai_live_text)
        };
        let mut think_index = 0usize;
        let mut rendered_parts: Vec<gpui::AnyElement> = Vec::new();
        let task_id = self.active_task_id.unwrap_or_default();
        if !waiting {
            let mut prev_was_think = false;
            for part in &parts {
                match part {
                    ContentPart::Normal(text) => {
                        let add_top_padding = prev_was_think;
                        prev_was_think = false;
                        let el = div()
                            .text_base()
                            .text_color(PRIMARY_TEXT())
                            .whitespace_normal()
                            .child(text.clone());
                        let el = if add_top_padding { el.pt_1() } else { el };
                        rendered_parts.push(el.into_any_element());
                    }
                    ContentPart::ProcessTable { processes } => {
                        prev_was_think = false;
                        let el = self.render_process_table(processes, cx);
                        rendered_parts.push(el.into_any_element());
                    }
                    ContentPart::IntentUnderstanding { text, complete } => {
                        prev_was_think = true;
                        let key = format!("task:{}:intent", task_id);
                        let collapsed = self.think_collapsed.get(&key).copied().unwrap_or(*complete);
                        let header_text = if *complete {
                            t(lang, Translations::INTENT_UNDERSTOOD)
                        } else {
                            t(lang, Translations::UNDERSTANDING_INTENT)
                        };
                        let icon_path = if collapsed { "fold.svg" } else { "expand.svg" };

                        let el = div()
                            .flex_col()
                            .w_full()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .px_2()
                                    .py_1()
                                    .rounded_md()
                                    .bg(SURFACE_ELEVATED())
                                    .border_1()
                                    .border_color(BORDER_LIGHT())
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        gpui::MouseButton::Left,
                                        cx.listener(
                                            move |this, _: &gpui::MouseDownEvent, _window, cx| {
                                                let next = !this
                                                    .think_collapsed
                                                    .get(&key)
                                                    .copied()
                                                    .unwrap_or(false);
                                                this.think_collapsed.insert(key.clone(), next);
                                                cx.notify();
                                            },
                                        ),
                                    )
                                    .child(
                                        svg()
                                            .path(icon_path)
                                            .size(px(14.0))
                                            .flex_none()
                                            .text_color(MUTED_TEXT()),
                                    )
                                    .child(
                                        div().text_xs().text_color(MUTED_TEXT()).child(header_text),
                                    ),
                            )
                            .when(!collapsed, |this| {
                                this.child(
                                    div()
                                        .pl_3()
                                        .pr_2()
                                        .text_xs()
                                        .text_color(TERTIARY_TEXT())
                                        .whitespace_normal()
                                        .child(text.clone()),
                                )
                            });
                        rendered_parts.push(el.into_any_element());
                    }
                    ContentPart::Think { text, complete } => {
                        prev_was_think = true;
                        let current_think_index = think_index;
                        think_index += 1;
                        let complete = *complete;
                        let key = format!("general:{}:think:{}", run_id, current_think_index);
                        let collapsed = self.think_collapsed.get(&key).copied().unwrap_or(false);
                        let header_text = if complete {
                            t(lang, Translations::THINKING_DONE)
                        } else {
                            t(lang, Translations::THINKING_IN_PROGRESS)
                        };
                        let icon_path = if collapsed { "fold.svg" } else { "expand.svg" };

                        let el = div()
                            .flex_col()
                            .w_full()
                            .pl_8()
                            .pt_4()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .px_2()
                                    .py_1()
                                    .rounded_md()
                                    .bg(SURFACE_ELEVATED())
                                    .border_1()
                                    .border_color(BORDER_LIGHT())
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        gpui::MouseButton::Left,
                                        cx.listener(
                                            move |this, _: &gpui::MouseDownEvent, _window, cx| {
                                                let next = !this
                                                    .think_collapsed
                                                    .get(&key)
                                                    .copied()
                                                    .unwrap_or(false);
                                                this.think_collapsed.insert(key.clone(), next);
                                                cx.notify();
                                            },
                                        ),
                                    )
                                    .child(
                                        svg()
                                            .path(icon_path)
                                            .size(px(14.0))
                                            .flex_none()
                                            .text_color(MUTED_TEXT()),
                                    )
                                    .child(
                                        div().text_xs().text_color(MUTED_TEXT()).child(header_text),
                                    ),
                            )
                            .when(!collapsed, |this| {
                                this.child(
                                    div()
                                        .pl_3()
                                        .pr_2()
                                        .text_xs()
                                        .text_color(TERTIARY_TEXT())
                                        .whitespace_normal()
                                        .child(text.clone()),
                                )
                            });
                        rendered_parts.push(el.into_any_element());
                    }
                }
            }
        }

        let mut content = div()
            .flex_col()
            .items_start()
            .gap_4()
            .max_w(px(780.0))
            .min_w(px(35.0))
            .w_full();
        if waiting {
            content = content.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .bg(GHOST_SURFACE_BG())
                            .text_xs()
                            .text_color(BRAND_BLUE())
                            .child("LIVE"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(TERTIARY_TEXT())
                            .child(status_text),
                    ),
            );
        } else {
            content = content.children(rendered_parts);
        }

        div()
            .flex_col()
            .items_start()
            .gap_2()
            .w_full()
            .mb_4()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .size(px(22.0))
                            .rounded_full()
                            .bg(AVATAR_BG())
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(render_icon_element("assistant", gpui::white(), 11.0)),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(SECONDARY_TEXT())
                            .font_weight(FontWeight::BOLD)
                            .child("ONE AI"),
                    ),
            )
            .child(content)
    }

    fn render_claude_live_message(
        &mut self,
        run: &ClaudeRunPanelState,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let lang = self.current_lang;
        let preview = if run.live_text.trim().is_empty() {
            run.status_message.clone()
        } else {
            run.live_text.clone()
        };

        let parts = parse_think_content(&preview);
        let mut think_index = 0usize;

        div()
            .flex_col()
            .items_start()
            .gap_2()
            .w_full()
            .mb_4()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .size(px(22.0))
                            .rounded_full()
                            .bg(AVATAR_BG())
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(render_icon_element("assistant", gpui::white(), 11.0))
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(SECONDARY_TEXT())
                            .font_weight(FontWeight::BOLD)
                            .child(format!("{} · {}", t(lang, Translations::CLAUDE_CODE), run.status.label(lang)))
                    )
            )
            .child(
                div()
                    .flex_col()
                    .items_start()
                    .gap_4()
                    .max_w(px(780.0))
                    .min_w(px(35.0))
                    .w_full()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .px_2()
                                    .py_1()
                                    .rounded_md()
                                    .bg(GHOST_SURFACE_BG())
                                    .text_xs()
                                    .text_color(BRAND_BLUE())
                                    .child("LIVE")
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(TERTIARY_TEXT())
                                    .child(run.status_message.clone())
                            )
                    )
                    .child(
                        div()
                            .flex_col()
                            .w_full()
                            .children({
                                let mut rendered_parts: Vec<gpui::AnyElement> = Vec::new();
                                let mut prev_was_think = false;
                                let parts_static = parts.clone();
                                for part in &parts_static {
                                    match part {
                                        ContentPart::Normal(text) => {
                                            let add_top_padding = prev_was_think;
                                            prev_was_think = false;
                                            let el = div()
                                                .text_base()
                                                .text_color(PRIMARY_TEXT())
                                                .whitespace_normal()
                                                .child(text.clone());
                                            let el = if add_top_padding { el.pt_1() } else { el };
                                            rendered_parts.push(el.into_any_element());
                                        }
                                        ContentPart::ProcessTable { processes } => {
                                            prev_was_think = false;
                                            let el = self.render_process_table(processes, cx);
                                            rendered_parts.push(el.into_any_element());
                                        }
                                        ContentPart::IntentUnderstanding { .. } => {
                                            continue;
                                        }
                                        ContentPart::Think { text, complete } => {
                                            prev_was_think = true;
                                            let current_think_index = think_index;
                                            think_index += 1;
                                            let complete = *complete;
                                            let key = format!("live:{}:think:{}", run.run_id, current_think_index);
                                            let collapsed = self
                                                .think_collapsed
                                                .get(&key)
                                                .copied()
                                                .unwrap_or(complete);
                                            let header_text = if complete {
                                                t(lang, Translations::THINKING_DONE)
                                            } else {
                                                t(lang, Translations::THINKING_IN_PROGRESS)
                                            };
                                            let icon_path = if collapsed { "fold.svg" } else { "expand.svg" };
                                            let default_collapsed = complete;

                                            let el = div()
                                                .flex_col()
                                                .w_full()
                                                .pl_8()
                                                .pt_4()
                                                .child(
                                                    div()
                                                        .flex()
                                                        .items_center()
                                                        .gap_2()
                                                        .px_2()
                                                        .py_1()
                                                        .rounded_md()
                                                        .bg(GHOST_SURFACE_BG())
                                                        .cursor_pointer()
                                                        .on_mouse_down(gpui::MouseButton::Left, cx.listener(move |this, _: &gpui::MouseDownEvent, _window, cx| {
                                                            let next = !this.think_collapsed.get(&key).copied().unwrap_or(default_collapsed);
                                                            this.think_collapsed.insert(key.clone(), next);
                                                            cx.notify();
                                                        }))
                                                        .child(
                                                            svg()
                                                                .path(icon_path)
                                                                .size(px(14.0))
                                                                .flex_none()
                                                                .text_color(MUTED_TEXT())
                                                        )
                                                        .child(
                                                            div()
                                                                .text_xs()
                                                                .text_color(MUTED_TEXT())
                                                                .child(header_text)
                                                        )
                                                )
                                                .when(!collapsed, |this| {
                                                    this.child(
                                                        div()
                                                            .pl_3()
                                                            .pr_2()
                                                            .text_xs()
                                                            .text_color(TERTIARY_TEXT())
                                                            .whitespace_normal()
                                                            .child(text.clone())
                                                    )
                                                });
                                            rendered_parts.push(el.into_any_element());
                                        }
                                    }
                                }
                                rendered_parts
                            })
                    )
            )
    }

    fn render_general_ai_pending_message(&self) -> impl IntoElement {
        let lang = self.current_lang;
        let status_text = self
            .request_status_text
            .clone()
            .unwrap_or_else(|| t(lang, Translations::AI_IS_THINKING).to_string());

        div()
            .flex_col()
            .items_start()
            .gap_2()
            .w_full()
            .mb_4()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .size(px(22.0))
                            .rounded_full()
                            .bg(AVATAR_BG())
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(render_icon_element("assistant", gpui::white(), 11.0)),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(SECONDARY_TEXT())
                            .font_weight(FontWeight::BOLD)
                            .child("ONE AI"),
                    ),
            )
            .child(
                div()
                    .flex_col()
                    .items_start()
                    .gap_4()
                    .max_w(px(780.0))
                    .min_w(px(35.0))
                    .w_full()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .px_2()
                                    .py_1()
                                    .rounded_md()
                                    .bg(GHOST_SURFACE_BG())
                                    .text_xs()
                                    .text_color(BRAND_BLUE())
                                    .child("WAIT"),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(TERTIARY_TEXT())
                                    .child(status_text),
                            ),
                    ),
            )
    }

    fn render_process_table(
        &self,
        processes: &[ProcessDisplayInfo],
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let critical_count = processes.iter().filter(|p| p.is_critical).count();
        let lang = self.current_lang;

        div()
            .flex_col()
            .w_full()
            .max_w(px(806.0))
            .rounded_xl()
            .bg(ASSISTANT_BUBBLE_BG())
            .border_1()
            .border_color(BORDER_LIGHT())
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_6()
                    .py_4()
                    .bg(FLOATING_PANEL_BG())
                    .border_b_1()
                    .border_color(BORDER_LIGHT())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                svg()
                                    .path("activity.svg")
                                    .size(px(20.0))
                                    .text_color(BRAND_BLUE()),
                            )
                            .child(
                                div()
                                    .text_base()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(PRIMARY_TEXT())
                                    .child("System Process Monitor"),
                            ),
                    )
                    .when(critical_count > 0, |this| {
                        this.child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .px_3()
                                .py_1()
                                .rounded_sm()
                                .bg(Hsla {
                                    h: 0.0,
                                    s: 1.0,
                                    l: 0.88,
                                    a: 1.0,
                                })
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(Hsla {
                                            h: 0.0,
                                            s: 1.0,
                                            l: 0.35,
                                            a: 1.0,
                                        })
                                        .child(format!("{} High Usage Warnings", critical_count)),
                                ),
                        )
                    }),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .px_6()
                    .py_3()
                    .bg(FLOATING_PANEL_BG())
                    .border_b_1()
                    .border_color(BORDER_LIGHT())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .w(px(245.0))
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(SECONDARY_TEXT())
                            .child("Process Name"),
                    )
                    .child(
                        div()
                            .w(px(120.0))
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(SECONDARY_TEXT())
                            .child("PID"),
                    )
                    .child(
                        div()
                            .w(px(200.0))
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(SECONDARY_TEXT())
                            .child("CPU %"),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_right()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(SECONDARY_TEXT())
                            .child("Memory"),
                    ),
            )
            .child(
                div()
                    .id("process_table_body")
                    .flex_1()
                    .overflow_scroll()
                    .max_h(px(400.0))
                    .children(processes.iter().enumerate().map(|(i, proc)| {
                        let is_first = i == 0;
                        let row_border = if is_first {
                            div().border_t_1()
                        } else {
                            div().border_t_1().border_color(Hsla {
                                h: 0.61,
                                s: 0.14,
                                l: 0.24,
                                a: 0.3,
                            })
                        };
                        let cpu_bar_color = if proc.is_critical {
                            Hsla {
                                h: 0.0,
                                s: 1.0,
                                l: 0.42,
                                a: 1.0,
                            }
                        } else {
                            BRAND_BLUE()
                        };
                        let bg_bar_width = (proc.cpu_percent / 100.0 * 180.0).min(180.0);

                        div()
                            .flex()
                            .items_center()
                            .px_6()
                            .py_4()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_3()
                                    .w(px(245.0))
                                    .child(
                                        div()
                                            .size(px(32.0))
                                            .rounded_sm()
                                            .bg(Hsla {
                                                h: 0.61,
                                                s: 0.62,
                                                l: 0.88,
                                                a: 1.0,
                                            })
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(
                                                svg()
                                                    .path("cpu.svg")
                                                    .size(px(16.0))
                                                    .text_color(BRAND_BLUE()),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .text_base()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(PRIMARY_TEXT())
                                            .text_ellipsis()
                                            .child(proc.name.clone()),
                                    ),
                            )
                            .child(
                                div()
                                    .w(px(120.0))
                                    .text_base()
                                    .text_color(SECONDARY_TEXT())
                                    .child(proc.pid.to_string()),
                            )
                            .child(
                                div()
                                    .w(px(200.0))
                                    .flex()
                                    .items_center()
                                    .gap_3()
                                    .child(
                                        div()
                                            .h(px(6.0))
                                            .w(px(180.0))
                                            .rounded_full()
                                            .bg(Hsla {
                                                h: 0.61,
                                                s: 0.42,
                                                l: 0.88,
                                                a: 1.0,
                                            })
                                            .overflow_hidden()
                                            .child(
                                                div()
                                                    .h(px(6.0))
                                                    .w(px(bg_bar_width as f32))
                                                    .rounded_full()
                                                    .bg(cpu_bar_color),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(if proc.is_critical {
                                                Hsla {
                                                    h: 0.0,
                                                    s: 1.0,
                                                    l: 0.42,
                                                    a: 1.0,
                                                }
                                            } else {
                                                SECONDARY_TEXT()
                                            })
                                            .child(format!("{:.1}%", proc.cpu_percent)),
                                    ),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .text_right()
                                    .text_base()
                                    .text_color(SECONDARY_TEXT())
                                    .child(format_memory(proc.memory_mb)),
                            )
                    })),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_6()
                    .py_4()
                    .bg(FLOATING_PANEL_BG())
                    .border_t_1()
                    .border_color(BORDER_LIGHT())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_6()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .size(px(10.0))
                                            .rounded_full()
                                            .bg(Hsla {
                                                h: 0.0,
                                                s: 1.0,
                                                l: 0.42,
                                                a: 1.0,
                                            }),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(SECONDARY_TEXT())
                                            .child("CRITICAL (>60%)"),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .size(px(10.0))
                                            .rounded_full()
                                            .bg(BRAND_BLUE()),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(SECONDARY_TEXT())
                                            .child("HEALTHY"),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .px_4()
                            .py_2()
                            .rounded_md()
                            .border_1()
                            .border_color(BORDER_LIGHT())
                            .cursor_pointer()
                            .on_mouse_down(gpui::MouseButton::Left, cx.listener(move |this, _: &gpui::MouseDownEvent, _window, cx| {
                                // TODO: Generate full report action
                            }))
                            .child(
                                div()
                                    .text_base()
                                    .text_color(BRAND_BLUE())
                                    .child("Generate Full Report"),
                            )
                            .child(
                                svg()
                                    .path("external-link.svg")
                                    .size(px(14.0))
                                    .text_color(BRAND_BLUE()),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn render_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let lang = self.current_lang;
        let composer_key = match lang {
            Lang::Zh => "composer_editor_zh",
            Lang::En => "composer_editor_en",
        };
        let composer_editor = window.use_keyed_state(composer_key, &mut *cx, |window, cx| {
            let mut editor = Editor::single_line(window, cx);
            editor.set_placeholder_text(t(lang, Translations::TYPE_MESSAGE), window, cx);
            editor
        });

        let composer_focus = composer_editor.read(cx).focus_handle(cx);
        let weak_composer = composer_editor.downgrade();
        let weak_composer_for_action = weak_composer.clone();

        let request_in_flight = self.request_in_flight;
        let send_bg = if request_in_flight {
            Hsla {
                h: 0.0,
                s: 0.0,
                l: 0.78,
                a: 1.0,
            }
        } else {
            BRAND_BLUE()
        };
        let send_label = if request_in_flight {
            t(lang, Translations::SENDING)
        } else {
            t(lang, Translations::SEND)
        };

        div()
            .flex()
            .justify_center()
            .pt_5()
            .pb_10()
            .child(
                div()
                    .flex_col()
                    .w_full()
                    .max_w(px(940.0))
                    .gap_2()
                    .px_3()
                    .py_2()
                    .rounded_xl()
                    .bg(FLOATING_PANEL_BG())
                    .border_1()
                    .border_color(GHOST_SURFACE_BG())
                    .child(
                        div()
                            .flex()
                            .items_end()
                            .gap_2()
                            .px_3()
                            .py_2()
                            .rounded_lg()
                            .bg(INPUT_BG())
                            .border_1()
                            .border_color(BORDER_LIGHT())
                            .child(
                                div()
                                    .size(px(30.0))
                                    .rounded_full()
                                    .bg(GHOST_SURFACE_BG())
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(render_icon_element("add", MUTED_TEXT(), 14.0))
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .px_2()
                                    .pb_1()
                                    .track_focus(&composer_focus)
                                    .on_action(cx.listener(move |this, _: &Confirm, _window, cx| {
                                        if this.request_in_flight {
                                            return;
                                        }
                                        if let Some(editor) = weak_composer_for_action.upgrade() {
                                            let text = editor.read_with(cx, |editor, cx| editor.text(cx)).trim().to_string();
                                            if !text.is_empty() {
                                                let user_message = text.clone();
                                                let is_first_message = this.messages.is_empty();
                                                if is_first_message {
                                                    this.pending_summarize = true;
                                                }

                                                // Clear composer
                                                editor.update(cx, |editor, cx| {
                                                    editor.set_text("", _window, cx);
                                                });

                                                // Route message (fast route or LLM fallback)
                                                this.route_message(user_message, cx);
                                            }
                                        }
                                    }))
                                    .child(composer_editor)
                            )
                            .child(
                                div()
                                    .size(px(30.0))
                                    .rounded_full()
                                    .bg(GHOST_SURFACE_BG())
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(render_icon_element("mic", MUTED_TEXT(), 14.0))
                            )
                            .child(
                                div()
                                    .px_3()
                                    .py_2()
                                    .rounded_lg()
                                    .bg(send_bg)
                                    .cursor_pointer()
                                    .text_color(gpui::white())
                                    .text_sm()
                                    .font_weight(FontWeight::BOLD)
                                    .on_mouse_down(gpui::MouseButton::Left, cx.listener(move |this, _: &gpui::MouseDownEvent, _window, cx| {
                                        if this.request_in_flight {
                                            return;
                                        }
                                        if let Some(editor) = weak_composer.upgrade() {
                                            let text = editor.read_with(cx, |editor, cx| editor.text(cx)).trim().to_string();
                                            if !text.is_empty() {
                                                let user_message = text.clone();
                                                let is_first_message = this.messages.is_empty();
                                                if is_first_message {
                                                    this.pending_summarize = true;
                                                }

                                                // Clear composer
                                                editor.update(cx, |editor, cx| {
                                                    editor.set_text("", _window, cx);
                                                });

                                                // Route message (fast route or LLM fallback)
                                                this.route_message(user_message, cx);
                                            }
                                        }
                                    }))
                                    .child(send_label)
                            )
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .px_2()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(MUTED_TEXT())
                                    .child(format!("{} · {}", self.model_name, t(lang, Translations::EXPLORER)))
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(if request_in_flight { BRAND_BLUE() } else { SECONDARY_TEXT() })
                                    .child(if request_in_flight {
                                        self.request_status_text.clone().unwrap_or_else(|| t(lang, Translations::SENDING).to_string())
                                    } else {
                                        format!("{} Active", self.model_name)
                                    })
                            )
                    )
            )
    }

    fn render_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let sidebar_bg = WORKSPACE_BG();
        let lang = self.current_lang;
        let run = self
            .current_claude_run
            .as_ref()
            .filter(|run| run.task_id == self.active_task_id)
            .cloned();

        let mut sidebar = div()
            .flex()
            .flex_col()
            .w(px(340.0))
            .h_full()
            .bg(sidebar_bg)
            .child(div().h(px(1.0)).bg(BORDER_LIGHT()))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .h(px(56.0))
                    .px_4()
                    .bg(SURFACE_ELEVATED())
                    .border_b_1()
                    .border_color(BORDER_LIGHT())
                    .child(
                        div()
                            .text_sm()
                            .text_color(PRIMARY_TEXT())
                            .font_weight(FontWeight::BOLD)
                            .child(t(lang, Translations::CLAUDE_CODE_RUN)),
                    ),
            );

        if let Some(run) = run {
            let status_color = run.status.color();
            let task_dir = run.work_dir.clone();
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
                .map(|preview| preview.status.label(lang).to_string())
                .unwrap_or_else(|| t(lang, Translations::PREVIEW_IDLE).to_string());
            let preview_color = preview
                .as_ref()
                .map(|preview| preview.status.color())
                .unwrap_or(MUTED_TEXT());
            let pending_question = run.pending_question.clone();
            let question_editor =
                window.use_keyed_state("claude-question-editor", &mut *cx, |window, cx| {
                    let mut editor = Editor::single_line(window, cx);
                    editor.set_placeholder_text(
                        t(lang, Translations::ANSWER_CLAUDE_QUESTION),
                        window,
                        cx,
                    );
                    editor
                });
            let question_focus = question_editor.read(cx).focus_handle(cx);
            let weak_question_editor = question_editor.downgrade();

            let mut timeline = div().flex().flex_col().gap_2();
            for event in run.events.iter().rev() {
                let detail_block = render_formatted_content(
                    &event.formatted_detail,
                    SECONDARY_TEXT(),
                    PRIMARY_TEXT(),
                );
                timeline = timeline.child(
                    div()
                        .flex_col()
                        .gap_1()
                        .p_3()
                        .rounded_lg()
                        .bg(SURFACE_ELEVATED())
                        .border_1()
                        .border_color(BORDER_LIGHT())
                        .child(
                            div()
                                .text_xs()
                                .text_color(event.tone.color())
                                .font_weight(FontWeight::BOLD)
                                .child(event.title.clone()),
                        )
                        .child(detail_block),
                );
            }

            let stderr_preview = if run.stderr_lines.is_empty() {
                t(lang, Translations::NO_STDERR_OUTPUT).to_string()
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
                            .gap_3()
                            .p_4()
                            .rounded_xl()
                            .bg(SURFACE_PANEL())
                            .border_1()
                            .border_color(BORDER_LIGHT())
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .px_2()
                                            .py_1()
                                            .rounded_md()
                                            .bg(CANVAS_BG())
                                            .border_1()
                                            .border_color(BORDER_LIGHT())
                                            .text_xs()
                                            .text_color(SECONDARY_TEXT())
                                            .child("RUN")
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(gpui::white())
                                            .px_2()
                                            .py_1()
                                            .rounded_md()
                                            .bg(status_color)
                                            .child(run.status.label(lang))
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(MUTED_TEXT())
                                            .child(run.status_message.clone())
                                    )
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(PRIMARY_TEXT())
                                    .font_weight(FontWeight::BOLD)
                                    .whitespace_normal()
                                    .child(run.instruction.clone())
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(MUTED_TEXT())
                                    .whitespace_normal()
                                    .child(format!("{}: {}", t(lang, Translations::WORKDIR), run.work_dir))
                            )
                    )
                    .child(
                        div()
                            .flex_col()
                            .gap_2()
                            .p_3()
                            .rounded_xl()
                            .bg(SURFACE_PANEL())
                            .border_1()
                            .border_color(BORDER_LIGHT())
                            .child(div().text_xs().text_color(MUTED_TEXT()).child(t(lang, Translations::PROGRESS)))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(PRIMARY_TEXT())
                                    .whitespace_normal()
                                    .child(run.status_message.clone())
                            )
                    )
                    .child(
                        div()
                            .flex_col()
                            .gap_2()
                            .p_3()
                            .rounded_xl()
                            .bg(SURFACE_PANEL())
                            .border_1()
                            .border_color(BORDER_LIGHT())
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(div().text_xs().text_color(MUTED_TEXT()).child(t(lang, Translations::PREVIEW)))
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
                                    .text_color(SECONDARY_TEXT())
                                    .whitespace_normal()
                                    .child(
                                        preview
                                            .as_ref()
                                            .map(|preview| preview.note.clone())
                                            .unwrap_or_else(|| t(lang, Translations::NO_PREVIEW_INFO).to_string())
                                    )
                            )
                            .when_some(preview.clone().and_then(|preview| preview.entry_file), |this, entry_file| {
                                this.child(
                                    div()
                                        .text_xs()
                                        .text_color(MUTED_TEXT())
                                        .whitespace_normal()
                                        .child(format!("{}: {}", t(lang, Translations::ENTRY), entry_file))
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
                                                .text_color(BRAND_BLUE())
                                                .whitespace_normal()
                                                .child(url.clone())
                                        )
                                        .child(
                                            div()
                                                .px_3()
                                                .py_2()
                                                .rounded_md()
                                                .bg(BRAND_BLUE())
                                                .text_xs()
                                                .text_color(gpui::white())
                                                .cursor_pointer()
                                                .on_mouse_down(gpui::MouseButton::Left, cx.listener({
                                                    let url = url.clone();
                                                    move |this, _: &gpui::MouseDownEvent, _window, _cx| {
                                                        this.open_url_in_browser(&url);
                                                    }
                                                }))
                                                .child(t(lang, Translations::OPEN_IN_BROWSER))
                                        )
                                )
                            })
                    )
                    .child(
                        div()
                            .flex_col()
                            .gap_2()
                            .p_3()
                            .rounded_xl()
                            .bg(SURFACE_PANEL())
                            .border_1()
                            .border_color(BORDER_LIGHT())
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(div().text_xs().text_color(MUTED_TEXT()).child(t(lang, Translations::ARTIFACTS)))
                                    .child(
                                        div()
                                            .px_3()
                                            .py_2()
                                            .rounded_md()
                                            .bg(CANVAS_BG())
                                            .text_xs()
                                            .text_color(PRIMARY_TEXT())
                                            .cursor_pointer()
                                            .on_mouse_down(gpui::MouseButton::Left, {
                                                let task_dir = task_dir.clone();
                                                cx.listener(move |this, _: &gpui::MouseDownEvent, _window, _cx| {
                                                    this.open_folder_in_finder(&task_dir);
                                                })
                                            })
                                            .child(t(lang, Translations::OPEN_TASK_FOLDER))
                                    )
                            )
                            .child(
                                div()
                                    .flex_col()
                                    .gap_2()
                                    .children(run.artifacts.iter().take(12).cloned().map(|artifact| {
                                        let absolute_path = artifact.absolute_path.clone();
                                        let label = format!("{} · {}", artifact.relative_path, artifact.kind);
                                        div()
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .gap_2()
                                            .px_2()
                                            .py_2()
                                            .rounded_md()
                                            .bg(CANVAS_BG())
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(PRIMARY_TEXT())
                                                    .whitespace_normal()
                                                    .child(label)
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(BRAND_BLUE())
                                                    .cursor_pointer()
                                                    .on_mouse_down(gpui::MouseButton::Left, cx.listener(move |this, _: &gpui::MouseDownEvent, _window, _cx| {
                                                        this.reveal_file_in_finder(&absolute_path);
                                                    }))
                                                    .child(t(lang, Translations::REVEAL))
                                            )
                                            .into_any_element()
                                    }))
                            )
                            .when(run.artifacts.is_empty(), |this| {
                                this.child(
                                    div()
                                        .text_xs()
                                        .text_color(SECONDARY_TEXT())
                                        .child(t(lang, Translations::NO_ARTIFACTS_YET))
                                )
                            })
                    )
                    .child(
                        div()
                            .flex_col()
                            .gap_2()
                            .p_3()
                            .rounded_xl()
                            .bg(SURFACE_PANEL())
                            .border_1()
                            .border_color(BORDER_LIGHT())
                            .child(div().text_xs().text_color(MUTED_TEXT()).child(t(lang, Translations::QUESTIONS)))
                            .when_some(pending_question.clone(), |this, question| {
                                this.child(
                                    div()
                                        .flex_col()
                                        .gap_2()
                                        .child(
                                            div()
                                                .text_sm()
                                                .text_color(PRIMARY_TEXT())
                                                .whitespace_normal()
                                                .child(question.prompt.clone())
                                        )
                                        .when(!question.options.is_empty(), |this| {
                                            this.child(
                                                div()
                                                    .flex()
                                                    .flex_col()
                                                    .gap_2()
                                                    .children(question.options.iter().cloned().map(|option| {
                                                        let option_label = option.clone();
                                                        div()
                                                            .px_3()
                                                            .py_2()
                                                            .rounded_md()
                                                            .bg(WORKSPACE_BG())
                                                            .cursor_pointer()
                                                            .text_xs()
                                                            .text_color(PRIMARY_TEXT())
                                                            .on_mouse_down(gpui::MouseButton::Left, cx.listener(move |this, _: &gpui::MouseDownEvent, _window, cx| {
                                                                this.continue_claude_with_answer(option.clone(), cx);
                                                            }))
                                                            .child(option_label)
                                                            .into_any_element()
                                                    }))
                                            )
                                        })
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap_2()
                                                .child(
                                                    div()
                                                        .flex_1()
                                                        .px_2()
                                                        .py_2()
                                                        .rounded_md()
                                                        .bg(CANVAS_BG())
                                                        .track_focus(&question_focus)
                                                        .on_action(cx.listener({
                                                            let weak_question_editor = weak_question_editor.clone();
                                                            move |this, _: &Confirm, _window, cx| {
                                                                if let Some(editor) = weak_question_editor.upgrade() {
                                                                    let answer = editor.read_with(cx, |editor, cx| editor.text(cx)).trim().to_string();
                                                                    if !answer.is_empty() {
                                                                        editor.update(cx, |editor, cx| editor.set_text("", _window, cx));
                                                                        this.continue_claude_with_answer(answer, cx);
                                                                    }
                                                                }
                                                            }
                                                        }))
                                                        .child(question_editor.clone())
                                                )
                                                .child(
                                                    div()
                                                        .px_3()
                                                        .py_2()
                                                        .rounded_md()
                                                        .bg(BRAND_BLUE())
                                                        .text_xs()
                                                        .text_color(gpui::white())
                                                        .cursor_pointer()
                                                        .on_mouse_down(gpui::MouseButton::Left, cx.listener({
                                                            let weak_question_editor = weak_question_editor.clone();
                                                            move |this, _: &gpui::MouseDownEvent, _window, cx| {
                                                                if let Some(editor) = weak_question_editor.upgrade() {
                                                                    let answer = editor.read_with(cx, |editor, cx| editor.text(cx)).trim().to_string();
                                                                    if !answer.is_empty() {
                                                                        editor.update(cx, |editor, cx| editor.set_text("", _window, cx));
                                                                        this.continue_claude_with_answer(answer, cx);
                                                                    }
                                                                }
                                                            }
                                                        }))
                                                        .child(t(lang, Translations::SUBMIT))
                                                )
                                        )
                                )
                            })
                            .when(pending_question.is_none(), |this| {
                                this.child(
                                    div()
                                        .text_xs()
                                        .text_color(SECONDARY_TEXT())
                                        .child(t(lang, Translations::NO_PENDING_QUESTIONS))
                                )
                            })
                    )
                    .child(
                        div()
                            .flex_col()
                            .gap_2()
                            .p_3()
                            .rounded_xl()
                            .bg(SURFACE_PANEL())
                            .border_1()
                            .border_color(BORDER_LIGHT())
                            .child(div().text_xs().text_color(MUTED_TEXT()).child(t(lang, Translations::LIVE_OUTPUT)))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(PRIMARY_TEXT())
                                    .whitespace_normal()
                                    .child(live_output)
                            )
                    )
                    .child(
                        div()
                            .flex_col()
                            .gap_2()
                            .p_3()
                            .rounded_xl()
                            .bg(SURFACE_PANEL())
                            .border_1()
                            .border_color(BORDER_LIGHT())
                            .child(div().text_xs().text_color(MUTED_TEXT()).child(t(lang, Translations::COMMAND)))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(SECONDARY_TEXT())
                                    .whitespace_normal()
                                    .child(if run.command_preview.is_empty() {
                                        t(lang, Translations::COMMAND_NOT_STARTED).to_string()
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
                            .rounded_xl()
                            .bg(SURFACE_PANEL())
                            .border_1()
                            .border_color(BORDER_LIGHT())
                            .child(div().text_xs().text_color(MUTED_TEXT()).child(t(lang, Translations::STDERR)))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(if run.stderr_lines.is_empty() { SECONDARY_TEXT() } else { Hsla { h: 0.0, s: 0.72, l: 0.52, a: 1.0 } })
                                    .whitespace_normal()
                                    .child(stderr_preview)
                            )
                    )
                    .child(
                        div()
                            .flex_col()
                            .gap_2()
                            .p_3()
                            .rounded_xl()
                            .bg(SURFACE_PANEL())
                            .border_1()
                            .border_color(BORDER_LIGHT())
                            .child(div().text_xs().text_color(MUTED_TEXT()).child(t(lang, Translations::TIMELINE)))
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
                            .w_full()
                            .max_w(px(320.0))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(PRIMARY_TEXT())
                                    .font_weight(FontWeight::BOLD)
                                    .child(t(lang, Translations::NO_CLAUDE_RUN_YET)),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(MUTED_TEXT())
                                    .text_center()
                                    .whitespace_normal()
                                    .child(t(lang, Translations::CLAUDE_PANEL_HINT)),
                            ),
                    ),
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
            .bg(BORDER_LIGHT())
            .on_drag(DraggedResizer, |_, _, _, cx| cx.new(|_| DraggedResizer))
            .on_drag_move(
                cx.listener(|this, e: &DragMoveEvent<DraggedResizer>, _window, _cx| {
                    if let (Some(initial_x), Some(initial_width)) = (
                        this.terminal_resize_initial_mouse_x,
                        this.terminal_resize_initial_width,
                    ) {
                        let current_x: f32 = e.event.position.x.into();
                        let delta = initial_x - current_x;
                        let new_width = initial_width + delta;
                        eprintln!(
                            "drag_move: initial_x={}, current_x={}, delta={}, new_width={}",
                            initial_x, current_x, delta, new_width
                        );
                        if new_width >= 200.0 && new_width <= 800.0 {
                            this.terminal_width = new_width;
                        }
                    }
                }),
            )
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, event: &gpui::MouseDownEvent, _window, _cx| {
                    let initial_mouse_x: f32 = event.position.x.into();
                    this.terminal_resize_initial_mouse_x = Some(initial_mouse_x);
                    this.terminal_resize_initial_width = Some(this.terminal_width);
                    eprintln!(
                        "on_mouse_down: initial_mouse_x={}, initial_width={}",
                        initial_mouse_x, this.terminal_width
                    );
                }),
            )
    }

    fn render_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let terminal_bg = WORKSPACE_BG();
        let terminal_text = PRIMARY_TEXT();
        let prompt_color = Hsla {
            h: 0.35,
            s: 0.8,
            l: 0.45,
            a: 1.0,
        };
        let error_color = Hsla {
            h: 0.0,
            s: 0.8,
            l: 0.45,
            a: 1.0,
        };
        let width = self.terminal_width;
        let lang = self.current_lang;

        // Get working directory based on active task
        let work_dir = self.get_work_dir();

        // Create terminal input editor
        let terminal_editor = window.use_keyed_state("terminal_editor", &mut *cx, |window, cx| {
            let mut editor = Editor::single_line(window, cx);
            editor.set_placeholder_text(t(lang, Translations::TYPE_COMMAND), window, cx);
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
                    .h(px(48.0))
                    .px_4()
                    .bg(SURFACE_ELEVATED())
                    .border_b_1()
                    .border_color(BORDER_LIGHT())
                    .child(
                        div()
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .bg(CANVAS_BG())
                            .border_1()
                            .border_color(BORDER_LIGHT())
                            .text_xs()
                            .text_color(SECONDARY_TEXT())
                            .child("TTY"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(MUTED_TEXT())
                            .child(t(lang, Translations::TERMINAL)),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(TERTIARY_TEXT())
                            .ml_auto()
                            .child(format!(
                                "{} | {}",
                                work_dir,
                                match &self.sandbox_backend {
                                    Backend::Docker(_) => "docker",
                                    Backend::Pty(_) => "pty",
                                }
                            )),
                    ),
            )
            .child(
                div()
                    .id("terminal-content")
                    .flex_1()
                    .overflow_scroll()
                    .p_4()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .children(self.terminal_output.iter().map(|line| {
                        let prompt_color = prompt_color;
                        let terminal_text = terminal_text;
                        let error_color = error_color;
                        let output = line.output.clone();
                        let is_error = output.contains("Error") || output.contains("error:");
                        div()
                            .flex_col()
                            .gap_2()
                            .p_3()
                            .rounded_lg()
                            .bg(SURFACE_PANEL())
                            .border_1()
                            .border_color(BORDER_LIGHT())
                            .children(line.command.iter().map(|cmd| {
                                div()
                                    .flex()
                                    .gap_2()
                                    .child(div().text_sm().text_color(prompt_color).child("➜"))
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(terminal_text)
                                            .child(cmd.clone()),
                                    )
                            }))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(if is_error { error_color } else { terminal_text })
                                    .child(output),
                            )
                            .into_any_element()
                    })),
            )
            .child(
                div()
                    .id("terminal-input-line")
                    .h(px(52.0))
                    .px_4()
                    .bg(SURFACE_ELEVATED())
                    .border_t_1()
                    .border_color(BORDER_LIGHT())
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().text_sm().text_color(prompt_color).child("➜"))
                    .child(
                        div()
                            .flex_1()
                            .px_2()
                            .rounded_lg()
                            .bg(CANVAS_BG())
                            .track_focus(&terminal_focus)
                            .on_action(cx.listener(move |this, _: &Confirm, _window, cx| {
                                if let Some(editor) = weak_terminal.upgrade() {
                                    let text = editor
                                        .read_with(cx, |editor, cx| editor.text(cx))
                                        .trim()
                                        .to_string();
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
                                            let exec_result =
                                                gpui_tokio::Tokio::spawn(cx, async move {
                                                    match &sandbox_backend {
                                                        Backend::Docker(b) => {
                                                            b.exec_command(task_id, vec![&text])
                                                                .await
                                                        }
                                                        Backend::Pty(b) => {
                                                            b.exec_command(task_id, vec![&text])
                                                                .await
                                                        }
                                                    }
                                                })
                                                .await;

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
                                        })
                                        .detach();
                                    }
                                }
                            }))
                            .child(terminal_editor),
                    ),
            )
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
        image::open(icon_path)
            .ok()
            .map(|img| img.to_rgba8())
            .map(|rgba| {
                let (width, height) = rgba.dimensions();
                Arc::new(image::RgbaImage::from_raw(width, height, rgba.into_raw()).unwrap())
            })
    } else {
        eprintln!("[App] Icon not found at assets/logo.png");
        None
    };

    application()
        .with_assets(assets::Assets)
        .run(move |cx: &mut App| {
            settings::init(cx);
            theme_settings::init(theme::LoadThemes::JustBase, cx);
            editor::init(cx);
            gpui_tokio::init(cx);

            // Load default keymap so Editor actions like Delete, Backspace work
            cx.bind_keys(
                KeymapFile::load_asset_allow_partial_failure(DEFAULT_KEYMAP_PATH, cx)
                    .expect("failed to load default keymap"),
            );

            let bounds = Bounds::centered(
                None,
                size(px(DEFAULT_WINDOW_WIDTH), px(DEFAULT_WINDOW_HEIGHT)),
                cx,
            );
            cx.open_window(
                WindowOptions {
                    titlebar: Some(TitlebarOptions {
                        title: None,
                        appears_transparent: true,
                        traffic_light_position: if cfg!(target_os = "macos") {
                            Some(point(px(12.0), px(12.0)))
                        } else {
                            None
                        },
                    }),
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    is_resizable: true,
                    window_min_size: Some(size(px(800.0), px(600.0))),
                    icon: icon_image.clone(),
                    ..Default::default()
                },
                move |window, cx| cx.new(|cx| AppState::new(window, cx, config.clone())),
            )
            .unwrap();
            cx.activate(true);
        });
}
