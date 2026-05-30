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
        let sidebar = if self.sidebar_visible {
            Some(self.render_sidebar(window, cx).into_any_element())
        } else {
            None
        };
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
                    .when_some(sidebar, |this, sidebar| {
                        this.child(div().w(px(1.0)).bg(BORDER_LIGHT()))
                            .child(div().h_full().child(sidebar))
                    })
                    .when(self.terminal_visible, |this| {
                        this.child(self.render_terminal_resizer(cx))
                            .child(self.render_terminal(window, cx))
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
