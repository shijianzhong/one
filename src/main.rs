use gpui::{
    App, AppContext as _, Bounds, Context, DragMoveEvent,
    Hsla, IntoElement, ParentElement, px, size, Render,
    Styled, StatefulInteractiveElement, Window, WindowOptions, WindowBounds, div, prelude::*,
    Focusable,
};
use gpui_platform::application;
use editor::Editor;
use menu::Confirm;
use settings::{KeymapFile, DEFAULT_KEYMAP_PATH};
use theme;
use theme_settings;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use gpui::FontWeight;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Config {
    model_base_url: String,
    model_api_key: String,
    model_name: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            model_base_url: "https://api.openai.com/v1".to_string(),
            model_api_key: "".to_string(),
            model_name: "gpt-4".to_string(),
        }
    }
}

fn get_config_path() -> PathBuf {
    let config_dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".solo3_gpui");
    std::fs::create_dir_all(&config_dir).ok();
    config_dir.join("config.json")
}

fn load_config() -> Config {
    let path = get_config_path();
    if path.exists() {
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(config) = serde_json::from_str(&content) {
                return config;
            }
        }
    }
    Config::default()
}

fn save_config(config: &Config) -> anyhow::Result<()> {
    let path = get_config_path();
    let content = serde_json::to_string_pretty(config)?;
    std::fs::write(&path, content)?;
    Ok(())
}

fn call_chat_api_sync(base_url: &str, api_key: &str, model: &str, messages: &[ChatMessage]) -> Result<String, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;

    eprintln!("[DEBUG] API call starting...");
    eprintln!("[DEBUG] base_url: {}, model: {}", base_url, model);
    eprintln!("[DEBUG] messages count: {}", messages.len());

    #[derive(serde::Serialize)]
    struct RequestBody {
        model: String,
        messages: Vec<serde_json::Value>,
    }

    let chat_messages: Vec<serde_json::Value> = messages.iter().map(|m| {
        serde_json::json!({
            "role": m.role,
            "content": m.content
        })
    }).collect();

    let request_body = RequestBody {
        model: model.to_string(),
        messages: chat_messages,
    };

    let url = format!("{}/chat/completions", base_url);
    eprintln!("[DEBUG] URL: {}", url);

    let body_str = serde_json::to_string(&request_body).unwrap();
    eprintln!("[DEBUG] Request body: {}", body_str);

    eprintln!("[DEBUG] Sending request...");
    let response = client.post(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .body(body_str)
        .send()
        .map_err(|e| e.to_string())?;
    eprintln!("[DEBUG] Response received, status: {}", response.status());

    if !response.status().is_success() {
        return Err(format!("API error: {}", response.status()));
    }

    let body_str = response.text().map_err(|e| e.to_string())?;
    eprintln!("[DEBUG] Response body: {}", body_str);

    #[derive(serde::Deserialize)]
    struct ApiResponse {
        choices: Vec<Choice>,
    }

    #[derive(serde::Deserialize)]
    struct Choice {
        message: Message,
    }

    #[derive(serde::Deserialize)]
    struct Message {
        content: String,
    }

    let api_response: ApiResponse = serde_json::from_str(&body_str).map_err(|e| e.to_string())?;

    let result = api_response.choices.first()
        .map(|c| c.message.content.clone())
        .unwrap_or_default();
    eprintln!("[DEBUG] Result: {}", result);
    Ok(result)
}

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
    workspaces: Vec<Workspace>,
    active_workspace_id: Option<usize>,
    active_task_id: Option<usize>,
    sidebar_visible: bool,
    terminal_visible: bool,
    terminal_width: f32,
    terminal_resize_initial_mouse_x: Option<f32>,
    terminal_resize_initial_width: Option<f32>,
    show_model_config_dialog: bool,
    model_base_url: String,
    model_api_key: String,
    model_name: String,
    editing_model_name: String,
    editing_base_url: String,
    editing_api_key: String,
    messages: Vec<ChatMessage>,
}

#[derive(Debug, Clone)]
struct ChatMessage {
    role: String,
    content: String,
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
    status: &'static str,
}

impl AppState {
    fn new(_window: &mut Window, cx: &mut Context<Self>, config: Config) -> Self {
        Self {
            workspaces: vec![],
            active_workspace_id: None,
            active_task_id: None,
            sidebar_visible: false,
            terminal_visible: false,
            terminal_width: 500.0,
            terminal_resize_initial_mouse_x: None,
            terminal_resize_initial_width: None,
            show_model_config_dialog: false,
            model_base_url: config.model_base_url,
            model_api_key: config.model_api_key,
            model_name: config.model_name,
            editing_model_name: "gpt-4".to_string(),
            editing_base_url: "https://api.openai.com/v1".to_string(),
            editing_api_key: "".to_string(),
            messages: vec![],
        }
    }

