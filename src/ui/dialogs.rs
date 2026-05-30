use editor::Editor;
use gpui::{
    div, px, Context, Focusable, FontWeight, InteractiveElement, IntoElement, ParentElement,
    Styled, Window,
};

use crate::i18n::{t, Translations};
use crate::task_db;
use crate::ui_theme::{
    ACTIVE_BG, BORDER_LIGHT, BRAND_BLUE, CANVAS_BG, PRIMARY_TEXT, SECONDARY_TEXT, SURFACE_PANEL,
};
use crate::{AppState, CancelModelConfig, SaveModelConfig};

impl AppState {
    pub(crate) fn render_model_config_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let lang = self.current_lang;
        let app = &mut *cx;

        let model_name_editor = window.use_keyed_state("model_name_editor", app, |window, cx| {
            let mut editor = Editor::single_line(window, cx);
            editor.set_text(self.editing_model_name.clone(), window, cx);
            editor
        });

        let base_url_editor = window.use_keyed_state("base_url_editor", app, |window, cx| {
            let mut editor = Editor::single_line(window, cx);
            editor.set_text(self.editing_base_url.clone(), window, cx);
            editor
        });

        let api_key_editor = window.use_keyed_state("api_key_editor", app, |window, cx| {
            let mut editor = Editor::single_line(window, cx);
            editor.set_placeholder_text(t(lang, Translations::API_KEY_PLACEHOLDER), window, cx);
            editor.set_text(self.editing_api_key.clone(), window, cx);
            editor
        });

        let model_name_focus = model_name_editor.read(cx).focus_handle(cx);
        let base_url_focus = base_url_editor.read(cx).focus_handle(cx);
        let api_key_focus = api_key_editor.read(cx).focus_handle(cx);

        let weak_model_name = model_name_editor.downgrade();
        let weak_base_url = base_url_editor.downgrade();
        let weak_api_key = api_key_editor.downgrade();

