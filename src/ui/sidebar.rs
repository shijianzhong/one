use editor::Editor;
use gpui::{
    div, prelude::*, px, Context, Focusable, FontWeight, Hsla, InteractiveElement, IntoElement,
    ParentElement, StatefulInteractiveElement, Styled, Window,
};
use menu::Confirm;

use crate::i18n::{t, Translations};
use crate::ui::render_formatted_content;
use crate::ui_theme::{
    BORDER_LIGHT, BRAND_BLUE, CANVAS_BG, MUTED_TEXT, PRIMARY_TEXT, SECONDARY_TEXT,
    SURFACE_ELEVATED, SURFACE_PANEL, WORKSPACE_BG,
};
use crate::AppState;

impl AppState {
    pub(crate) fn render_sidebar(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
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
                                            .child("RUN"),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(gpui::white())
                                            .px_2()
                                            .py_1()
                                            .rounded_md()
                                            .bg(status_color)
                                            .child(run.status.label(lang)),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(MUTED_TEXT())
                                            .child(run.status_message.clone()),
                                    ),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(PRIMARY_TEXT())
                                    .font_weight(FontWeight::BOLD)
                                    .whitespace_normal()
                                    .child(run.instruction.clone()),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(MUTED_TEXT())
                                    .whitespace_normal()
                                    .child(format!(
                                        "{}: {}",
                                        t(lang, Translations::WORKDIR),
                                        run.work_dir
                                    )),
                            ),
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
                                    .text_xs()
                                    .text_color(MUTED_TEXT())
                                    .child(t(lang, Translations::PROGRESS)),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(PRIMARY_TEXT())
                                    .whitespace_normal()
                                    .child(run.status_message.clone()),
                            ),
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
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(MUTED_TEXT())
                                            .child(t(lang, Translations::PREVIEW)),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(preview_color)
                                            .font_weight(FontWeight::BOLD)
                                            .child(preview_label),
                                    ),
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
                                            .unwrap_or_else(|| {
                                                t(lang, Translations::NO_PREVIEW_INFO).to_string()
                                            }),
                                    ),
                            )
                            .when_some(
                                preview.clone().and_then(|preview| preview.entry_file),
                                |this, entry_file| {
                                    this.child(
                                        div()
                                            .text_xs()
                                            .text_color(MUTED_TEXT())
                                            .whitespace_normal()
                                            .child(format!(
                                                "{}: {}",
                                                t(lang, Translations::ENTRY),
                                                entry_file
                                            )),
                                    )
                                },
                            )
                            .when_some(
                                preview.clone().and_then(|preview| preview.url),
                                |this, url| {
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
                                                    .child(url.clone()),
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
                                                    .on_mouse_down(
                                                        gpui::MouseButton::Left,
                                                        cx.listener({
                                                            let url = url.clone();
                                                            move |this,
                                                                  _: &gpui::MouseDownEvent,
                                                                  _window,
                                                                  _cx| {
                                                                this.open_url_in_browser(&url);
                                                            }
                                                        }),
                                                    )
                                                    .child(t(lang, Translations::OPEN_IN_BROWSER)),
                                            ),
                                    )
                                },
                            ),
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
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(MUTED_TEXT())
                                            .child(t(lang, Translations::ARTIFACTS)),
                                    )
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
                                                cx.listener(
                                                    move |this,
                                                          _: &gpui::MouseDownEvent,
                                                          _window,
                                                          _cx| {
                                                        this.open_folder_in_finder(&task_dir);
                                                    },
                                                )
                                            })
                                            .child(t(lang, Translations::OPEN_TASK_FOLDER)),
                                    ),
                            )
                            .child(
                                div().flex_col().gap_2().children(
                                    run.artifacts.iter().take(12).cloned().map(|artifact| {
                                        let absolute_path = artifact.absolute_path.clone();
                                        let label = format!(
                                            "{} · {}",
                                            artifact.relative_path, artifact.kind
                                        );
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
                                                    .child(label),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(BRAND_BLUE())
                                                    .cursor_pointer()
                                                    .on_mouse_down(
                                                        gpui::MouseButton::Left,
                                                        cx.listener(
                                                            move |this,
                                                                  _: &gpui::MouseDownEvent,
                                                                  _window,
                                                                  _cx| {
                                                                this.reveal_file_in_finder(
                                                                    &absolute_path,
                                                                );
                                                            },
                                                        ),
                                                    )
                                                    .child(t(lang, Translations::REVEAL)),
                                            )
                                            .into_any_element()
                                    }),
                                ),
                            )
                            .when(run.artifacts.is_empty(), |this| {
                                this.child(
                                    div()
                                        .text_xs()
                                        .text_color(SECONDARY_TEXT())
                                        .child(t(lang, Translations::NO_ARTIFACTS_YET)),
                                )
                            }),
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
                                    .text_xs()
                                    .text_color(MUTED_TEXT())
                                    .child(t(lang, Translations::QUESTIONS)),
                            )
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
                                                .child(question.prompt.clone()),
                                        )
                                        .when(!question.options.is_empty(), |this| {
                                            this.child(div().flex().flex_col().gap_2().children(
                                                question.options.iter().cloned().map(|option| {
                                                    let option_label = option.clone();
                                                    div()
                                                        .px_3()
                                                        .py_2()
                                                        .rounded_md()
                                                        .bg(WORKSPACE_BG())
                                                        .cursor_pointer()
                                                        .text_xs()
                                                        .text_color(PRIMARY_TEXT())
                                                        .on_mouse_down(
                                                            gpui::MouseButton::Left,
                                                            cx.listener(
                                                                move |this,
                                                                      _: &gpui::MouseDownEvent,
                                                                      _window,
                                                                      cx| {
                                                                    this.continue_claude_with_answer(
                                                                        option.clone(),
                                                                        cx,
                                                                    );
                                                                },
                                                            ),
                                                        )
                                                        .child(option_label)
                                                        .into_any_element()
                                                }),
                                            ))
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
                                                            let weak_question_editor =
                                                                weak_question_editor.clone();
                                                            move |this, _: &Confirm, _window, cx| {
                                                                if let Some(editor) =
                                                                    weak_question_editor.upgrade()
                                                                {
                                                                    let answer = editor
                                                                        .read_with(
                                                                            cx,
                                                                            |editor, cx| {
                                                                                editor.text(cx)
                                                                            },
                                                                        )
                                                                        .trim()
                                                                        .to_string();
                                                                    if !answer.is_empty() {
                                                                        editor.update(
                                                                            cx,
                                                                            |editor, cx| {
                                                                                editor.set_text(
                                                                                    "", _window,
                                                                                    cx,
                                                                                )
                                                                            },
                                                                        );
                                                                        this.continue_claude_with_answer(
                                                                            answer, cx,
                                                                        );
                                                                    }
                                                                }
                                                            }
                                                        }))
                                                        .child(question_editor.clone()),
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
                                                        .on_mouse_down(
                                                            gpui::MouseButton::Left,
                                                            cx.listener({
                                                                let weak_question_editor =
                                                                    weak_question_editor.clone();
                                                                move |this,
                                                                      _: &gpui::MouseDownEvent,
                                                                      _window,
                                                                      cx| {
                                                                    if let Some(editor) =
                                                                        weak_question_editor
                                                                            .upgrade()
                                                                    {
                                                                        let answer = editor
                                                                            .read_with(
                                                                                cx,
                                                                                |editor, cx| {
                                                                                    editor.text(cx)
                                                                                },
                                                                            )
                                                                            .trim()
                                                                            .to_string();
                                                                        if !answer.is_empty() {
                                                                            editor.update(
                                                                                cx,
                                                                                |editor, cx| {
                                                                                    editor
                                                                                        .set_text(
                                                                                            "",
                                                                                            _window,
                                                                                            cx,
                                                                                        )
                                                                                },
                                                                            );
                                                                            this.continue_claude_with_answer(
                                                                                answer, cx,
                                                                            );
                                                                        }
                                                                    }
                                                                }
                                                            }),
                                                        )
                                                        .child(t(lang, Translations::SUBMIT)),
                                                ),
                                        ),
                                )
                            })
                            .when(pending_question.is_none(), |this| {
                                this.child(
                                    div()
                                        .text_xs()
                                        .text_color(SECONDARY_TEXT())
                                        .child(t(lang, Translations::NO_PENDING_QUESTIONS)),
                                )
                            }),
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
                                    .text_xs()
                                    .text_color(MUTED_TEXT())
                                    .child(t(lang, Translations::LIVE_OUTPUT)),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(PRIMARY_TEXT())
                                    .whitespace_normal()
                                    .child(live_output),
                            ),
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
                                    .text_xs()
                                    .text_color(MUTED_TEXT())
                                    .child(t(lang, Translations::COMMAND)),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(SECONDARY_TEXT())
                                    .whitespace_normal()
                                    .child(if run.command_preview.is_empty() {
                                        t(lang, Translations::COMMAND_NOT_STARTED).to_string()
                                    } else {
                                        run.command_preview.clone()
                                    }),
                            ),
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
                                    .text_xs()
                                    .text_color(MUTED_TEXT())
                                    .child(t(lang, Translations::STDERR)),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(if run.stderr_lines.is_empty() {
                                        SECONDARY_TEXT()
                                    } else {
                                        Hsla {
                                            h: 0.0,
                                            s: 0.72,
                                            l: 0.52,
                                            a: 1.0,
                                        }
                                    })
                                    .whitespace_normal()
                                    .child(stderr_preview),
                            ),
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
                                    .text_xs()
                                    .text_color(MUTED_TEXT())
                                    .child(t(lang, Translations::TIMELINE)),
                            )
                            .child(timeline),
                    ),
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
}
