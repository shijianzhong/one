use gpui::{
    div, prelude::*, px, Context, Hsla, InteractiveElement, IntoElement,
    KeyDownEvent, ParentElement, Render, StatefulInteractiveElement, Styled, Window,
};
use std::sync::Arc;
use std::sync::Mutex;

use crate::i18n::{t, Translations};
use crate::terminal_emulator::TerminalEmulator;
use crate::terminal_emulator::mappings::keys::to_esc_str;
use crate::ui_theme::{
    BORDER_LIGHT, CANVAS_BG, INPUT_BG, MUTED_TEXT, PRIMARY_TEXT, SECONDARY_TEXT,
    SURFACE_ELEVATED, TERTIARY_TEXT,
};
use crate::{AppState, TerminalLine};

pub(crate) struct DraggedResizer;

impl Render for DraggedResizer {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size(px(0.0)).into_element()
    }
}

fn term_bg() -> Hsla {
    let mut c = CANVAS_BG();
    c.l = c.l * 0.95;
    c
}

impl AppState {
    fn ensure_terminal(&mut self, cx: &mut Context<Self>) {
        let work_dir = self.get_work_dir();
        let work_dir_path = std::path::PathBuf::from(&work_dir);
        let project_dir = if let Some(task_id) = self.active_task_id {
            work_dir_path.join(task_id.to_string())
        } else {
            work_dir_path
        };
        let _ = std::fs::create_dir_all(&project_dir);

        if self.terminal_emulator.is_some() {
            return; // 终端已存在，不需要重复 cd
        }

        match TerminalEmulator::new(None, Some(&project_dir), 80, 24) {
            Ok(term) => {
                let project_dir_str = project_dir.to_string_lossy().to_string();
                self.terminal_emulator = Some(Arc::new(Mutex::new(term)));
                eprintln!("[Terminal] Terminal emulator initialized, work_dir: {}", project_dir_str);

                // 延迟发送 cd 命令（等 shell 初始化完成）
                let term_arc = self.terminal_emulator.as_ref().unwrap().clone();
                let cd_cmd = format!("cd {} && clear\n", project_dir_str);
                cx.spawn(async move |this, cx| {
                    // 等待 500ms 让 shell 启动完成
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(500))
                        .await;
                    if let Ok(term_lock) = term_arc.lock() {
                        term_lock.write(cd_cmd.as_bytes());
                        eprintln!("[Terminal] Sent cd to project dir");
                    }

                    // 之后每 50ms 刷新终端
                    loop {
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(50))
                            .await;
                        let _ = this.update(cx, |this, cx| {
                            // 处理终端事件
                            if let Some(ref ta) = this.terminal_emulator {
                                if let Ok(t) = ta.lock() {
                                    t.process_events();
                                }
                            }
                            cx.notify();
                        });
                    }
                })
                .detach();
            }
            Err(e) => {
                eprintln!("[Terminal] Failed to initialize: {}", e);
            }
        }
    }

    pub(crate) fn render_terminal_resizer(
        &mut self,
        _cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .id("terminal-resizer")
            .w(px(6.0))
            .h_full()
            .cursor_col_resize()
            .bg(BORDER_LIGHT())
    }

    pub(crate) fn render_terminal(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        self.ensure_terminal(cx);
        let lang = self.current_lang;
        let work_dir = self.get_work_dir();

        div()
            .id("terminal-view")
            .flex()
            .flex_col()
            .size_full()
            .bg(term_bg())
            .child(self.render_terminal_header(work_dir, lang))
            .child(self.render_terminal_body(window, cx))
    }

    fn render_terminal_header(
        &self,
        work_dir: String,
        lang: crate::i18n::Lang,
    ) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .h(px(32.0))
            .px_4()
            .bg(SURFACE_ELEVATED())
            .border_b_1()
            .border_color(BORDER_LIGHT())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .size(px(8.0))
                            .rounded_full()
                            .bg(Hsla { h: 0.33, s: 0.7, l: 0.55, a: 1.0 })
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
                            .child(work_dir),
                    )
            )
    }

    fn render_terminal_body(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let has_terminal = self.terminal_emulator.is_some();
        if !has_terminal {
            return div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_sm()
                .text_color(MUTED_TEXT())
                .child("Terminal not available")
                .into_any_element();
        }

        let focus_handle = self.terminal_focus_handle.clone();
        let term_arc = self.terminal_emulator.as_ref().unwrap().clone();
        let output_lines = {
            if let Ok(term_lock) = term_arc.lock() {
                let render_lines = term_lock.renderable_lines();
                let v: Vec<(String, bool)> = render_lines
                    .iter()
                    .map(|line| {
                        let s: String = line.chars.iter().map(|c| c.c).collect();
                        let has_cursor = line.chars.iter().any(|c| c.is_cursor);
                        (s, has_cursor)
                    })
                    .collect();
                v
            } else {
                Vec::new()
            }
        };

        div()
            .id("terminal-body")
            .flex_1()
            .overflow_hidden()
            .flex()
            .flex_col()
            .key_context("terminal")
            .track_focus(&focus_handle)
            .on_mouse_down(gpui::MouseButton::Left, cx.listener(|this, _: &gpui::MouseDownEvent, window, cx| {
                window.focus(&this.terminal_focus_handle, cx);
                eprintln!("[Terminal] Focus set");
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                if let Some(ref term_arc) = this.terminal_emulator {
                    if let Ok(term) = term_arc.lock() {
                        use alacritty_terminal::term::TermMode;
                        let mode = TermMode::default();
                        if let Some(esc) = to_esc_str(&event.keystroke, &mode, false) {
                            term.write(esc.as_bytes());
                        } else if event.keystroke.key.len() == 1 {
                            // 普通可打印字符直接写入
                            let c = event.keystroke.key.as_bytes();
                            term.write(c);
                        }
                    }
                }
                cx.notify();
            }))
            .child(
                div()
                    .id("terminal-output")
                    .flex_1()
                    .overflow_scroll()
                    .p_2()
                    .font_family("Menlo")
                    .text_sm()
                    .children(output_lines.iter().map(|(text, has_cursor)| {
                        let bg_color = if *has_cursor {
                            Hsla { h: 0.61, s: 0.35, l: 0.18, a: 0.6 }
                        } else {
                            Hsla { h: 0.0, s: 0.0, l: 0.0, a: 0.0 }
                        };
                        div()
                            .h(px(18.0))
                            .flex()
                            .items_center()
                            .bg(bg_color)
                            .text_color(PRIMARY_TEXT())
                            .child(text.clone())
                            .into_any_element()
                    }))
            )
            .child(
                div()
                    .id("terminal-input-line")
                    .flex_none()
                    .h(px(36.0))
                    .px_3()
                    .bg(INPUT_BG())
                    .border_t_1()
                    .border_color(BORDER_LIGHT())
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .text_color(SECONDARY_TEXT())
                            .child("$")
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_xs()
                            .text_color(PRIMARY_TEXT())
                            .child("Terminal ready — click and type")
                    )
            )
            .into_any_element()
    }
}