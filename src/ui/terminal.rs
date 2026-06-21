use gpui::{
    div, prelude::*, px, relative, Context, Hsla, InteractiveElement, IntoElement, KeyDownEvent,
    ParentElement, Render, StatefulInteractiveElement, Styled, Window,
};
use std::sync::Arc;
use std::sync::Mutex;

use crate::i18n::{t, Translations};
use crate::runtime::{
    global_terminal_event_bus, log_runtime_event, supervise_coding_session,
    CodingSessionNotification, CodingSupervisionRequest, RuntimeEvent,
};
use crate::terminal_emulator::mappings::keys::to_esc_str;
use crate::terminal_emulator::TerminalEmulator;
use crate::ui_theme::{
    BORDER_LIGHT, CANVAS_BG, INPUT_BG, MUTED_TEXT, PRIMARY_TEXT, SECONDARY_TEXT, SURFACE_ELEVATED,
    TERTIARY_TEXT,
};
use crate::{AppState, TerminalTab};

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
    pub(crate) fn ensure_terminal_event_subscription(&mut self, cx: &mut Context<Self>) {
        if self.terminal_event_subscription_running {
            return;
        }
        self.terminal_event_subscription_running = true;
        let mut rx = global_terminal_event_bus().subscribe();
        cx.spawn(async move |this, cx| loop {
            let event = match rx.recv().await {
                Ok(event) => event,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            };
            let should_continue = this
                .update(cx, |this, cx| {
                    this.handle_runtime_event(event, cx);
                    true
                })
                .unwrap_or(false);
            if !should_continue {
                break;
            }
        })
        .detach();
    }

    fn handle_runtime_event(&mut self, event: RuntimeEvent, cx: &mut Context<Self>) {
        match event {
            RuntimeEvent::TerminalOutputChanged { terminal_id, seq } => {
                log_runtime_event(
                    "app.runtime_event",
                    format!(
                        "received TerminalOutputChanged terminal_id={} seq={}",
                        terminal_id, seq
                    ),
                );
                if let Some(coding_event) =
                    self.coding_sessions.lock().ok().and_then(|mut sessions| {
                        sessions.handle_terminal_output_changed(&self.db.conn, &terminal_id, seq)
                    })
                {
                    log_runtime_event(
                        "app.runtime_event",
                        format!(
                            "publishing CodingOutputChanged from terminal_id={} seq={}",
                            terminal_id, seq
                        ),
                    );
                    global_terminal_event_bus().publish(coding_event);
                } else {
                    log_runtime_event(
                        "app.runtime_event",
                        format!(
                            "ignored TerminalOutputChanged terminal_id={} seq={} reason=no_active_coding_session",
                            terminal_id, seq
                        ),
                    );
                }
            }
            RuntimeEvent::TerminalExited { terminal_id } => {
                log_runtime_event(
                    "app.runtime_event",
                    format!("received TerminalExited terminal_id={}", terminal_id),
                );
                if let Ok(mut sessions) = self.coding_sessions.lock() {
                    sessions.refresh_session_status(&self.db.conn, &terminal_id);
                }
            }
            RuntimeEvent::CodingOutputChanged {
                session_id, seq, ..
            } => {
                log_runtime_event(
                    "app.runtime_event",
                    format!(
                        "received CodingOutputChanged session_id={} seq={}",
                        session_id, seq
                    ),
                );
                self.schedule_coding_supervision(session_id, seq, cx);
            }
            RuntimeEvent::TerminalTitleChanged { .. }
            | RuntimeEvent::ShellCommandStarted { .. }
            | RuntimeEvent::ShellCommandFinished { .. }
            | RuntimeEvent::ShellCommandFailed { .. } => {}
        }
    }

    fn schedule_coding_supervision(
        &mut self,
        session_id: String,
        seq: u64,
        cx: &mut Context<Self>,
    ) {
        let already_scheduled = self.pending_coding_supervision.contains_key(&session_id);
        self.pending_coding_supervision
            .insert(session_id.clone(), seq);
        log_runtime_event(
            "supervision.schedule",
            format!(
                "session_id={} seq={} already_scheduled={}",
                session_id, seq, already_scheduled
            ),
        );
        if already_scheduled {
            return;
        }
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(600))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this
                    .pending_coding_supervision
                    .remove(&session_id)
                    .is_none()
                {
                    log_runtime_event(
                        "supervision.schedule",
                        format!(
                            "session_id={} fired=false reason=removed_before_timer",
                            session_id
                        ),
                    );
                    return;
                }
                log_runtime_event(
                    "supervision.schedule",
                    format!("session_id={} fired=true", session_id),
                );
                let request = this.coding_sessions.lock().ok().and_then(|mut sessions| {
                    sessions.collect_supervision_request_for_session(
                        &this.db.conn,
                        &session_id,
                        120,
                    )
                });
                if let Some(request) = request {
                    this.spawn_coding_supervision_request(request, cx);
                } else {
                    log_runtime_event(
                        "supervision.schedule",
                        format!("session_id={} no_request_collected", session_id),
                    );
                }
            });
        })
        .detach();
    }

    fn spawn_coding_supervision_request(
        &mut self,
        request: CodingSupervisionRequest,
        cx: &mut Context<Self>,
    ) {
        let base_url = self.model_base_url.clone();
        let api_key = self.model_api_key.clone();
        let model = crate::services::load_config()
            .light_model
            .unwrap_or_else(|| self.model_name.clone());
        log_runtime_event(
            "supervision.spawn",
            format!(
                "session_id={} turn_id={} fingerprint={} model={} transcript_lines={} workspace_delta={}",
                request.session_id,
                request.turn_id,
                request.fingerprint,
                model,
                request.terminal_transcript.len(),
                request.workspace_delta.describe()
            ),
        );
        cx.spawn(async move |this, cx| {
            let request_for_task = request.clone();
            let result = gpui_tokio::Tokio::spawn(cx, async move {
                supervise_coding_session(&base_url, &api_key, &model, &request_for_task).await
            })
            .await
            .unwrap_or_else(|error| Err(format!("supervisor task join failed: {}", error)));
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(decision) => {
                        log_runtime_event(
                            "supervision.result",
                            format!(
                                "session_id={} fingerprint={} state={:?} confidence={} action_kind={} target={} artifacts={} risks={}",
                                request.session_id,
                                request.fingerprint,
                                decision.state,
                                decision.confidence,
                                decision.action_kind,
                                decision.target,
                                decision.artifacts.join("|"),
                                decision.risks.join("|")
                            ),
                        );
                        let notification =
                            this.coding_sessions.lock().ok().and_then(|mut sessions| {
                                sessions.apply_supervision_decision(&request, decision)
                            });
                        if let Some(notification) = notification {
                            append_coding_session_notification(this, notification, cx);
                        }
                    }
                    Err(error) => {
                        log_runtime_event(
                            "supervision.result",
                            format!(
                                "session_id={} fingerprint={} error={}",
                                request.session_id, request.fingerprint, error
                            ),
                        );
                        if let Ok(mut sessions) = this.coding_sessions.lock() {
                            sessions
                                .mark_supervision_failed(&request.session_id, &request.fingerprint);
                        }
                        eprintln!(
                            "[CodingSupervisor] session={} failed: {}",
                            request.session_id, error
                        );
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn ensure_terminal(&mut self, cx: &mut Context<Self>) {
        self.ensure_terminal_event_subscription(cx);
        if let Ok(mut sessions) = self.coding_sessions.lock() {
            sessions.refresh_all(&self.db.conn);
        }
        let project_dir = std::path::PathBuf::from(self.get_work_dir());
        let _ = std::fs::create_dir_all(&project_dir);

        if self.terminal_emulator.is_some() && self.terminal_work_dir.as_ref() == Some(&project_dir)
        {
            self.ensure_terminal_refresh_loop(cx);
            return;
        }

        self.terminal_emulator = None;
        self.terminal_work_dir = None;
        self.terminal_refresh_generation = self.terminal_refresh_generation.wrapping_add(1);
        self.terminal_refresh_running = false;

        let shell = std::env::var("SHELL").unwrap_or_else(|_| "sh".to_string());
        let terminal_id = format!("shell:{}", project_dir.to_string_lossy());
        match TerminalEmulator::new_with_terminal_id(
            terminal_id,
            Some(&shell),
            Some(&project_dir),
            80,
            24,
        ) {
            Ok(term) => {
                let project_dir_str = project_dir.to_string_lossy().to_string();
                self.terminal_emulator = Some(Arc::new(Mutex::new(term)));
                self.terminal_work_dir = Some(project_dir.clone());
                eprintln!(
                    "[Terminal] Terminal emulator initialized, target dir: {}",
                    project_dir_str
                );

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
        cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(50))
                .await;
            let should_continue = this
                .update(cx, |this, cx| {
                    if this.terminal_refresh_generation != generation || !this.terminal_visible {
                        this.terminal_refresh_running = false;
                        return false;
                    }
                    if let Some(ref ta) = this.terminal_emulator {
                        if let Ok(mut t) = ta.lock() {
                            t.process_events();
                        }
                    }
                    if let Ok(mut sessions) = this.coding_sessions.lock() {
                        sessions.refresh_all(&this.db.conn);
                    }
                    let max_scroll_y: f32 = this.terminal_scroll_handle.max_offset().y.into();
                    let current_scroll_y: f32 = this.terminal_scroll_handle.offset().y.into();
                    if max_scroll_y <= 1.0 || -current_scroll_y >= max_scroll_y - 24.0 {
                        this.terminal_scroll_handle.scroll_to_bottom();
                    }
                    cx.notify();
                    true
                })
                .unwrap_or(false);
            if !should_continue {
                break;
            }
        })
        .detach();
    }

    pub(crate) fn render_terminal_resizer(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("terminal-resizer")
            .w(px(6.0))
            .h_full()
            .cursor_col_resize()
            .bg(BORDER_LIGHT().opacity(0.32))
            .border_l_1()
            .border_r_1()
            .border_color(BORDER_LIGHT().opacity(0.55))
            .hover(|this| {
                this.bg(BORDER_LIGHT().opacity(0.9))
                    .border_color(SECONDARY_TEXT().opacity(0.55))
            })
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, event: &gpui::MouseDownEvent, _window, cx| {
                    this.right_panel_resize_initial_mouse_x = Some(f32::from(event.position.x));
                    this.right_panel_resize_initial_width = Some(this.right_panel_width);
                    cx.notify();
                }),
            )
            .on_mouse_up(
                gpui::MouseButton::Left,
                cx.listener(|this, _: &gpui::MouseUpEvent, _window, cx| {
                    this.right_panel_resize_initial_mouse_x = None;
                    this.right_panel_resize_initial_width = None;
                    cx.notify();
                }),
            )
            .on_drag(DraggedResizer, |_, _, _, cx| cx.new(|_| DraggedResizer))
    }

    pub(crate) fn render_terminal(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        self.ensure_terminal(cx);
        let lang = self.current_lang;
        let header = self.terminal_header_text();

        div()
            .id("terminal-view")
            .flex()
            .flex_col()
            .size_full()
            .bg(term_bg())
            .child(self.render_terminal_header(header, lang, cx))
            .child(self.render_terminal_body(window, cx))
    }

    fn terminal_header_text(&self) -> String {
        if self.active_terminal_tab == TerminalTab::Shell {
            return self
                .terminal_work_dir
                .as_ref()
                .map(|path| path.to_string_lossy().to_string())
                .unwrap_or_else(|| self.get_work_dir());
        }
        if let Some(task_id) = self.active_task_id {
            let text = self.coding_sessions.lock().ok().and_then(|sessions| {
                sessions.session_for_task(task_id).map(|session| {
                    format!(
                        "{} · {} · {}",
                        session.agent_kind.label(),
                        session.status.label(),
                        session.cwd.to_string_lossy()
                    )
                })
            });
            if let Some(text) = text {
                return text;
            }
        }
        self.get_work_dir()
    }

    fn render_terminal_header(
        &mut self,
        header_text: String,
        lang: crate::i18n::Lang,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let has_session = self
            .active_task_id
            .and_then(|task_id| {
                self.coding_sessions
                    .lock()
                    .ok()
                    .and_then(|sessions| sessions.attached_session_id_for_task(task_id))
            })
            .is_some();
        let coding_label = self
            .active_task_id
            .and_then(|task_id| {
                self.coding_sessions.lock().ok().and_then(|sessions| {
                    sessions
                        .session_for_task(task_id)
                        .map(|session| session.agent_kind.label().to_string())
                })
            })
            .unwrap_or_else(|| "Coding".to_string());
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
                    .child(div().size(px(8.0)).rounded_full().bg(Hsla {
                        h: 0.33,
                        s: 0.7,
                        l: 0.55,
                        a: 1.0,
                    }))
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
                            .child(header_text),
                    )
                    .child(self.terminal_tab_button(
                        "Shell".to_string(),
                        self.active_terminal_tab == TerminalTab::Shell,
                        cx,
                        |this, cx| {
                            this.active_terminal_tab = TerminalTab::Shell;
                            this.terminal_scroll_handle.scroll_to_bottom();
                            cx.notify();
                        },
                    ))
                    .when(has_session, |this| {
                        this.child(self.terminal_tab_button(
                            coding_label,
                            self.active_terminal_tab == TerminalTab::Coding,
                            cx,
                            |this, cx| {
                                this.active_terminal_tab = TerminalTab::Coding;
                                this.terminal_scroll_handle.scroll_to_bottom();
                                cx.notify();
                            },
                        ))
                    })
                    .when(
                        has_session && self.active_terminal_tab == TerminalTab::Coding,
                        |this| {
                            this.child(self.terminal_header_button(
                                "Stop".to_string(),
                                cx,
                                |this, cx| {
                                    this.stop_persistent_coding_session(None, cx);
                                },
                            ))
                        },
                    ),
            )
    }

    fn terminal_header_button<F>(
        &mut self,
        label: String,
        cx: &mut Context<Self>,
        handler: F,
    ) -> impl IntoElement
    where
        F: Fn(&mut AppState, &mut Context<AppState>) + 'static,
    {
        div()
            .px_2()
            .py_1()
            .rounded_sm()
            .bg(INPUT_BG())
            .border_1()
            .border_color(BORDER_LIGHT())
            .text_xs()
            .text_color(PRIMARY_TEXT())
            .cursor_pointer()
            .hover(|this| this.bg(BORDER_LIGHT().opacity(0.45)))
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(move |this, _: &gpui::MouseDownEvent, _window, cx| {
                    handler(this, cx);
                    cx.notify();
                }),
            )
            .child(label)
    }

    fn terminal_tab_button<F>(
        &mut self,
        label: String,
        active: bool,
        cx: &mut Context<Self>,
        handler: F,
    ) -> impl IntoElement
    where
        F: Fn(&mut AppState, &mut Context<AppState>) + 'static,
    {
        div()
            .px_2()
            .py_1()
            .rounded_sm()
            .bg(if active {
                BORDER_LIGHT().opacity(0.55)
            } else {
                INPUT_BG()
            })
            .border_1()
            .border_color(if active {
                SECONDARY_TEXT().opacity(0.55)
            } else {
                BORDER_LIGHT()
            })
            .text_xs()
            .text_color(if active {
                PRIMARY_TEXT()
            } else {
                SECONDARY_TEXT()
            })
            .cursor_pointer()
            .hover(|this| this.bg(BORDER_LIGHT().opacity(0.45)))
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(move |this, _: &gpui::MouseDownEvent, _window, cx| {
                    handler(this, cx);
                    cx.notify();
                }),
            )
            .child(label)
    }

    fn render_terminal_body(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let session_term = if self.active_terminal_tab == TerminalTab::Coding {
            self.active_task_id.and_then(|task_id| {
                self.coding_sessions.lock().ok().and_then(|sessions| {
                    sessions
                        .session_for_task(task_id)
                        .map(|session| session.terminal.clone())
                })
            })
        } else {
            None
        };
        let has_session_term = session_term.is_some();
        let term_arc = session_term.or_else(|| self.terminal_emulator.as_ref().cloned());
        let has_terminal = term_arc.is_some() || !self.terminal_output.is_empty();
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
        let output_lines = if !has_session_term && !self.terminal_output.is_empty() {
            let mut lines = Vec::new();
            for entry in &self.terminal_output {
                if let Some(command) = &entry.command {
                    lines.push((format!("$ {}", command), false, false));
                }
                for line in entry.output.lines() {
                    lines.push((line.to_string(), false, false));
                }
                if entry.output.ends_with('\n') {
                    lines.push((String::new(), false, false));
                }
            }
            lines
        } else {
            if let Some(term_arc) = &term_arc {
                if let Ok(term_lock) = term_arc.lock() {
                    let render_lines = term_lock.renderable_history_lines();
                    let mut in_think_block = false;
                    let v: Vec<(String, bool, bool)> = render_lines
                        .iter()
                        .map(|line| {
                            let s: String = line.chars.iter().map(|c| c.c).collect();
                            let has_cursor = line.chars.iter().any(|c| c.is_cursor);
                            let trimmed = s.trim_start();
                            let is_think = in_think_block
                                || trimmed.starts_with("<think>")
                                || trimmed.starts_with("</think>");
                            if trimmed.starts_with("<think>") && !trimmed.contains("</think>") {
                                in_think_block = true;
                            }
                            if trimmed.contains("</think>") {
                                in_think_block = false;
                            }
                            (s, has_cursor, is_think)
                        })
                        .collect();
                    v
                } else {
                    Vec::new()
                }
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
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _: &gpui::MouseDownEvent, window, cx| {
                    window.focus(&this.terminal_focus_handle, cx);
                    eprintln!("[Terminal] Focus set");
                }),
            )
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                let term_arc = if this.active_terminal_tab == TerminalTab::Coding {
                    this.active_task_id.and_then(|task_id| {
                        this.coding_sessions.lock().ok().and_then(|sessions| {
                            sessions
                                .session_for_task(task_id)
                                .map(|session| session.terminal.clone())
                        })
                    })
                } else {
                    this.terminal_emulator.as_ref().cloned()
                };
                if let Some(ref term_arc) = term_arc {
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
                    .track_scroll(&self.terminal_scroll_handle)
                    .p_2()
                    .font_family("Menlo")
                    .text_sm()
                    .children(output_lines.iter().map(|(text, has_cursor, is_think)| {
                        let display_text = if *has_cursor && !text.is_empty() {
                            // 在光标位置插入光标符号
                            let cursor_pos = text.trim_end().len();
                            let prefix = &text[..cursor_pos];
                            let suffix = &text[cursor_pos..];
                            format!("{}█{}", prefix, suffix)
                        } else {
                            text.clone()
                        };
                        if has_session_term {
                            div()
                                .h(px(18.0))
                                .flex()
                                .items_center()
                                .text_color(if *is_think {
                                    SECONDARY_TEXT()
                                } else {
                                    PRIMARY_TEXT()
                                })
                                .opacity(if *is_think { 0.55 } else { 1.0 })
                                .child(display_text)
                                .into_any_element()
                        } else {
                            div()
                                .min_h(px(18.0))
                                .w_full()
                                .line_height(relative(1.4))
                                .whitespace_normal()
                                .text_color(PRIMARY_TEXT())
                                .child(display_text)
                                .into_any_element()
                        }
                    })),
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
                    .child(div().text_xs().text_color(SECONDARY_TEXT()).child("$"))
                    .child(
                        div()
                            .flex_1()
                            .text_xs()
                            .text_color(PRIMARY_TEXT())
                            .child("Terminal ready — click and type"),
                    ),
            )
            .into_any_element()
    }
}

fn append_coding_session_notification(
    app: &mut AppState,
    notification: CodingSessionNotification,
    cx: &mut Context<AppState>,
) {
    match notification {
        CodingSessionNotification::UserAction { task_id, message }
        | CodingSessionNotification::Completed { task_id, message }
        | CodingSessionNotification::Failed { task_id, message } => {
            app.append_task_message(Some(task_id), "assistant", &message, cx);
        }
    }
}