        div()
            .absolute()
            .inset_0()
            .bg(gpui::hsla(0., 0., 0., 0.5))
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _: &gpui::MouseDownEvent, _window, cx| {
                    this.cancel_model_config(&CancelModelConfig, _window, cx);
                }),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .w(px(400.0))
                    .p_5()
                    .bg(SURFACE_PANEL())
                    .rounded_xl()
                    .border_1()
                    .border_color(BORDER_LIGHT())
                    .shadow_md()
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|_, _: &gpui::MouseDownEvent, _window, _cx| {}),
                    )
                    .child(
                        div()
                            .text_base()
                            .text_color(PRIMARY_TEXT())
                            .font_weight(FontWeight::BOLD)
                            .child(t(lang, Translations::MODEL_SERVICE_CONFIG)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(SECONDARY_TEXT())
                                    .child(t(lang, Translations::MODEL_NAME)),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .h(px(36.0))
                                    .px_3()
                                    .rounded_lg()
                                    .border_1()
                                    .border_color(BORDER_LIGHT())
                                    .bg(CANVAS_BG())
                                    .track_focus(&model_name_focus)
                                    .child(model_name_editor.clone()),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(SECONDARY_TEXT())
                                    .child(t(lang, Translations::BASE_URL)),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .h(px(36.0))
                                    .px_3()
                                    .rounded_lg()
                                    .border_1()
                                    .border_color(BORDER_LIGHT())
                                    .bg(CANVAS_BG())
                                    .track_focus(&base_url_focus)
                                    .child(base_url_editor.clone()),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(SECONDARY_TEXT())
                                    .child(t(lang, Translations::API_KEY)),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .h(px(36.0))
                                    .px_3()
                                    .rounded_lg()
                                    .border_1()
                                    .border_color(BORDER_LIGHT())
                                    .bg(CANVAS_BG())
                                    .track_focus(&api_key_focus)
                                    .child(api_key_editor.clone()),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_3()
                            .mt_2()
                            .child(
                                div()
                                    .flex_1()
                                    .h(px(36.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_lg()
                                    .border_1()
                                    .border_color(BORDER_LIGHT())
                                    .bg(CANVAS_BG())
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        gpui::MouseButton::Left,
                                        cx.listener(
                                            |this, _: &gpui::MouseDownEvent, _window, cx| {
                                                this.cancel_model_config(
                                                    &CancelModelConfig,
                                                    _window,
                                                    cx,
                                                );
                                            },
                                        ),
                                    )
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(PRIMARY_TEXT())
                                            .child(t(lang, Translations::CANCEL)),
                                    ),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .h(px(36.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_lg()
                                    .bg(BRAND_BLUE())
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        gpui::MouseButton::Left,
                                        cx.listener(
                                            move |this, _: &gpui::MouseDownEvent, _window, cx| {
                                                if let Some(editor) = weak_model_name.upgrade() {
                                                    this.editing_model_name = editor
                                                        .read_with(cx, |editor, cx| editor.text(cx))
                                                        .trim()
                                                        .to_string();
                                                }
                                                if let Some(editor) = weak_base_url.upgrade() {
                                                    this.editing_base_url = editor
                                                        .read_with(cx, |editor, cx| editor.text(cx))
                                                        .trim()
                                                        .to_string();
                                                }
                                                if let Some(editor) = weak_api_key.upgrade() {
                                                    this.editing_api_key = editor
                                                        .read_with(cx, |editor, cx| editor.text(cx))
                                                        .trim()
                                                        .to_string();
                                                }
                                                this.save_model_config(
                                                    &SaveModelConfig,
                                                    _window,
                                                    cx,
                                                );
                                            },
                                        ),
                                    )
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(gpui::white())
                                            .child(t(lang, Translations::SAVE)),
                                    ),
                            ),
                    ),
            )
    }

    pub(crate) fn render_workspace_popup(
        &mut self,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let lang = self.current_lang;
        let ws_id = self.delete_confirm_workspace_id.unwrap_or(0);
        let pos = self.popup_position;
        div()
            .absolute()
            .left(pos.x)
            .top(pos.y)
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _: &gpui::MouseDownEvent, _window, _cx| {
                    this.delete_confirm_workspace_id = None;
                }),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .w(px(180.0))
                    .p_3()
                    .bg(SURFACE_PANEL())
                    .rounded_xl()
                    .border_1()
                    .border_color(BORDER_LIGHT())
                    .shadow_md()
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|_, _: &gpui::MouseDownEvent, _window, _cx| {}),
                    )
                    .child(
                        div()
                            .px_3()
                            .py_2()
                            .text_sm()
                            .text_color(PRIMARY_TEXT())
                            .rounded_md()
                            .cursor_pointer()
                            .hover(|this| this.bg(ACTIVE_BG()))
                            .on_mouse_down(
                                gpui::MouseButton::Left,
                                cx.listener(move |this, _: &gpui::MouseDownEvent, _window, cx| {
                                    cx.stop_propagation();
                                    this.active_workspace_id = Some(ws_id);
                                    this.active_task_id = this.ensure_workspace_draft_task(ws_id);
                                    this.delete_confirm_workspace_id = None;
                                    this.restore_task_context();
                                    cx.notify();
                                }),
                            )
                            .child(t(lang, Translations::NEW_TASK)),
                    )
                    .child(
                        div()
                            .px_3()
                            .py_2()
                            .text_sm()
                            .text_color(PRIMARY_TEXT())
                            .rounded_md()
                            .cursor_pointer()
                            .hover(|this| this.bg(ACTIVE_BG()))
                            .on_mouse_down(
                                gpui::MouseButton::Left,
                                cx.listener(move |this, _: &gpui::MouseDownEvent, _window, cx| {
                                    cx.stop_propagation();
                                    this.workspaces.retain(|w| w.id != ws_id);
                                    task_db::delete_workspace(&this.db.conn, ws_id).ok();
                                    if this.active_workspace_id == Some(ws_id) {
                                        this.active_workspace_id = None;
                                    }
                                    if this.active_task_id.is_some() {
                                        this.active_task_id = None;
                                    }
                                    this.delete_confirm_workspace_id = None;
                                    cx.notify();
                                }),
                            )
                            .child(t(lang, Translations::DELETE_WORKSPACE)),
                    ),
            )
    }

    pub(crate) fn render_export_dialog(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let lang = self.current_lang;
        let json_content = self.exported_json.clone().unwrap_or_default();
        let md_content = self.exported_md.clone().unwrap_or_default();

        div()
            .absolute()
            .inset_0()
            .bg(gpui::hsla(0., 0., 0., 0.5))
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _: &gpui::MouseDownEvent, _window, cx| {
                    this.show_export_dialog = false;
                    this.exported_json = None;
                    this.exported_md = None;
                    cx.notify();
                }),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .w(px(500.0))
                    .h(px(400.0))
                    .p_5()
                    .bg(SURFACE_PANEL())
                    .rounded_xl()
                    .border_1()
                    .border_color(BORDER_LIGHT())
                    .shadow_md()
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|_, _: &gpui::MouseDownEvent, _window, _cx| {}),
                    )
                    .child(
                        div()
                            .text_base()
                            .text_color(PRIMARY_TEXT())
                            .font_weight(FontWeight::BOLD)
                            .child(t(lang, Translations::EXPORT)),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .h(px(200.0))
                                    .p_3()
                                    .rounded_lg()
                                    .bg(CANVAS_BG())
                                    .border_1()
                                    .border_color(BORDER_LIGHT())
                                    .overflow_hidden()
                                    .text_xs()
                                    .text_color(PRIMARY_TEXT())
                                    .child(format!(
                                        "{}:\n{}",
                                        t(lang, Translations::JSON),
                                        json_content
                                    )),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .h(px(200.0))
                                    .p_3()
                                    .rounded_lg()
                                    .bg(CANVAS_BG())
                                    .border_1()
                                    .border_color(BORDER_LIGHT())
                                    .overflow_hidden()
                                    .text_xs()
                                    .text_color(PRIMARY_TEXT())
                                    .child(format!(
                                        "{}:\n{}",
                                        t(lang, Translations::MARKDOWN),
                                        md_content
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_3()
                            .mt_2()
                            .child(
                                div()
                                    .flex_1()
                                    .h(px(36.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_lg()
                                    .border_1()
                                    .border_color(BORDER_LIGHT())
                                    .bg(CANVAS_BG())
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        gpui::MouseButton::Left,
                                        cx.listener(
                                            |this, _: &gpui::MouseDownEvent, _window, cx| {
                                                if let Some(json) = this.exported_json.clone() {
                                                    if let Some(path) = rfd::FileDialog::new()
                                                        .set_title(t(
                                                            this.current_lang,
                                                            Translations::EXPORT_JSON_TITLE,
                                                        ))
                                                        .add_filter(
                                                            t(
                                                                this.current_lang,
                                                                Translations::JSON,
                                                            ),
                                                            &["json"],
                                                        )
                                                        .save_file()
                                                    {
                                                        std::fs::write(&path, json).ok();
                                                    }
                                                }
                                                this.show_export_dialog = false;
                                                this.exported_json = None;
                                                this.exported_md = None;
                                                cx.notify();
                                            },
                                        ),
                                    )
                                    .child(t(lang, Translations::SAVE_JSON)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .h(px(36.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_lg()
                                    .border_1()
                                    .border_color(BORDER_LIGHT())
                                    .bg(CANVAS_BG())
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        gpui::MouseButton::Left,
                                        cx.listener(
                                            |this, _: &gpui::MouseDownEvent, _window, cx| {
                                                if let Some(md) = this.exported_md.clone() {
                                                    if let Some(path) = rfd::FileDialog::new()
                                                        .set_title(t(
                                                            this.current_lang,
                                                            Translations::EXPORT_MARKDOWN_TITLE,
                                                        ))
                                                        .add_filter(
                                                            t(
                                                                this.current_lang,
                                                                Translations::MARKDOWN,
                                                            ),
                                                            &["md"],
                                                        )
                                                        .save_file()
                                                    {
                                                        std::fs::write(&path, md).ok();
                                                    }
                                                }
                                                this.show_export_dialog = false;
                                                this.exported_json = None;
                                                this.exported_md = None;
                                                cx.notify();
                                            },
                                        ),
                                    )
                                    .child(t(lang, Translations::SAVE_MARKDOWN)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .h(px(36.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_lg()
                                    .bg(BRAND_BLUE())
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        gpui::MouseButton::Left,
                                        cx.listener(
                                            |this, _: &gpui::MouseDownEvent, _window, _cx| {
                                                this.show_export_dialog = false;
                                                this.exported_json = None;
                                                this.exported_md = None;
                                            },
                                        ),
                                    )
                                    .child(t(lang, Translations::CANCEL)),
                            ),
                    ),
            )
    }
}
