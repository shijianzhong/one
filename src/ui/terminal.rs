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
        let project_dir = std::path::PathBuf::from(self.get_work_dir());
        let _ = std::fs::create_dir_all(&project_dir);

        if self.terminal_emulator.is_some()
            && self.terminal_work_dir.as_ref() == Some(&project_dir)
        {
            self.ensure_terminal_refresh_loop(cx);
            return;
        }

        self.terminal_emulator = None;
        self.terminal_work_dir = None;
        self.terminal_refresh_generation = self.terminal_refresh_generation.wrapping_add(1);
        self.terminal_refresh_running = false;

        match TerminalEmulator::new(None, Some(&project_dir), 80, 24) {
            Ok(term) => {
                let project_dir_str = project_dir.to_string_lossy().to_string();
                self.terminal_emulator = Some(Arc::new(Mutex::new(term)));
                self.terminal_work_dir = Some(project_dir.clone());
                eprintln!("[Terminal] Terminal emulator initialized, target dir: {}", project_dir_str);

                self.ensure_terminal_refresh_loop(cx);
            }
            Err(e) => {
                eprintln!("[Terminal] Failed to initialize: {}", e);
            }
        }
    }

    fn ensure_terminal_refresh_loop(&mut self, cx: &mut Context<Self>) {
        if self.terminal_refresh_running {
            return;
        }

        self.terminal_refresh_running = true;
        let generation = self.terminal_refresh_generation;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(50))
                    .await;
                let should_continue = this
                    .update(cx, |this, cx| {
                        if this.terminal_refresh_generation != generation
                            || !this.terminal_visible
                            || this.terminal_emulator.is_none()
                        {
                            this.terminal_refresh_running = false;
                            return false;
                        }
                        if let Some(ref ta) = this.terminal_emulator {
                            if let Ok(t) = ta.lock() {
                                t.process_events();
                            }
                        }
                        cx.notify();
                        true
                    })
                    .unwrap_or(false);
                if !should_continue {
                    break;
                }
            }
        })
        .detach();
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
                        } else if event.keystroke.key == "space" {
                            term.write(b" ");
                        } else if event.keystroke.key.len() == 1 {
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
                        let display_text = if *has_cursor && !text.is_empty() {
                            // 在光标位置插入光标符号
                            let cursor_pos = text.trim_end().len();
                            let prefix = &text[..cursor_pos];
                            let suffix = &text[cursor_pos..];
                            format!("{}█{}", prefix, suffix)
                        } else {
                            text.clone()
                        };
                        div()
                            .h(px(18.0))
                            .flex()
                            .items_center()
                            .text_color(PRIMARY_TEXT())
                            .child(display_text)
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
