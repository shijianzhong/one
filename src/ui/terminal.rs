use editor::Editor;
use gpui::{
    div, prelude::*, px, Context, DragMoveEvent, Focusable, Hsla, InteractiveElement, IntoElement,
    ParentElement, Render, StatefulInteractiveElement, Styled, Window,
};
use menu::Confirm;

use crate::i18n::{t, Translations};
use crate::sandbox::backend::{Backend, SandboxBackend};
use crate::ui_theme::{
    BORDER_LIGHT, CANVAS_BG, MUTED_TEXT, PRIMARY_TEXT, SECONDARY_TEXT, SURFACE_ELEVATED,
    SURFACE_PANEL, TERTIARY_TEXT, WORKSPACE_BG,
};
use crate::{AppState, TerminalLine};

pub(crate) struct DraggedResizer;

impl Render for DraggedResizer {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size(px(0.0)).into_element()
    }
}

impl AppState {
    pub(crate) fn render_terminal_resizer(
        &mut self,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
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
                        this.right_panel_resize_initial_mouse_x,
                        this.right_panel_resize_initial_width,
                    ) {
                        let current_x: f32 = e.event.position.x.into();
                        let delta = initial_x - current_x;
                        let new_width = initial_width + delta;
                        eprintln!(
                            "drag_move: initial_x={}, current_x={}, delta={}, new_width={}",
                            initial_x, current_x, delta, new_width
                        );
                        if new_width >= 200.0 && new_width <= 1000.0 {
                            this.right_panel_width = new_width;
                        }
                    }
                }),
            )
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, event: &gpui::MouseDownEvent, _window, _cx| {
                    let initial_mouse_x: f32 = event.position.x.into();
                    this.right_panel_resize_initial_mouse_x = Some(initial_mouse_x);
                    this.right_panel_resize_initial_width = Some(this.right_panel_width);
                    eprintln!(
                        "on_mouse_down: initial_mouse_x={}, initial_width={}",
                        initial_mouse_x, this.right_panel_width
                    );
                }),
            )
    }

    pub(crate) fn render_terminal(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
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
        let lang = self.current_lang;

        let work_dir = self.get_work_dir();

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
            .size_full()
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

                                        editor.update(cx, |editor, cx| {
                                            editor.set_text("", _window, cx);
                                        });

                                        cx.spawn(async move |this, cx| {
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
