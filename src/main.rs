use gpui::{
    App, AppContext as _, Bounds, Context, DragMoveEvent,
    Hsla, IntoElement, ParentElement, px, size, Render,
    Styled, Window, WindowOptions, WindowBounds, div, prelude::*
};
use gpui_platform::application;

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

const NAV_WIDTH: f32 = 240.0;
const DEFAULT_WINDOW_WIDTH: f32 = 1200.0;
const DEFAULT_WINDOW_HEIGHT: f32 = 760.0;

struct AppState {
    chat: Vec<ChatItem>,
    chat_title: String,
    active_nav: &'static str,
    active_task_id: usize,
    sidebar_visible: bool,
    terminal_visible: bool,
    terminal_width: f32,
    terminal_resize_initial_mouse_x: Option<f32>,
    terminal_resize_initial_width: Option<f32>,
    tasks: Vec<TaskItem>,
    todos: Vec<String>,
    artifacts: Vec<String>,
    references: Vec<String>,
}

#[derive(Debug, Clone)]
struct ChatItem {
    role: &'static str,
    content: String,
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
            chat: vec![
                ChatItem {
                    role: "assistant",
                    content: "Hello! I'm your SOLO 3.0 assistant. How can I help you today?".to_string(),
                }
            ],
            chat_title: "Design UI mockups".to_string(),
            active_nav: "tasks",
            active_task_id: 1,
            sidebar_visible: false,
            terminal_visible: false,
            terminal_width: 500.0,
            terminal_resize_initial_mouse_x: None,
            terminal_resize_initial_width: None,
            tasks: vec![
                TaskItem { id: 1, title: "Design UI mockups".to_string(), status: "in_progress" },
                TaskItem { id: 2, title: "Implement navigation".to_string(), status: "todo" },
                TaskItem { id: 3, title: "Add chat functionality".to_string(), status: "todo" },
                TaskItem { id: 4, title: "Test and polish".to_string(), status: "todo" },
            ],
            todos: vec!["Create wireframes", "Design color palette", "Test on mobile"]
                .into_iter()
                .map(String::from)
                .collect(),
            artifacts: vec!["mockups.fig", "style-guide.pdf", "components.zip"]
                .into_iter()
                .map(String::from)
                .collect(),
            references: vec!["https://trae.ai", "https://docs.example.com"]
                .into_iter()
                .map(String::from)
                .collect(),
        }
    }
}

impl Render for AppState {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .size_full()
            .bg(CARD_BG)
            .on_drag_move(cx.listener(
                move |this: &mut AppState, e: &DragMoveEvent<DraggedResizer>, _window, _cx| {
                    if let (Some(initial_x), Some(initial_width)) = (this.terminal_resize_initial_mouse_x, this.terminal_resize_initial_width) {
                        let current_x: f32 = e.event.position.x.into();
                        let delta = current_x - initial_x;
                        let new_width = initial_width + delta;
                        eprintln!("drag_move: initial_x={}, current_x={}, delta={}, new_width={}", initial_x, current_x, delta, new_width);
                        if new_width >= 200.0 && new_width <= 800.0 {
                            this.terminal_width = new_width;
                        }
                    }
                },
            ))
            .child(self.render_nav())
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
    fn render_nav(&self) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .w(px(NAV_WIDTH))
            .h_full()
            .bg(NAV_BG)
            .child(self.render_nav_header())
            .child(div().h(px(1.0)).bg(BORDER_LIGHT))
            .child(self.render_nav_buttons())
            .child(div().h(px(1.0)).bg(BORDER_LIGHT))
            .child(self.render_task_list())
    }

    fn render_nav_header(&self) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .h(px(40.0))
            .px_4()
            .child(div().text_base().text_color(PRIMARY_TEXT).font_weight(FontWeight::BOLD).child("SOLO"))
    }

    fn render_nav_buttons(&self) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_1()
            .p_2()
            .child(self.make_nav_item("New Task", "⌘N", "tasks", true))
            .child(self.make_nav_item("Skills", "⌘S", "skills", false))
            .child(self.make_nav_item("Automation", "⌘A", "automation", false))
    }

    fn make_nav_item(&self, label: &'static str, shortcut: &'static str, nav_id: &'static str, _active: bool) -> impl IntoElement {
        let is_active = self.active_nav == nav_id;
        let bg = if is_active { ACTIVE_BG } else { CARD_BG };
        let text_color = if is_active { BRAND_BLUE } else { SECONDARY_TEXT };

        div()
            .flex()
            .items_center()
            .gap_3()
            .px_3()
            .py_2()
            .rounded_md()
            .bg(bg)
            .cursor_pointer()
            .child(div().text_sm().text_color(text_color).child(label))
            .child(div().text_xs().text_color(MUTED_TEXT).ml_auto().child(shortcut))
    }

    fn render_task_list(&self) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .flex_1()
            .overflow_hidden()
            .p_3()
            .child(div().text_xs().text_color(MUTED_TEXT).mb_3().child("TASKS"))
            .children(self.tasks.iter().map(|task| {
                let is_active = task.id == 1;
                let bg = if is_active { ACTIVE_BG } else { CARD_BG };
                let text_color = if is_active { BRAND_BLUE } else { PRIMARY_TEXT };

                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .bg(bg)
                    .child(div().w(px(6.0)).h(px(6.0)).rounded_full().bg(if is_active { BRAND_BLUE } else { MUTED_TEXT }))
                    .child(div().text_sm().text_color(text_color).child(task.title.clone()))
            }))
    }

    fn render_chat(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .flex_1()
            .h_full()
            .min_w(px(350.0))
            .child(self.render_chat_header(cx))
            .child(div().flex_1().overflow_hidden().p_4().child(self.render_chat_messages()))
            .child(div().h(px(1.0)).bg(BORDER_LIGHT))
            .child(self.render_composer())
    }

    fn render_chat_header(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let title = self.tasks.iter().find(|t| t.id == self.active_task_id).map(|t| t.title.clone()).unwrap_or_default();
        let sidebar_visible = self.sidebar_visible;
        let terminal_visible = self.terminal_visible;

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
            .children(self.chat.iter().map(|item| {
                let (bg, border_color, role_text) = if item.role == "user" {
                    (ACTIVE_BG, BRAND_BLUE, "You")
                } else {
                    (CARD_BG, BORDER_LIGHT, "Assistant")
                };

                let text_color = if item.role == "user" { PRIMARY_TEXT } else { SECONDARY_TEXT };

                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .p_4()
                    .rounded_lg()
                    .bg(bg)
                    .border_1()
                    .border_color(border_color)
                    .child(div().text_xs().text_color(MUTED_TEXT).child(role_text))
                    .child(div().text_base().text_color(text_color).child(item.content.clone()))
            }))
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
            .child(self.render_sidebar_section("Todo".to_string(), self.todos.clone()))
            .child(div().h(px(1.0)).bg(BORDER_LIGHT))
            .child(self.render_sidebar_section("Artifacts".to_string(), self.artifacts.clone()))
            .child(div().h(px(1.0)).bg(BORDER_LIGHT))
            .child(self.render_sidebar_section("References".to_string(), self.references.clone()))
    }

    fn render_sidebar_section(&self, title: String, items: Vec<String>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .p_3()
            .child(div().text_xs().text_color(MUTED_TEXT).mb_2().child(title))
            .children(items.into_iter().map(|item| {
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .py_1()
                    .text_sm()
                    .text_color(SECONDARY_TEXT)
                    .child(div().text_color(TERTIARY_TEXT).child("○"))
                    .child(item)
            }))
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
                    let delta = current_x - initial_x;
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
