use gpui::{
    div, prelude::*, px, svg, AnyElement, Context, FontWeight, InteractiveElement, IntoElement,
    ParentElement, StatefulInteractiveElement, Styled, Window,
};
use std::collections::HashMap;

use crate::i18n::{t, Lang, Translations};
use crate::task_db;
use crate::ui::render_icon_element;
use crate::ui_theme::{
    get_theme_mode, ThemeMode, BORDER_LIGHT, BRAND_BLUE, GHOST_SURFACE_BG, HEADER_BG, HOVER_BG,
    MUTED_TEXT, NAV_BG, PRIMARY_TEXT, SECONDARY_TEXT, TERTIARY_TEXT,
};
use crate::workspace::TaskItem;
use crate::{
    skills_market, AppState, MainView, OpenCipherDialog, OpenModelConfigDialog, ToggleLang,
    ToggleTheme, NAV_WIDTH, TITLEBAR_HEIGHT,
};

impl AppState {
    pub(crate) fn render_task_list(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        self.ensure_default_workspace();

        let workspaces = self.workspaces.clone();
        let active_workspace_id = self.active_workspace_id;
        let active_task_id = self.active_task_id;

        let mut result = div().flex().flex_col().px_4().pb_3().gap_3().on_mouse_down(
            gpui::MouseButton::Left,
            cx.listener(move |this, _: &gpui::MouseDownEvent, _window, _cx| {
                this.delete_confirm_workspace_id = None;
            }),
        );

        for workspace in workspaces {
            let is_active_ws = active_workspace_id == Some(workspace.id);
            let ws_id = workspace.id;

            let ws_row = div()
                .flex()
                .items_center()
                .gap_2()
                .px_2()
                .py_1()
                .rounded_md()
                .cursor_pointer()
                .hover(|this| this.bg(HOVER_BG()))
                .on_mouse_move(
                    cx.listener(move |this, _: &gpui::MouseMoveEvent, _window, _cx| {
                        this.hovered_workspace_id = Some(ws_id);
                    }),
                )
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(move |this, _: &gpui::MouseDownEvent, _window, _cx| {
                        this.active_workspace_id = Some(ws_id);
                        if let Some(ws) = this.workspaces.iter_mut().find(|w| w.id == ws_id) {
                            ws.expanded = !ws.expanded;
                            task_db::update_workspace_expanded(&this.db.conn, ws_id, ws.expanded)
                                .ok();
                        }
                    }),
                );

            let expand_btn = div().size(px(16.0)).flex().items_center().justify_center();

            let add_btn = div()
                .text_sm()
                .text_color(MUTED_TEXT())
                .px_1()
                .py_1()
                .opacity(0.72)
                .cursor_pointer()
                .id(format!("add-btn-{}", ws_id))
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(move |this, _: &gpui::MouseDownEvent, _window, cx| {
                        cx.stop_propagation();
                        this.active_workspace_id = Some(ws_id);
                        this.active_task_id = this.ensure_workspace_draft_task(ws_id);
                        this.restore_task_context();
                        cx.notify();
                    }),
                );

            let ws_label = workspace.name.clone();

