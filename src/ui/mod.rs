pub mod chat;
pub mod components;
pub mod dialogs;
pub mod nav;
pub mod sidebar;
pub mod subagent;
pub mod terminal;

pub use components::*;

use gpui::{div, prelude::*, px, Context, IntoElement, Window};

use crate::ui_theme::{BORDER_LIGHT, CARD_BG};
use crate::AppState;

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
    }
}
