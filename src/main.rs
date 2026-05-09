use gpui::{
    svg,
    App, AppContext as _, Bounds, Context, DragMoveEvent,
    Hsla, IntoElement, ParentElement, px, size, Render,
    Styled, StatefulInteractiveElement, Window, WindowOptions, WindowBounds, div, prelude::*,
    Focusable, ScrollHandle,
};
use std::sync::Arc;
use std::path::PathBuf;
use image::RgbaImage;

use gpui_platform::application;
use editor::Editor;
use menu::Confirm;
use settings::{KeymapFile, DEFAULT_KEYMAP_PATH};
use theme;
use theme_settings;

use gpui::FontWeight;

mod memory;
mod sandbox;
mod services;

use memory::types::ChatMessage;
use sandbox::backend::{Backend, SandboxBackend};
use services::{Config, load_config, save_config};
use services::api::call_chat_api_sync;

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
    chat_scroll_handle: ScrollHandle,
    needs_auto_scroll: bool,
    sandbox_backend: Backend,
    // Terminal state
    terminal_output: Vec<TerminalLine>,
    terminal_input: String,
    terminal_history: Vec<String>,
    terminal_history_index: isize,
}

#[derive(Debug, Clone)]
struct TerminalLine {
    command: Option<String>,
    output: String,
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
    fn new(_window: &mut Window, _cx: &mut Context<Self>, config: Config) -> Self {
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
            needs_auto_scroll: false,
            chat_scroll_handle: ScrollHandle::default(),
            sandbox_backend: futures::executor::block_on(Backend::detect()),
            terminal_output: vec![],
            terminal_input: String::new(),
            terminal_history: vec![],
            terminal_history_index: -1,
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
                    .child(self.render_terminal(_window, cx))
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
            .p_3()
            .id("task-list")
            .overflow_scroll();

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
                .on_mouse_down(gpui::MouseButton::Left, cx.listener(move |this, _: &gpui::MouseDownEvent, _window, _cx| {
                    this.active_workspace_id = Some(ws_id);
                    if let Some(ws) = this.workspaces.iter_mut().find(|w| w.id == ws_id) {
                        ws.expanded = !ws.expanded;
                    }
                }));

            // Workspace expand/collapse icon - visual only, click handled by ws_row
            let expand_btn = div()
                .text_base()
                .text_color(MUTED_TEXT)
                .px_1()
                .py_1();
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
                    this.add_task_to_workspace(ws_id, "New Task".to_string(), cx);
                }));

            let ws_label = workspace.name.clone();
            result = result.child(
                ws_row.child(
                    if workspace.expanded {
                        expand_btn.child(
                            svg().external_path("assets/expand.svg").w(px(16.0)).h(px(16.0)).text_color(MUTED_TEXT)
                        )
                    } else {
                        expand_btn.child(
                            svg().external_path("assets/fold.svg").w(px(16.0)).h(px(16.0)).text_color(MUTED_TEXT)
                        )
                    }
                ).child(
                    svg().external_path("assets/folder.svg").w(px(16.0)).h(px(16.0)).text_color(MUTED_TEXT)
                ).child(
                    div().text_sm().ml_1().text_color(if is_active_ws { BRAND_BLUE } else { PRIMARY_TEXT }).child(ws_label)
                ).child(
                    div().ml_auto().child(add_btn.child("+"))
                )
            );

            // Tasks under workspace (if expanded) - each workspace's tasks are in their own scrollable container
            if workspace.expanded {
                let mut tasks_container = div()
                    .flex_col()
                    .ml_4();

                for task in &workspace.tasks {
                    let is_active_task = active_task_id == Some(task.id) && active_workspace_id == Some(workspace.id);

                    let mut task_div = div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_3()
                        .py_2()
                        .ml_4()
                        .rounded_md()
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
        let scroll_handle = self.chat_scroll_handle.clone();

        div()
            .flex()
            .flex_col()
            .flex_1()
            .h_full()
            .min_w(px(350.0))
            .child(self.render_chat_header(title, sidebar_visible, terminal_visible, cx))
            .child(
                div()
                    .id("chat_container")
                    .flex_1()
                    .w_full()
                    .overflow_y_scroll()
                    .track_scroll(&scroll_handle)
                    .p_4()
                    .child(self.render_chat_messages(&scroll_handle, cx))
            )
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

    fn render_chat_messages(&mut self, scroll_handle: &ScrollHandle, cx: &mut Context<Self>) -> impl IntoElement {
        let messages = self.messages.clone();
        let is_user = |role: &str| role == "user";

        // Auto-scroll to bottom only when needs_auto_scroll is set
        if self.needs_auto_scroll && !messages.is_empty() {
            scroll_handle.scroll_to_bottom();
            self.needs_auto_scroll = false;
        }

        div()
            .flex_col()
            .gap_4()
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
                let role_label = if is_user_msg { "You" } else { "Assistant" };

                // User messages: right aligned, Assistant messages: left aligned
                let message_container = if is_user_msg {
                    div()
                        .flex()
                        .justify_end()
                        .w_full()
                        .child(
                            div()
                                .flex_col()
                                .items_end()
                                .gap_1()
                                .p_4()
                                .rounded_2xl()
                                .bg(bubble_bg)
                                .max_w(px(520.0))
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
                                .gap_1()
                                .p_4()
                                .rounded_2xl()
                                .bg(bubble_bg)
                                .max_w(px(520.0))
                                .w_full()
                                .child(
                                    div()
                                        .text_base()
                                        .text_color(text_color)
                                        .whitespace_normal()
                                        .child(msg.content.clone())
                                )
                        )
                };

                message_container
            }))
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
                                                    content: resp,
                                                });
                                                this.needs_auto_scroll = true;
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
                                                    content: resp,
                                                });
                                                this.needs_auto_scroll = true;
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

    fn execute_terminal_command(&mut self, cx: &mut Context<Self>) {
        let cmd = self.terminal_input.trim().to_string();
        if cmd.is_empty() {
            return;
        }

        // Add to history
        if !self.terminal_history.contains(&cmd) {
            self.terminal_history.push(cmd.clone());
        }
        self.terminal_history_index = self.terminal_history.len() as isize;
        self.terminal_input.clear();

        let task_id = self.active_task_id.unwrap_or(0);

        // Clone backend for use in async task
        let sandbox_backend = self.sandbox_backend.clone();

        // Execute command in sandbox using tokio::spawn
        tokio::spawn(async move {
            let output = match &sandbox_backend {
                Backend::Docker(b) => b.exec_command(task_id, vec![&cmd]).await,
                Backend::Pty(b) => b.exec_command(task_id, vec![&cmd]).await,
            };

            let result = match output {
                Ok(out) => out,
                Err(e) => format!("Error: {}", e),
            };

            // Note: Since we can't easily update state from a raw tokio::spawn,
            // we log the result for now. In a real implementation, we'd use
            // a channel or different approach to communicate back to the UI.
            eprintln!("[Terminal] {}: {}\n{}", cmd, result, result);
        });
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
        let work_dir = self.active_task_id
            .map(|id| format!("/tmp/solo3_task_{}", id))
            .unwrap_or_else(|| "~/solo3".to_string());

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
                                            this.update(cx, |this, cx| {
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
    println!("SOLO 3.0 GUI PoC - Starting...");

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
                icon: icon_image.clone(),
                ..Default::default()
            },
            move |window, cx| cx.new(|cx| AppState::new(window, cx, config.clone())),
        ).unwrap();
        cx.activate(true);
    });
}
