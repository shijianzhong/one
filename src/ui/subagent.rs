use gpui::{
    div, prelude::*, px, svg, Context, FontWeight, Hsla, InteractiveElement, IntoElement,
    ParentElement, StatefulInteractiveElement, Styled,
};

use crate::agents::types::{SubagentEventTone, SubagentMessageState, SubagentStatus};
use crate::ui::render_icon_element;
use crate::ui_theme::{
    BRAND_BLUE, GHOST_SURFACE_BG, MUTED_TEXT, PRIMARY_TEXT, SECONDARY_TEXT, TERTIARY_TEXT,
};
use crate::AppState;

impl AppState {
    pub(crate) fn render_subagent_card(
        &mut self,
        run_id: u64,
        state: &SubagentMessageState,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let status_color = match state.status {
            SubagentStatus::Pending => MUTED_TEXT(),
            SubagentStatus::Running => BRAND_BLUE(),
            SubagentStatus::Completed => Hsla {
                h: 0.36,
                s: 0.65,
                l: 0.42,
                a: 1.0,
            },
            SubagentStatus::Failed => Hsla {
                h: 0.0,
                s: 0.72,
                l: 0.52,
                a: 1.0,
            },
        };
        let status_label = match state.status {
            SubagentStatus::Pending => "PENDING",
            SubagentStatus::Running => "RUNNING",
            SubagentStatus::Completed => "COMPLETED",
            SubagentStatus::Failed => "FAILED",
        };
        let collapsed = state.collapsed;

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
                            .bg(Hsla {
                                h: 0.55,
                                s: 0.6,
                                l: 0.5,
                                a: 1.0,
                            })
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(render_icon_element("subagent", gpui::white(), 11.0)),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(SECONDARY_TEXT())
                            .font_weight(FontWeight::BOLD)
                            .child("SUBAGENT"),
                    )
                    .child(
                        div()
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .bg(GHOST_SURFACE_BG())
                            .text_xs()
                            .text_color(status_color)
                            .child(status_label),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(TERTIARY_TEXT())
                            .child(state.status_message.clone()),
                    )
                    .when(true, |this| {
                        let icon_path = if collapsed { "expand.svg" } else { "fold.svg" };
                        let run_id_for_collapse = run_id;
                        this.child(
                            div()
                                .flex()
                                .items_center()
                                .gap_1()
                                .ml_auto()
                                .cursor_pointer()
                                .on_mouse_down(
                                    gpui::MouseButton::Left,
                                    cx.listener(
                                        move |this, _: &gpui::MouseDownEvent, _window, cx| {
                                            this.toggle_subagent_collapsed(run_id_for_collapse);
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
                                ),
                        )
                    }),
            )
            .when(!collapsed, |this| {
                this.child(
                    div()
                        .flex_col()
                        .items_start()
                        .gap_4()
                        .max_w(px(780.0))
                        .min_w(px(35.0))
                        .w_full()
                        .pl_8()
                        .pt_4()
                        .child(
                            div()
                                .flex_col()
                                .gap_1()
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(MUTED_TEXT())
                                        .child("Instruction:"),
                                )
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(PRIMARY_TEXT())
                                        .whitespace_normal()
                                        .child(state.instruction.clone()),
                                ),
                        )
                        .when(!state.events.is_empty(), |this| {
                            let events_collapsed = state.events_collapsed;
                            let run_id_for_toggle = run_id;
                            this.child(
                                div()
                                    .flex_col()
                                    .gap_2()
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
                                            .on_mouse_down(
                                                gpui::MouseButton::Left,
                                                cx.listener(
                                                    move |this,
                                                          _: &gpui::MouseDownEvent,
                                                          _window,
                                                          cx| {
                                                        this.toggle_subagent_events_collapsed(
                                                            run_id_for_toggle,
                                                        );
                                                        cx.notify();
                                                    },
                                                ),
                                            )
                                            .child(
                                                svg()
                                                    .path(if events_collapsed {
                                                        "expand.svg"
                                                    } else {
                                                        "fold.svg"
                                                    })
                                                    .size(px(14.0))
                                                    .flex_none()
                                                    .text_color(MUTED_TEXT()),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(MUTED_TEXT())
                                                    .child("Events:"),
                                            ),
                                    )
                                    .when(!events_collapsed, |this| {
                                        this.child(
                                            div()
                                                .flex_col()
                                                .gap_2()
                                                .w_full()
                                                .bg(GHOST_SURFACE_BG())
                                                .rounded_md()
                                                .max_h(px(300.0))
                                                .id("events_container")
                                                .overflow_scroll()
                                                .children(state.events.iter().map(|event| {
                                                    let event_color = match event.tone {
                                                        SubagentEventTone::Info => TERTIARY_TEXT(),
                                                        SubagentEventTone::Error => Hsla {
                                                            h: 0.0,
                                                            s: 0.72,
                                                            l: 0.52,
                                                            a: 1.0,
                                                        },
                                                    };
                                                    div().flex_col().gap_1().p_2().w_full().child(
                                                        div()
                                                            .flex()
                                                            .gap_2()
                                                            .child(
                                                                div()
                                                                    .text_xs()
                                                                    .text_color(event_color)
                                                                    .font_weight(FontWeight::BOLD)
                                                                    .child(format!(
                                                                        "[{}]",
                                                                        event.title
                                                                    )),
                                                            )
                                                            .child(
                                                                div()
                                                                    .text_xs()
                                                                    .text_color(TERTIARY_TEXT())
                                                                    .whitespace_normal()
                                                                    .w_full()
                                                                    .child(event.detail.clone()),
                                                            ),
                                                    )
                                                })),
                                        )
                                    }),
                            )
                        })
                        .when(!state.live_text.trim().is_empty(), |this| {
                            this.child(
                                div()
                                    .flex_col()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(MUTED_TEXT())
                                            .child("Output:"),
                                    )
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(PRIMARY_TEXT())
                                            .whitespace_normal()
                                            .child(state.live_text.clone()),
                                    ),
                            )
                        })
                        .when(!state.stderr_lines.is_empty(), |this| {
                            this.child(
                                div()
                                    .flex_col()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(Hsla {
                                                h: 0.0,
                                                s: 0.72,
                                                l: 0.52,
                                                a: 1.0,
                                            })
                                            .child("Errors:"),
                                    )
                                    .children(state.stderr_lines.iter().map(|line| {
                                        div()
                                            .text_xs()
                                            .text_color(Hsla {
                                                h: 0.0,
                                                s: 0.72,
                                                l: 0.52,
                                                a: 1.0,
                                            })
                                            .font_family("monospace")
                                            .child(line.clone())
                                    })),
                            )
                        }),
                )
            })
    }
}
