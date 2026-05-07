use gpui::{
    App, AppContext as _, Bounds, Context, DragMoveEvent, EntityId,
    Hsla, IntoElement, ParentElement, px, size, Render, Task, VisualContext,
    Styled, StatefulInteractiveElement, Window, WindowOptions, WindowBounds, div, prelude::*
};
use gpui_platform::application;
use std::path::PathBuf;

use gpui::FontWeight;

struct DraggedResizer;

impl Render for DraggedResizer {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size(px(0.0)).into_element()
    }
}

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
    fn new() -> Self {
        Self {
            workspaces: vec![],
            active_workspace_id: None,
            active_task_id: None,
            sidebar_visible: false,
            terminal_visible: false,
            terminal_width: 500.0,
            terminal_resize_initial_mouse_x: None,
            terminal_resize_initial_width: None,
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
}

impl Render for AppState {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .size_full()
            .bg(CARD_BG)
            .child(self.render_nav(cx))
            .child(div().w(px(1.0)).bg(BORDER_LIGHT))
            .child(self.render_chat(cx))
            .when(self.sidebar_visible, |this| {
                this.child(div().w(px(1.0)).bg(BORDER_LIGHT))
                    .child(self.render_sidebar())
            })
            .when(self.terminal_visible, |this| {
                this.child(self.render_terminal_resizer(cx))
                    .child(self.render_terminal())
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
        div()
            .flex()
            .flex_col()
            .gap_1()
            .p_2()
            .child(self.make_nav_item("New Task", "⌘N", cx))
            .child(self.make_nav_item("Skills", "⌘S", cx))
            .child(self.make_nav_item("Automation", "⌘A", cx))
    }

    fn make_nav_item(&mut self, label: &'static str, shortcut: &'static str, cx: &mut Context<Self>) -> impl IntoElement {
        let is_new_task = label == "New Task";

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

    fn render_chat(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
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
            .child(self.render_composer())
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

    fn render_chat_messages(&self) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_4()
            .w_full()
            .child(
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
            )
    }

    fn render_composer(&mut self) -> impl IntoElement {
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
                    .text_base()
                    .text_color(PRIMARY_TEXT)
                    .child("Type a message...")
            )
            .child(
                div()
                    .px_5()
                    .py_3()
                    .rounded_lg()
                    .bg(BRAND_BLUE)
                    .text_color(gpui::white())
                    .text_base()
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

    application().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(DEFAULT_WINDOW_WIDTH), px(DEFAULT_WINDOW_HEIGHT)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                is_resizable: true,
                window_min_size: Some(size(px(800.0), px(600.0))),
                ..Default::default()
            },
            |_, cx| cx.new(|_| AppState::new()),
        ).unwrap();
        cx.activate(true);
    });
}