    fn get_active_workspace(&self) -> Option<&Workspace> {
        self.active_workspace_id.and_then(|id| self.workspaces.iter().find(|w| w.id == id))
    }

    fn get_active_workspace_mut(&mut self) -> Option<&mut Workspace> {
        self.active_workspace_id.and_then(|id| self.workspaces.iter_mut().find(|w| w.id == id))
    }

    fn get_active_task(&self) -> Option<&TaskItem> {
        self.get_active_workspace()
            .and_then(|w| w.tasks.iter().find(|t| Some(t.id) == self.active_task_id))
    }

    fn add_workspace(&mut self, path: PathBuf, name: String) {
        let id = self.workspaces.len() + 1;
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
            let id = workspace.tasks.len() + 1;
            workspace.tasks.push(TaskItem {
                id,
                title,
                status: "todo",
            });
            self.active_task_id = Some(id);
            cx.notify();
        }
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

}

impl Render for AppState {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .size_full()
            .bg(CARD_BG)
            .child(self.render_nav(cx))
            .child(div().w(px(1.0)).bg(BORDER_LIGHT))
            .child(self.render_chat(_window, cx))
            .when(self.sidebar_visible, |this| {
                this.child(div().w(px(1.0)).bg(BORDER_LIGHT))
                    .child(self.render_sidebar())
            })
            .when(self.terminal_visible, |this| {
                this.child(self.render_terminal_resizer(cx))
                    .child(self.render_terminal())
            })
            .when(self.show_model_config_dialog, |this| {
                this.child(self.render_model_config_dialog(_window, cx))
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
        div()
            .flex()
            .items_center()
            .h(px(40.0))
            .px_4()
            .child(div().text_base().text_color(PRIMARY_TEXT).font_weight(FontWeight::BOLD).child("ONE"))
    }

    fn render_nav_buttons(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut nav = div()
            .flex()
            .flex_col()
            .gap_1()
            .p_2();

        nav = nav.child(self.make_nav_item("New Task", "⌘N", cx));
        nav = nav.child(self.make_nav_item("Skills", "⌘S", cx));
        nav = nav.child(self.make_nav_item("Automation", "⌘A", cx));
        nav = nav.child(self.make_nav_item("Model Config", "⌘M", cx));

        nav
    }

    fn make_nav_item(&mut self, label: &'static str, shortcut: &'static str, cx: &mut Context<Self>) -> impl IntoElement {
        let is_new_task = label == "New Task";
        let is_model_config = label == "Model Config";

        div()
            .flex()
            .items_center()
            .gap_3()
            .px_3()
            .py_2()
            .rounded_md()
            .cursor_pointer()
            .when(is_new_task, |this| {
                this.on_mouse_down(gpui::MouseButton::Left, cx.listener(|this, _: &gpui::MouseDownEvent, _window, cx| {
                    this.handle_new_task_click(cx);
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

    fn handle_new_task_click(&mut self, cx: &mut Context<Self>) {
        if let Some((path, name)) = Self::pick_folder_dialog() {
            self.add_workspace(path, name);
            if let Some(ws_id) = self.active_workspace_id {
                self.add_task_to_workspace(ws_id, "New Task".to_string(), cx);
            }
        }
    }

    fn render_task_list(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let workspaces = self.workspaces.clone();
        let active_workspace_id = self.active_workspace_id;
        let active_task_id = self.active_task_id;

        let mut result = div()
            .flex()
            .flex_col()
            .flex_1()
            .p_3();

        result = result.child(div().text_xs().text_color(MUTED_TEXT).mb_3().child("WORKSPACES"));

        for workspace in workspaces {
            let is_active_ws = active_workspace_id == Some(workspace.id);
            let ws_bg = if is_active_ws { CARD_BG } else { NAV_BG };
            let ws_id = workspace.id;

            // Workspace row - clicking selects workspace (but doesn't toggle expand)
            let ws_row = div()
                .flex()
                .items_center()
                .gap_2()
                .px_3()
                .py_2()
                .rounded_md()
                .bg(ws_bg)
                .cursor_pointer()
                .on_mouse_down(gpui::MouseButton::Left, cx.listener(move |this, _: &gpui::MouseDownEvent, _window, _cx| {
                    this.active_workspace_id = Some(ws_id);
                }));

            // Workspace expand/collapse icon - separate clickable area
            let expand_icon = if workspace.expanded { "▾" } else { "▸" };
            let expand_btn = div()
                .text_base()
                .text_color(MUTED_TEXT)
                .px_1()
                .py_1()
                .cursor_pointer()
                .id("expand-btn")
                .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _window, _cx| {
                    this.active_workspace_id = Some(ws_id);
                    if let Some(ws) = this.workspaces.iter_mut().find(|w| w.id == ws_id) {
                        ws.expanded = !ws.expanded;
                    }
                }));
            // Add button (+)
            let add_btn = div()
                .text_base()
                .text_color(MUTED_TEXT)
                .px_1()
                .py_1()
                .cursor_pointer()
                .id("add-btn")
                .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _window, cx| {
                    this.active_workspace_id = Some(ws_id);
                    this.add_task_to_workspace(ws_id, "New Task".to_string(), cx);
                }));

            let ws_label = format!("📁 {}", workspace.name);
            result = result.child(
                ws_row.child(
                    expand_btn.child(expand_icon)
                ).child(
                    div().text_sm().text_color(if is_active_ws { BRAND_BLUE } else { PRIMARY_TEXT }).child(ws_label)
                ).child(
                    div().ml_auto().child(add_btn.child("+"))
                )
            );

            // Tasks under workspace (if expanded) - each workspace's tasks are in their own scrollable container
            if workspace.expanded {
                let mut tasks_container = div()
                    .flex_col()
                    .ml_4()
                    .max_h(px(200.0))
                    .id("tasks-container")
                    .overflow_y_scroll();

                for task in &workspace.tasks {
                    let is_active_task = active_task_id == Some(task.id) && active_workspace_id == Some(workspace.id);
                    let task_bg = if is_active_task { ACTIVE_BG } else { CARD_BG };

                    let mut task_div = div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_3()
                        .py_2()
                        .rounded_md()
                        .bg(task_bg)
                        .cursor_pointer();

                    let task_id = task.id;
                    let ws_id = workspace.id;

                    task_div = task_div.on_mouse_down(gpui::MouseButton::Left, cx.listener(move |this, _: &gpui::MouseDownEvent, _window, _cx| {
                        this.active_workspace_id = Some(ws_id);
                        this.active_task_id = Some(task_id);
                        _cx.notify();
                    }));

                    tasks_container = tasks_container.child(
                        task_div.child(
                            div().w(px(6.0)).h(px(6.0)).rounded_full()
                                .bg(if is_active_task { BRAND_BLUE } else { MUTED_TEXT })
                        ).child(
                            div().text_sm().text_color(if is_active_task { BRAND_BLUE } else { PRIMARY_TEXT }).child(task.title.clone())
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
        let sidebar_visible = self.sidebar_visible;
        let terminal_visible = self.terminal_visible;

        div()
            .flex()
            .flex_col()
            .flex_1()
            .h_full()
            .min_w(px(350.0))
            .child(self.render_chat_header(title, sidebar_visible, terminal_visible, cx))
            .child(div().flex_1().overflow_hidden().p_4().child(self.render_chat_messages()))
            .child(div().h(px(1.0)).bg(BORDER_LIGHT))
            .child(self.render_composer(window, cx))
    }

    fn render_chat_header(&mut self, title: String, sidebar_visible: bool, terminal_visible: bool, cx: &mut Context<Self>) -> impl IntoElement {
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
                    .gap_4()
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

    fn render_chat_messages(&self) -> impl IntoElement {
        let messages = self.messages.clone();
        let mut result = div()
            .flex()
            .flex_col()
            .gap_4()
            .w_full();

        if messages.is_empty() {
            result = result.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .p_4()
                    .rounded_lg()
                    .bg(CARD_BG)
                    .border_1()
                    .border_color(BORDER_LIGHT)
                    .child(div().text_xs().text_color(MUTED_TEXT).child("Assistant"))
                    .child(div().text_base().text_color(PRIMARY_TEXT).child("Hello! I'm your SOLO 3.0 assistant. Select or create a task to get started."))
            );
        } else {
            for msg in messages {
                let role_label = if msg.role == "user" { "You" } else { "Assistant" };
                let role_color = if msg.role == "user" { BRAND_BLUE } else { MUTED_TEXT };
                result = result.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .p_4()
                        .rounded_lg()
                        .bg(CARD_BG)
                        .border_1()
                        .border_color(BORDER_LIGHT)
                        .child(div().text_xs().text_color(role_color).child(role_label))
                        .child(div().text_base().text_color(PRIMARY_TEXT).child(msg.content))
                );
            }
        }

        div()
            .id("chat_messages")
            .overflow_scroll()
            .flex_1()
            .w_full()
            .child(result)
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
                        if let Some(editor) = weak_composer_for_action.upgrade() {
                            let text = editor.read_with(cx, |editor, cx| editor.text(cx)).trim().to_string();
                            if !text.is_empty() {
                                let user_message = text.clone();
                                this.messages.push(ChatMessage {
                                    role: "user".to_string(),
                                    content: user_message,
                                });
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
                                                    content: resp,
                                                });
                                                cx.notify();
                                            });
                                            eprintln!("[DEBUG] UI updated");
                                        }
                                        Ok(Ok(Err(e))) => {
                                            eprintln!("API error: {}", e);
                                        }
                                        Ok(Err(e)) => {
                                            eprintln!("Spawn error: {:?}", e);
                                        }
                                        Err(e) => {
                                            eprintln!("Tokio error: {:?}", e);
                                        }
                                    }
                                }).detach();
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
                    .bg(BRAND_BLUE)
                    .cursor_pointer()
                    .text_color(gpui::white())
                    .text_base()
                    .on_mouse_down(gpui::MouseButton::Left, cx.listener(move |this, _: &gpui::MouseDownEvent, _window, cx| {
                        if let Some(editor) = weak_composer.upgrade() {
                            let text = editor.read_with(cx, |editor, cx| editor.text(cx)).trim().to_string();
                            if !text.is_empty() {
                                let user_message = text.clone();
                                this.messages.push(ChatMessage {
                                    role: "user".to_string(),
                                    content: user_message,
                                });
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
                                                    content: resp,
                                                });
                                                cx.notify();
                                            });
                                            eprintln!("[DEBUG] UI updated");
                                        }
                                        Ok(Ok(Err(e))) => {
                                            eprintln!("API error: {}", e);
                                        }
                                        Ok(Err(e)) => {
                                            eprintln!("Spawn error: {:?}", e);
                                        }
                                        Err(e) => {
                                            eprintln!("Tokio error: {:?}", e);
                                        }
                                    }
                                }).detach();
                            }
                        }
                    }))
                    .child("Send")
            )
    }

    fn render_sidebar(&self) -> impl IntoElement {
        let sidebar_bg = Hsla { h: 0.0, s: 0.0, l: 0.96, a: 1.0 };

        div()
            .flex()
            .flex_col()
            .w(px(280.0))
            .h_full()
            .bg(sidebar_bg)
            .child(self.render_sidebar_section("Todo".to_string(), vec![]))
            .child(div().h(px(1.0)).bg(BORDER_LIGHT))
            .child(self.render_sidebar_section("Artifacts".to_string(), vec![]))
            .child(div().h(px(1.0)).bg(BORDER_LIGHT))
            .child(self.render_sidebar_section("References".to_string(), vec![]))
    }

    fn render_sidebar_section(&self, title: String, _items: Vec<String>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .p_3()
            .child(div().text_xs().text_color(MUTED_TEXT).mb_2().child(title))
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

    fn render_terminal(&mut self) -> impl IntoElement {
        eprintln!("render_terminal called with width: {}", self.terminal_width);
        let terminal_bg = Hsla { h: 0.0, s: 0.0, l: 0.08, a: 1.0 };
        let terminal_text = Hsla { h: 0.0, s: 0.0, l: 0.90, a: 1.0 };
        let prompt_color = Hsla { h: 0.35, s: 0.8, l: 0.65, a: 1.0 };
        let width = self.terminal_width;

        div()
            .flex()
            .flex_col()
            .w(px(width))
            .child(
                div()
                    .flex()
                    .items_center()
                    .h(px(36.0))
                    .px_3()
                    .bg(Hsla { h: 0.0, s: 0.0, l: 0.12, a: 1.0 })
                    .child(div().text_xs().text_color(MUTED_TEXT).child("Terminal"))
                    .child(div().text_xs().text_color(TERTIARY_TEXT).ml_auto().child("bash"))
            )
            .child(
                div()
                    .flex_1()
                    .p_3()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .children(vec![
                        div().flex().gap_2().child(div().text_sm().text_color(prompt_color).child("➜")).child(div().text_sm().text_color(terminal_text).child("~/solo3")),
                        div().text_sm().text_color(terminal_text).child(""),
                        div().text_sm().text_color(terminal_text).child("Type a message to start..."),
                    ])
            )
    }
}

fn main() {
    println!("SOLO 3.0 GUI PoC - Starting...");

    env_logger::init();

    // Load config from file
    let config = load_config();

    application().run(move |cx: &mut App| {
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
                ..Default::default()
            },
            move |window, cx| cx.new(|cx| AppState::new(window, cx, config.clone())),
        ).unwrap();
        cx.activate(true);
    });
}
