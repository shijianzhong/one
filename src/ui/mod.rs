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
                    .when(self.sidebar_visible || self.terminal_visible, |this| {
                        let sidebar_visible = self.sidebar_visible;
                        let terminal_visible = self.terminal_visible;
                        let width = self.right_panel_width;
                        
                        this.child(self.render_terminal_resizer(cx))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .w(px(width))
                                    .h_full()
                                    .overflow_hidden()
                                    .when(sidebar_visible && terminal_visible, |this| {
                                        this.child(div().flex_1().child(self.render_sidebar(window, cx)))
                                            .child(div().h(px(1.0)).bg(BORDER_LIGHT()))
                                            .child(div().flex_1().child(self.render_terminal(window, cx)))
                                    })
                                    .when(sidebar_visible && !terminal_visible, |this| {
                                        this.child(self.render_sidebar(window, cx))
                                    })
                                    .when(!sidebar_visible && terminal_visible, |this| {
                                        this.child(self.render_terminal(window, cx))
                                    })
                            )
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
