pub mod chat;
pub mod components;
pub mod dialogs;
pub mod nav;
pub mod sidebar;
pub mod terminal;

pub use components::*;

use gpui::{div, prelude::*, px, Context, DragMoveEvent, FontWeight, IntoElement, Window};

use crate::ui_theme::{BORDER_LIGHT, CARD_BG, SURFACE_ELEVATED, FLOATING_PANEL_BG, PRIMARY_TEXT, SECONDARY_TEXT, SUCCESS_TEXT, ERROR_TEXT};
use crate::{AppState, ToastInfo, ToastLevel};
use crate::ui::terminal::DraggedResizer;

impl gpui::Render for AppState {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 必须在 when() 外面提前渲染，避免 use_keyed_state 在事件处理阶段调用导致 panic
        let right_panel_visible = self.sidebar_visible || self.terminal_visible;
        let sidebar_visible = self.sidebar_visible;
        let terminal_visible = self.terminal_visible;
        let right_panel_width = self.right_panel_width;

        // 始终提前渲染两个面板，用 Option 包装以支持按分支 move
        let mut terminal_panel: Option<gpui::AnyElement> = Some(
            self.render_terminal(window, cx).into_any_element()
        );
        let mut sidebar_panel: Option<gpui::AnyElement> = Some(
            self.render_sidebar(window, cx).into_any_element()
        );

        div()
            .flex_col()
            .size_full()
            .bg(CARD_BG())
            .child(self.render_window_titlebar(window, cx))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .h_full()
                    .overflow_hidden()
                    .on_drag_move::<DraggedResizer>(cx.listener(|this, event: &DragMoveEvent<DraggedResizer>, _window, cx| {
                        let Some(start_x) = this.right_panel_resize_initial_mouse_x else {
                            return;
                        };
                        let Some(start_width) = this.right_panel_resize_initial_width else {
                            return;
                        };

                        let current_x = f32::from(event.event.position.x);
                        let available_width = f32::from(event.bounds.size.width);
                        let min_width = 280.0;
                        let max_width = (available_width - 320.0).max(min_width);
                        this.right_panel_width = (start_width - (current_x - start_x))
                            .clamp(min_width, max_width);
                        cx.notify();
                    }))
                    .child(self.render_nav(cx))
                    .child(div().w(px(1.0)).bg(BORDER_LIGHT()))
                    .child(
                        div()
                            .flex_1()
                            .h_full()
                            .overflow_hidden()
                            .child(self.render_main_content(window, cx)),
                    )
                    .when(right_panel_visible, |this| {
                        this.child(self.render_terminal_resizer(cx))
                            .child({
                                let mut panel = div()
                                    .flex()
                                    .flex_col()
                                    .w(px(right_panel_width))
                                    .h_full()
                                    .overflow_hidden();
                                if sidebar_visible && terminal_visible {
                                    if let (Some(s), Some(t)) = (sidebar_panel.take(), terminal_panel.take()) {
                                        panel = panel
                                            .child(div().flex_1().child(s))
                                            .child(div().h(px(1.0)).bg(BORDER_LIGHT()))
                                            .child(div().flex_1().child(t));
                                    }
                                } else if sidebar_visible {
                                    if let Some(s) = sidebar_panel.take() {
                                        panel = panel.child(s);
                                    }
                                } else if terminal_visible {
                                    if let Some(t) = terminal_panel.take() {
                                        panel = panel.child(t);
                                    }
                                }
                                panel
                            })
                    }),
            )
            .when(self.show_model_config_dialog, |this| {
                this.child(self.render_model_config_dialog(window, cx))
            })
            .when(self.show_export_dialog, |this| {
                this.child(self.render_export_dialog(cx))
            })
            .when(self.delete_confirm_workspace_id.is_some(), |this| {
                this.child(self.render_workspace_popup(cx))
            })
            .when(self.pending_approval.is_some(), |this| {
                this.child(self.render_approval_dialog(cx))
            })
            .when(self.pending_soul_proposal.is_some(), |this| {
                this.child(self.render_soul_proposal_dialog(cx))
            })
            .when(self.skill_card.is_some(), |this| {
                this.child(self.render_skill_card_dialog(cx))
            })
            .when(self.show_cipher_dialog, |this| {
                this.child(self.render_cipher_dialog(window, cx))
            })
            .when(!self.toasts.is_empty(), |this| {
                this.child(self.render_toast_overlay(cx))
            })
    }
}

impl AppState {
    fn render_toast_overlay(&self, _cx: &mut Context<Self>) -> impl IntoElement {
        let max_show = 5usize;
        let toasts: Vec<&ToastInfo> = self.toasts.iter().rev().take(max_show).collect();

        div()
            .absolute()
            .bottom(px(20.0))
            .right(px(20.0))
            .flex_col()
            .gap_2()
            .children(toasts.into_iter().map(|toast| {
                let color = match toast.level {
                    ToastLevel::Success => SUCCESS_TEXT(),
                    ToastLevel::Error => ERROR_TEXT(),
                    ToastLevel::Warning => SECONDARY_TEXT(),
                    ToastLevel::Info => SECONDARY_TEXT(),
                };
                div()
                    .px_4()
                    .py_3()
                    .rounded_lg()
                    .bg(FLOATING_PANEL_BG())
                    .border_1()
                    .border_color(BORDER_LIGHT())
                    .shadow_lg()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_color(color)
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child(toast.message.clone()),
                    )
            }))
    }
}