            let more_btn = div()
                .id(format!("more-btn-{}", ws_id))
                .px_1()
                .py_1()
                .opacity(0.72)
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(
                        move |this,
                              event: &gpui::MouseDownEvent,
                              _window: &mut Window,
                              cx: &mut Context<Self>| {
                            cx.stop_propagation();
                            this.delete_confirm_workspace_id = Some(ws_id);
                            this.popup_position = event.position;
                        },
                    ),
                )
                .child(
                    svg()
                        .path("more.svg")
                        .size(px(16.0))
                        .flex_none()
                        .text_color(MUTED_TEXT()),
                );

            let action_div = div()
                .ml_auto()
                .flex()
                .items_center()
                .gap_2()
                .child(more_btn)
                .child(add_btn.child("+"));

            result = result.child(
                ws_row
                    .child(
                        svg()
                            .path("folder.svg")
                            .size(px(16.0))
                            .flex_none()
                            .text_color(if is_active_ws {
                                BRAND_BLUE()
                            } else {
                                SECONDARY_TEXT()
                            }),
                    )
                    .child(if is_active_ws {
                        div()
                            .text_sm()
                            .ml_1()
                            .text_color(PRIMARY_TEXT())
                            .font_weight(FontWeight::BOLD)
                            .child(ws_label)
                    } else {
                        div()
                            .text_sm()
                            .ml_1()
                            .text_color(SECONDARY_TEXT())
                            .child(ws_label)
                    })
                    .child(if workspace.expanded {
                        expand_btn.child(
                            svg()
                                .path("expand.svg")
                                .size(px(16.0))
                                .flex_none()
                                .text_color(MUTED_TEXT()),
                        )
                    } else {
                        expand_btn.child(
                            svg()
                                .path("fold.svg")
                                .size(px(16.0))
                                .flex_none()
                                .text_color(MUTED_TEXT()),
                        )
                    })
                    .child(
                        div()
                            .ml_auto()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(action_div),
                    ),
            );

            if workspace.expanded {
                let mut tasks_container = div()
                    .flex_col()
                    .ml_4()
                    .pl_3()
                    .border_l_1()
                    .border_color(GHOST_SURFACE_BG())
                    .gap_1();

                for task in &workspace.tasks {
                    let is_active_task = active_task_id == Some(task.id)
                        && active_workspace_id == Some(workspace.id);

                    let mut task_div = div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .w_full()
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .cursor_pointer()
                        .bg(if is_active_task {
                            GHOST_SURFACE_BG()
                        } else {
                            NAV_BG()
                        })
                        .hover(|this| this.bg(HOVER_BG()));

                    let task_id = task.id;
                    let ws_id = workspace.id;
                    let lang = self.current_lang;
                    let title_display = if task.title.trim().is_empty() {
                        t(lang, Translations::NEW_TASK).to_string()
                    } else {
                        task.title.trim().to_string()
                    };

                    task_div = task_div.on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(move |this, _: &gpui::MouseDownEvent, _window, cx| {
                            this.active_workspace_id = Some(ws_id);
                            this.active_task_id = Some(task_id);
                            this.restore_task_context();
                            this.main_view = MainView::Chat;
                            cx.notify();
                        }),
                    );

                    tasks_container = tasks_container.child(
                        task_div
                            .child(div().w(px(2.0)).h(px(18.0)).rounded_full().bg(
                                if is_active_task {
                                    BRAND_BLUE()
                                } else {
                                    BORDER_LIGHT()
                                },
                            ))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(if is_active_task {
                                        TERTIARY_TEXT()
                                    } else {
                                        MUTED_TEXT()
                                    })
                                    .child(""),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .overflow_hidden()
                                    .text_sm()
                                    .text_color(if is_active_task {
                                        PRIMARY_TEXT()
                                    } else {
                                        SECONDARY_TEXT()
                                    })
                                    .text_ellipsis()
                                    .child(title_display.clone()),
                            )
                            .child(
                                div()
                                    .ml_auto()
                                    .text_xs()
                                    .text_color(MUTED_TEXT())
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        gpui::MouseButton::Left,
                                        cx.listener(
                                            move |this, _: &gpui::MouseDownEvent, _window, cx| {
                                                cx.stop_propagation();

                                                // ── P1: 运行时禁止删除 ────────────────
                                                if this.is_task_active(Some(task_id)) {
                                                    this.push_toast(
                                                        crate::app_state::ToastLevel::Warning,
                                                        "⚠️ 任务正在运行，无法删除。请等待任务完成后重试。".to_string(),
                                                        cx,
                                                    );
                                                    return;
                                                }

                                                let (was_draft, was_active, task_title) = this
                                                    .workspaces
                                                    .iter()
                                                    .find(|w| w.id == ws_id)
                                                    .and_then(|w| w.tasks.iter().find(|t| t.id == task_id))
                                                    .map(|t| {
                                                        (
                                                            t.is_draft,
                                                            this.active_task_id == Some(task_id),
                                                            t.title.clone(),
                                                        )
                                                    })
                                                    .unwrap_or_default();

                                                // ── 内存移除 ────────────────────────
                                                if let Some(ws) = this
                                                    .workspaces
                                                    .iter_mut()
                                                    .find(|w| w.id == ws_id)
                                                {
                                                    ws.tasks.retain(|t| t.id != task_id);
                                                }

                                                // ── 数据库删除 + 级联清理 (P0) ──
                                                let delete_result =
                                                    task_db::delete_task(&this.db.conn, task_id);

                                                match delete_result {
                                                    Ok(()) => {
                                                        // ── P2: 清理文件系统目录 ────
                                                        if !task_title.is_empty() {
                                                            let task_dir = this.get_task_dir_for_ids(
                                                                ws_id, task_id, &task_title,
                                                            );
                                                            if task_dir.exists() {
                                                                let _ = std::fs::remove_dir_all(&task_dir);
                                                            }
                                                        }

                                                        // ── P2: 清理 JobManager 状态 ─
                                                        if this.job_manager.general_ai_task_id == Some(task_id) {
                                                            this.job_manager.reset_general_ai_run();
                                                        }

                                                        this.task_active_states.remove(&task_id);

                                                        // ── 删除成功提示 ────────────
                                                        this.push_toast(
                                                            crate::app_state::ToastLevel::Success,
                                                            format!(
                                                                "✅ 任务已删除: {} (id={})",
                                                                task_title, task_id,
                                                            ),
                                                            cx,
                                                        );
                                                    }
                                                    Err(e) => {
                                                        // ── P3: 删除失败 → 回滚 UI + 提示 ─
                                                        eprintln!(
                                                            "Failed to delete task {}: {}",
                                                            task_id, e
                                                        );

                                                        // 回滚内存状态
                                                        if let Ok(rows) = task_db::load_tasks(
                                                            &this.db.conn,
                                                            ws_id,
                                                        ) {
                                                            if let Some(ws) = this
                                                                .workspaces
                                                                .iter_mut()
                                                                .find(|w| w.id == ws_id)
                                                            {
                                                                ws.tasks = rows
                                                                    .into_iter()
                                                                    .map(|t| TaskItem {
                                                                        id: t.id,
                                                                        title: t.title,
                                                                        is_draft: t.is_draft,
                                                                        messages: vec![],
                                                                        pending_summarize: false,
                                                                        needs_auto_scroll: false,
                                                                        think_collapsed: HashMap::new(),
                                                                    })
                                                                    .collect();
                                                            }
                                                        }

                                                        this.push_toast(
                                                            crate::app_state::ToastLevel::Error,
                                                            format!(
                                                                "❌ 删除任务失败 (id={}): {}",
                                                                task_id, e,
                                                            ),
                                                            cx,
                                                        );
                                                    }
                                                }

                                                // ── 删除后切换 active task ─────────
                                                if was_active {
                                                    this.active_task_id = if let Some(ws) = this
                                                        .workspaces
                                                        .iter()
                                                        .find(|w| w.id == ws_id)
                                                    {
                                                        ws.tasks
                                                            .iter()
                                                            .find(|t| t.is_draft)
                                                            .map(|t| t.id)
                                                            .or_else(|| ws.tasks.first().map(|t| t.id))
                                                    } else {
                                                        None
                                                    };
                                                    this.restore_task_context();
                                                }
                                                cx.notify();
                                            },
                                        ),
                                    )
                                    .child("×"),
                            ),
                    );
                }

                result = result.child(tasks_container);
            }
        }

        result
    }

    pub(crate) fn render_main_content(
        &mut self,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> AnyElement {
        match self.main_view {
            MainView::Chat => self.render_chat(window, _cx).into_any_element(),
            MainView::SkillsMarket => skills_market::render_skills_market(&*self, window, _cx),
        }
    }

    pub(crate) fn render_nav(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let lang = self.current_lang;
        let workspaces_heading = div()
            .px_4()
            .pt_4()
            .pb_1()
            .text_xs()
            .text_color(MUTED_TEXT())
            .font_weight(FontWeight::BOLD)
            .child(t(lang, Translations::WORKSPACES_HEADING))
            .into_element();

        div()
            .flex()
            .flex_col()
            .w(px(NAV_WIDTH))
            .h_full()
            .bg(NAV_BG())
            .child(div().flex_none().child(self.render_nav_buttons(cx)))
            .child(div().flex_none().child(workspaces_heading).into_element())
            .child(
                div()
                    .id("task-list-container")
                    .flex_1()
                    .overflow_y_scroll()
                    .child(self.render_task_list(cx)),
            )
            .child(div().flex_none().h(px(1.0)).bg(BORDER_LIGHT()))
            .child(div().flex_none().child(self.render_nav_footer_actions(cx)))
    }

    pub(crate) fn render_titlebar_leading(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let lang = self.current_lang;
        let theme_mode = get_theme_mode();
        let theme_label = match (lang, theme_mode) {
            (Lang::Zh, ThemeMode::Dark) => "深色",
            (Lang::Zh, ThemeMode::Light) => "浅色",
            (Lang::En, ThemeMode::Dark) => "Dark",
            (Lang::En, ThemeMode::Light) => "Light",
        };
        div()
            .flex()
            .items_center()
            .justify_between()
            .h_full()
            .pl(px(crate::util::titlebar_leading_inset()))
            .pr_4()
            .child(
                div().flex().items_center().gap_3().child(
                    div()
                        .text_size(px(20.0))
                        .text_color(PRIMARY_TEXT())
                        .font_weight(FontWeight::BOLD)
                        .child(t(lang, Translations::NAV_ONE)),
                ),
            )
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
                            .border_1()
                            .border_color(BORDER_LIGHT())
                            .text_xs()
                            .text_color(SECONDARY_TEXT())
                            .cursor_pointer()
                            .on_mouse_down(
                                gpui::MouseButton::Left,
                                cx.listener(|this, _: &gpui::MouseDownEvent, _window, cx| {
                                    this.toggle_theme(&ToggleTheme, _window, cx);
                                }),
                            )
                            .child(theme_label),
                    )
                    .child(
                        div()
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .bg(GHOST_SURFACE_BG())
                            .border_1()
                            .border_color(BORDER_LIGHT())
                            .text_xs()
                            .text_color(SECONDARY_TEXT())
                            .cursor_pointer()
                            .on_mouse_down(
                                gpui::MouseButton::Left,
                                cx.listener(|this, _: &gpui::MouseDownEvent, _window, cx| {
                                    this.toggle_lang(&ToggleLang, _window, cx);
                                }),
                            )
                            .child(lang.label()),
                    ),
            )
    }

    pub(crate) fn render_window_titlebar(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let header_content = match self.main_view {
            MainView::Chat => {
                let lang = self.current_lang;
                let title = if let Some(task) = self.get_active_task() {
                    if task.title.trim().is_empty() {
                        t(lang, Translations::NEW_TASK).to_string()
                    } else {
                        task.title.clone()
                    }
                } else {
                    t(lang, Translations::NO_TASK_SELECTED).to_string()
                };
                self.render_chat_header(
                    title,
                    self.get_work_dir(),
                    self.sidebar_visible,
                    self.terminal_visible,
                    cx,
                )
                .into_any_element()
            }
            MainView::SkillsMarket => {
                skills_market::render_skills_market_titlebar(&*self, window, cx)
            }
        };

        div()
            .flex()
            .flex_none()
            .h(px(TITLEBAR_HEIGHT))
            .border_b_1()
            .border_color(BORDER_LIGHT())
            .on_mouse_down_out(cx.listener(|this, _ev, _window, _cx| {
                this.titlebar_should_move = false;
            }))
            .on_mouse_up(
                gpui::MouseButton::Left,
                cx.listener(|this, _ev, _window, _cx| {
                    this.titlebar_should_move = false;
                }),
            )
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _ev, _window, _cx| {
                    this.titlebar_should_move = true;
                }),
            )
            .on_mouse_move(cx.listener(|this, _ev, window, _cx| {
                if this.titlebar_should_move {
                    this.titlebar_should_move = false;
                    window.start_window_move();
                }
            }))
            .child(
                div()
                    .w(px(NAV_WIDTH))
                    .h_full()
                    .bg(NAV_BG())
                    .child(self.render_titlebar_leading(cx)),
            )
            .child(div().w(px(1.0)).bg(BORDER_LIGHT()))
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .bg(HEADER_BG())
                    .child(header_content),
            )
    }

    fn render_nav_buttons(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let lang = self.current_lang;
        let skills_active = matches!(self.main_view, MainView::SkillsMarket);
        let models_active = self.show_model_config_dialog;
        let mut nav = div().flex().flex_col().gap_1().px_4().py_3();

        nav = nav.child(self.make_nav_item(
            t(lang, Translations::NEW_WORKSPACE).to_string(),
            "⌘N".to_string(),
            "workspace",
            false,
            cx,
        ));
        nav = nav.child(self.make_nav_item(
            t(lang, Translations::CAPABILITIES).to_string(),
            "⌘K".to_string(),
            "capabilities",
            skills_active,
            cx,
        ));
        nav = nav.child(self.make_nav_item(
            t(lang, Translations::MODELS).to_string(),
            "⌘M".to_string(),
            "models",
            models_active,
            cx,
        ));
        nav = nav.child(self.make_cipher_nav_item(cx));

        nav
    }

    fn make_nav_item(
        &mut self,
        title: String,
        shortcut: String,
        icon_key: &'static str,
        active: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let is_new_workspace = title == t(self.current_lang, Translations::NEW_WORKSPACE);
        let is_skills = title == t(self.current_lang, Translations::CAPABILITIES);
        let is_model_config = title == t(self.current_lang, Translations::MODELS);

        div()
            .flex()
            .items_center()
            .gap_3()
            .px_1()
            .py_1()
            .cursor_pointer()
            .hover(|this| this.opacity(0.92))
            .when(is_new_workspace, |this| {
                this.on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(|this, _: &gpui::MouseDownEvent, _window, cx| {
                        this.handle_new_workspace_click(cx);
                    }),
                )
            })
            .when(is_skills, |this| {
                this.on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(|this, _: &gpui::MouseDownEvent, _window, cx| {
                        this.open_skills_market(cx);
                    }),
                )
            })
            .when(is_model_config, |this| {
                this.on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(|this, _: &gpui::MouseDownEvent, _window, cx| {
                        this.open_model_config_dialog(&OpenModelConfigDialog, _window, cx);
                    }),
                )
            })
            .child(self.make_icon_slot(icon_key, active))
            .child(
                div()
                    .flex_1()
                    .overflow_hidden()
                    .text_sm()
                    .text_color(if active {
                        PRIMARY_TEXT()
                    } else {
                        SECONDARY_TEXT()
                    })
                    .font_weight(FontWeight::BOLD)
                    .text_ellipsis()
                    .child(title),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(if active {
                        TERTIARY_TEXT()
                    } else {
                        MUTED_TEXT()
                    })
                    .child(shortcut),
            )
    }

    fn make_footer_action_item(
        &mut self,
        title: String,
        icon_key: &'static str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let lang = self.current_lang;
        let tip = t(lang, Translations::FEATURE_IN_PROGRESS).to_string();
        div()
            .flex()
            .items_center()
            .gap_3()
            .px_1()
            .py_1()
            .rounded_md()
            .opacity(0.88)
            .cursor_pointer()
            .id(format!("nav-footer-{}", icon_key))
            .hover(|this| this.opacity(1.0).bg(HOVER_BG()))
            .tooltip(move |_, cx| {
                cx.new(|_| crate::HeaderTooltip { text: tip.clone() })
                    .into()
            })
            .child(self.make_icon_slot(icon_key, false))
            .child(
                div()
                    .flex_1()
                    .text_sm()
                    .text_color(SECONDARY_TEXT())
                    .child(title),
            )
    }

    fn render_nav_footer_actions(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let lang = self.current_lang;
        div()
            .flex()
            .flex_col()
            .gap_1()
            .px_4()
            .py_3()
            .child(self.make_footer_action_item(
                t(lang, Translations::SETTINGS).to_string(),
                "settings",
                cx,
            ))
            .child(self.make_footer_action_item(
                t(lang, Translations::SUPPORT).to_string(),
                "support",
                cx,
            ))
            .child(div().h(px(40.0)))
    }

    fn make_cipher_nav_item(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .gap_3()
            .px_1()
            .py_1()
            .cursor_pointer()
            .hover(|this| this.opacity(0.92))
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _: &gpui::MouseDownEvent, _window, cx| {
                    this.open_cipher_dialog(&OpenCipherDialog, _window, cx);
                }),
            )
            .child(self.make_icon_slot("lock", false))
            .child(
                div()
                    .flex_1()
                    .overflow_hidden()
                    .text_sm()
                    .text_color(SECONDARY_TEXT())
                    .font_weight(FontWeight::BOLD)
                    .text_ellipsis()
                    .child("暗号"),
            )
            .child(div().w(px(12.0)).text_xs().text_color(MUTED_TEXT()))
    }

    fn make_icon_slot(&mut self, icon_key: &'static str, active: bool) -> impl IntoElement {
        div()
            .w(px(16.0))
            .h(px(16.0))
            .flex()
            .items_center()
            .justify_center()
            .child(render_icon_element(
                icon_key,
                if active {
                    PRIMARY_TEXT()
                } else {
                    SECONDARY_TEXT()
                },
                14.0,
            ))
    }
}
