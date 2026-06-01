use editor::Editor;
use gpui::{
    div, prelude::*, px, relative, svg, Animation, AnimationExt, Context, Focusable,
    FontWeight, Hsla, InteractiveElement, IntoElement, ParentElement, ScrollHandle,
    StatefulInteractiveElement, Styled, Window,
};
use std::time::Duration;
use menu::Confirm;

use crate::agents::types::{ClaudeRunPanelState, RequestKind, SubagentMessageState};
use crate::i18n::{t, Lang, Translations};
use crate::ui::{parse_think_content, render_icon_element, render_process_table, ContentPart};
use crate::ui_theme::{
    ACCENT_TEXT, ACTIVE_BG, ASSISTANT_BUBBLE_BG, AVATAR_BG, BORDER_LIGHT, BRAND_BLUE, CANVAS_BG,
    FLOATING_PANEL_BG, GHOST_SURFACE_BG, HEADER_BG, INPUT_BG, MUTED_TEXT, PRIMARY_TEXT,
    SECONDARY_TEXT, SURFACE_ELEVATED, TERTIARY_TEXT, USER_BUBBLE_BG,
};
use crate::{
    escape_visible_snippet, normalize_single_line_label, AppState, ExportChat, HeaderTooltip,
};

impl AppState {
    pub(crate) fn render_chat(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
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

    pub(crate) fn render_chat_header(
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
        let lang = self.current_lang;
        let coming_soon = t(lang, Translations::COMING_SOON).to_string();
        div()
            .text_xs()
            .text_color(MUTED_TEXT())
            .opacity(0.60)
            .cursor_default()
            .id("placeholder_header_tab")
            .tooltip(move |_, cx| {
                cx.new(|_| HeaderTooltip {
                    text: coming_soon.clone(),
                })
                .into()
            })
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
                        Some("terminal") => {
                            this.terminal_visible = !this.terminal_visible;
                            if this.terminal_visible {
                                // Auto focus terminal editor
                                let terminal_editor =
                                    _window.use_keyed_state("terminal_editor", _cx, |window, cx| {
                                        let mut editor = editor::Editor::single_line(window, cx);
                                        editor.set_placeholder_text(
                                            crate::i18n::t(this.current_lang, crate::i18n::Translations::TYPE_COMMAND),
                                            window,
                                            cx,
                                        );
                                        editor
                                    });
                                terminal_editor.update(_cx, |editor, cx| {
                                    editor.focus_handle(cx).focus(_window, cx);
                                });
                            }
                        }
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

    pub(crate) fn render_chat_messages(
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

                let parts = parse_think_content(&msg.content);

                let message_container = if is_user_msg {
                    div()
                        .flex()
                        .justify_end()
                        .w_full()
                        .mb_6()
                        .child(
                            div()
                                .flex_col()
                                .items_end()
                                .gap_2()
                                .px_5()
                                .py_3()
                                .rounded_xl()
                                .bg(bubble_bg)
                                .shadow_md()
                                .max_w(px(680.0))
                                .min_w(px(35.0))
                                .child(
                                    div()
                                        .text_base()
                                        .text_color(text_color)
                                        .line_height(relative(1.5))
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
                        .mb_8()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .mb_1()
                                .child(
                                    div()
                                        .size(px(26.0))
                                        .rounded_full()
                                        .bg(AVATAR_BG())
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .shadow_sm()
                                        .child(render_icon_element("assistant", gpui::white(), 13.0))
                                )
                                .child(
                                    div()
                                        .text_sm()
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
                                .max_w(px(840.0))
                                .min_w(px(35.0))
                                .w_full()
                                .px_6()
                                .py_5()
                                .rounded_xl()
                                .bg(ASSISTANT_BUBBLE_BG())
                                .border_1()
                                .border_color(BORDER_LIGHT())
                                .shadow_sm()
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
                                                    .line_height(relative(1.6))
                                                    .whitespace_normal()
                                                    .child(text.clone());
                                                let el = if add_top_padding { el.pt_2() } else { el };
                                                
                                                let animation_id = format!("msg-{}-part-{}", msg_index, rendered_parts.len());
                                                rendered_parts.push(
                                                    el.with_animation(
                                                        animation_id,
                                                        Animation::new(Duration::from_millis(400)),
                                                        |el, delta| el.opacity(0.4 + delta * 0.6)
                                                    ).into_any_element()
                                                );
                                            }
                                            ContentPart::ProcessTable { processes } => {
                                                prev_was_think = false;
                                                let el = render_process_table(processes);
                                                let animation_id = format!("msg-{}-proc-{}", msg_index, rendered_parts.len());
                                                rendered_parts.push(
                                                    div().child(el).with_animation(
                                                        animation_id,
                                                        Animation::new(Duration::from_millis(500)),
                                                        |el, delta| el.opacity(0.4 + delta * 0.6)
                                                    ).into_any_element()
                                                );
                                            }
                                            ContentPart::Think { text, complete } => {
                                                prev_was_think = true;
                                                let current_think_index = think_index;
                                                think_index += 1;
                                                let complete = *complete;
                                                let key = format!("task:{}:msg:{}:think:{}", task_id, msg_index, current_think_index);
                                                let key_for_animation = key.clone();
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

                                                let header = div()
                                                            .flex()
                                                            .items_center()
                                                            .gap_2()
                                                            .px_3()
                                                            .py_1p5()
                                                            .rounded_lg()
                                                            .bg(GHOST_SURFACE_BG())
                                                            .cursor_pointer()
                                                            .hover(|this| this.bg(SURFACE_ELEVATED()))
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
                                                            );
                                                
                                                let header_any = if !complete {
                                                    header.with_animation(
                                                        format!("thinking-{}", key_for_animation),
                                                        Animation::new(Duration::from_secs(2)).repeat(),
                                                        |el, delta| el.opacity(0.6 + gpui::pulsating_between(0.0, 0.4)(delta))
                                                    ).into_any_element()
                                                } else {
                                                    header.into_any_element()
                                                };

                                                let el = div()
                                                    .flex_col()
                                                    .w_full()
                                                    .child(header_any)
                                                    .when(!collapsed, |this| {
                                                        this.child(
                                                            div()
                                                                .mt_2()
                                                                .px_3()
                                                                .text_xs()
                                                                .text_color(TERTIARY_TEXT())
                                                                .line_height(relative(1.5))
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
                                                    .child(t(lang, Translations::CONFIRM_EXECUTE))
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
                                                    .child(t(lang, Translations::CANCEL))
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

        let current_task_id = self.active_task_id;
        let active_subagents: Vec<(u64, SubagentMessageState)> = self
            .subagent_messages
            .iter()
            .filter(|(_, state)| state.task_id == current_task_id)
            .map(|(run_id, state)| (*run_id, state.clone()))
            .collect();

        for (run_id, state) in active_subagents {
            message_list = message_list.child(self.render_subagent_card(run_id, &state, cx));
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
                            .line_height(relative(1.6))
                            .whitespace_normal()
                            .child(text.clone());
                        let el = if add_top_padding { el.pt_2() } else { el };
                        
                        let animation_id = format!("live-{}-part-{}", run_id, rendered_parts.len());
                        rendered_parts.push(
                            el.with_animation(
                                animation_id,
                                Animation::new(Duration::from_millis(300)),
                                |el, delta| el.opacity(0.5 + delta * 0.5)
                            ).into_any_element()
                        );
                    }
                    ContentPart::ProcessTable { processes } => {
                        prev_was_think = false;
                        let el = render_process_table(processes);
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
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .px_3()
                                    .py_1p5()
                                    .rounded_lg()
                                    .bg(SURFACE_ELEVATED())
                                    .border_1()
                                    .border_color(BORDER_LIGHT())
                                    .cursor_pointer()
                                    .hover(|this| this.bg(ACTIVE_BG()))
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
                                        .mt_2()
                                        .px_3()
                                        .text_xs()
                                        .text_color(TERTIARY_TEXT())
                                        .line_height(relative(1.5))
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
            .max_w(px(840.0))
            .min_w(px(35.0))
            .w_full()
            .px_6()
            .py_5()
            .rounded_xl()
            .bg(ASSISTANT_BUBBLE_BG())
            .border_1()
            .border_color(BORDER_LIGHT())
            .shadow_sm();

        if waiting {
            content = content.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .px_2()
                            .py_0p5()
                            .rounded_md()
                            .bg(GHOST_SURFACE_BG())
                            .text_xs()
                            .text_color(BRAND_BLUE())
                            .child("LIVE")
                            .with_animation(
                                "live-pulse",
                                Animation::new(Duration::from_secs(2)).repeat(),
                                |el, delta| el.opacity(0.5 + gpui::pulsating_between(0.0, 0.5)(delta))
                            ),
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
            .mb_8()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .mb_1()
                    .child(
                        div()
                            .size(px(26.0))
                            .rounded_full()
                            .bg(AVATAR_BG())
                            .flex()
                            .items_center()
                            .justify_center()
                            .shadow_sm()
                            .child(render_icon_element("assistant", gpui::white(), 13.0)),
                    )
                    .child(
                        div()
                            .text_sm()
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
            .mb_8()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .mb_1()
                    .child(
                        div()
                            .size(px(26.0))
                            .rounded_full()
                            .bg(AVATAR_BG())
                            .flex()
                            .items_center()
                            .justify_center()
                            .shadow_sm()
                            .child(render_icon_element("assistant", gpui::white(), 13.0))
                    )
                    .child(
                        div()
                            .text_sm()
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
                    .max_w(px(840.0))
                    .min_w(px(35.0))
                    .w_full()
                    .px_6()
                    .py_5()
                    .rounded_xl()
                    .bg(ASSISTANT_BUBBLE_BG())
                    .border_1()
                    .border_color(BORDER_LIGHT())
                    .shadow_sm()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .mb_2()
                            .child(
                                div()
                                    .px_2()
                                    .py_0p5()
                                    .rounded_md()
                                    .bg(GHOST_SURFACE_BG())
                                    .text_xs()
                                    .text_color(BRAND_BLUE())
                                    .child("LIVE")
                                    .with_animation(
                                        "claude-live-pulse",
                                        Animation::new(Duration::from_secs(2)).repeat(),
                                        |el, delta| el.opacity(0.5 + gpui::pulsating_between(0.0, 0.5)(delta))
                                    )
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
                                                .line_height(relative(1.6))
                                                .whitespace_normal()
                                                .child(text.clone());
                                            let el = if add_top_padding { el.pt_2() } else { el };
                                            rendered_parts.push(el.into_any_element());
                                        }
                                        ContentPart::ProcessTable { processes } => {
                                            prev_was_think = false;
                                            let el = render_process_table(processes);
                                            rendered_parts.push(el.into_any_element());
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
                                                .child(
                                                    div()
                                                        .flex()
                                                        .items_center()
                                                        .gap_2()
                                                        .px_3()
                                                        .py_1p5()
                                                        .rounded_lg()
                                                        .bg(GHOST_SURFACE_BG())
                                                        .cursor_pointer()
                                                        .hover(|this| this.bg(ACTIVE_BG()))
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
                                                            .mt_2()
                                                            .px_3()
                                                            .text_xs()
                                                            .text_color(TERTIARY_TEXT())
                                                            .line_height(relative(1.5))
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

    fn render_composer(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
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
        let send_bg = BRAND_BLUE();
        let send_label = if request_in_flight {
            t(lang, Translations::STOP_GENERATING)
        } else {
            t(lang, Translations::SEND)
        };

        div()
            .flex()
            .justify_center()
            .pt_4()
            .pb_10()
            .child(
                div()
                    .flex_col()
                    .w_full()
                    .max_w(px(940.0))
                    .gap_3()
                    .px_4()
                    .py_3()
                    .rounded_2xl()
                    .bg(FLOATING_PANEL_BG())
                    .border_1()
                    .border_color(BORDER_LIGHT())
                    .shadow_lg()
                    .child(
                        div()
                            .flex()
                            .items_end()
                            .gap_3()
                            .px_4()
                            .py_3()
                            .rounded_xl()
                            .bg(INPUT_BG())
                            .border_1()
                            .border_color(BORDER_LIGHT())
                            .child(
                                div()
                                    .size(px(32.0))
                                    .rounded_full()
                                    .bg(GHOST_SURFACE_BG())
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .hover(|this| this.bg(ACTIVE_BG()))
                                    .cursor_pointer()
                                    .child(render_icon_element("add", MUTED_TEXT(), 16.0))
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

                                                editor.update(cx, |editor, cx| {
                                                    editor.set_text("", _window, cx);
                                                });

                                                this.route_message(user_message, cx);
                                            }
                                        }
                                    }))
                                    .child(composer_editor)
                            )
                            .child(
                                div()
                                    .size(px(32.0))
                                    .rounded_full()
                                    .bg(GHOST_SURFACE_BG())
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .hover(|this| this.bg(ACTIVE_BG()))
                                    .cursor_pointer()
                                    .child(render_icon_element("mic", MUTED_TEXT(), 16.0))
                            )
                            .child(
                                div()
                                    .px_5()
                                    .py_2()
                                    .rounded_lg()
                                    .bg(send_bg)
                                    .cursor_pointer()
                                    .hover(|this| this.opacity(0.9))
                                    .text_color(gpui::white())
                                    .text_sm()
                                    .font_weight(FontWeight::BOLD)
                                    .on_mouse_down(gpui::MouseButton::Left, cx.listener(move |this, _: &gpui::MouseDownEvent, _window, cx| {
                                        if this.request_in_flight {
                                            // 停止生成：重置请求状态
                                            this.request_in_flight = false;
                                            this.request_status_text = None;
                                            cx.notify();
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

                                                editor.update(cx, |editor, cx| {
                                                    editor.set_text("", _window, cx);
                                                });

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
                            .px_3()
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
                                        self.request_status_text.clone().unwrap_or_else(|| t(lang, Translations::AI_IS_THINKING).to_string())
                                    } else {
                                        format!("{} · Active", self.model_name)
                                    })
                            )
                    )
            )
    }
}
