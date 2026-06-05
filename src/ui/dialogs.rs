use editor::Editor;
use gpui::{
    div, prelude::FluentBuilder, px, Context, Focusable, FontWeight, InteractiveElement,
    IntoElement, ParentElement, StatefulInteractiveElement, Styled, Window,
};

use crate::i18n::{t, Translations};
use crate::task_db;
use crate::ui_theme::{
    ACTIVE_BG, BORDER_LIGHT, BRAND_BLUE, CANVAS_BG, PRIMARY_TEXT, SECONDARY_TEXT, SURFACE_PANEL,
};
use crate::{AppState, CancelModelConfig, OpenCipherDialog, SaveModelConfig};

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
        // 防止菜单溢出右/下边界（菜单宽 180px，高约 96px）
        let safe_x = if pos.x.as_f32() + 180.0 > 1400.0 {
            gpui::px(pos.x.as_f32() - 180.0)
        } else {
            pos.x
        };
        let safe_y = if pos.y.as_f32() + 100.0 > 900.0 {
            gpui::px(pos.y.as_f32() - 100.0)
        } else {
            pos.y
        };
        div()
            .absolute()
            .left(safe_x)
            .top(safe_y)
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
                                    .id("export-json-preview")
                                    .overflow_y_scroll()
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
                                    .id("export-md-preview")
                                    .overflow_y_scroll()
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

    pub(crate) fn render_approval_dialog(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(req) = self.pending_approval.as_ref() else {
            return div().into_any_element();
        };
        let kind_label = req.kind.label().to_string();
        let detail = req.detail.clone();
        let queue_more = crate::agents::permission::pending_count();

        div()
            .absolute()
            .inset_0()
            .bg(gpui::hsla(0., 0., 0., 0.55))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .w(px(460.0))
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
                            .child(format!("需要授权：{}", kind_label)),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(SECONDARY_TEXT())
                            .child("AI 正请求执行下面的操作。请确认是否允许。"),
                    )
                    .child(
                        div()
                            .max_h(px(220.0))
                            .p_3()
                            .rounded_lg()
                            .bg(CANVAS_BG())
                            .border_1()
                            .border_color(BORDER_LIGHT())
                            .id("approval-detail")
                            .overflow_y_scroll()
                            .text_xs()
                            .text_color(PRIMARY_TEXT())
                            .child(detail),
                    )
                    .when(queue_more > 0, |this| {
                        this.child(
                            div()
                                .text_xs()
                                .text_color(SECONDARY_TEXT())
                                .child(format!("队列中还有 {} 个待审批请求", queue_more)),
                        )
                    })
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
                                                this.deny_pending_permission(cx);
                                            },
                                        ),
                                    )
                                    .child("拒绝"),
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
                                            |this, _: &gpui::MouseDownEvent, _window, cx| {
                                                this.approve_pending_permission(cx);
                                            },
                                        ),
                                    )
                                    .child(
                                        div()
                                            .text_color(gpui::white())
                                            .child("允许执行"),
                                    ),
                            ),
                    ),
            )
            .into_any_element()
    }

    pub(crate) fn render_soul_proposal_dialog(
        &mut self,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let Some(prop) = self.pending_soul_proposal.as_ref() else {
            return div().into_any_element();
        };
        let rationale = prop.rationale.clone();
        let new_content = prop.new_content.clone();
        let previous_content = prop.previous_content.clone();
        let queue_more = crate::agents::soul::pending_count();

        div()
            .absolute()
            .inset_0()
            .bg(gpui::hsla(0., 0., 0., 0.55))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .w(px(640.0))
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
                            .child("人格草案待审核（soul.md）"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(SECONDARY_TEXT())
                            .child(
                                "AI 提议改写它自己的人格设定。务必先阅读改动内容，确认无误再批准。",
                            ),
                    )
                    .when(!rationale.is_empty(), |this| {
                        this.child(
                            div()
                                .p_3()
                                .rounded_lg()
                                .bg(CANVAS_BG())
                                .border_1()
                                .border_color(BORDER_LIGHT())
                                .text_xs()
                                .text_color(PRIMARY_TEXT())
                                .child(format!("理由：{}", rationale)),
                        )
                    })
                    .child(
                        div()
                            .flex()
                            .gap_3()
                            .child(
                                div()
                                    .flex_1()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(SECONDARY_TEXT())
                                            .child("当前 soul.md"),
                                    )
                                    .child(
                                        div()
                                            .h(px(260.0))
                                            .p_3()
                                            .rounded_lg()
                                            .bg(CANVAS_BG())
                                            .border_1()
                                            .border_color(BORDER_LIGHT())
                                            .id("soul-prev")
                                            .overflow_y_scroll()
                                            .text_xs()
                                            .text_color(PRIMARY_TEXT())
                                            .child(previous_content),
                                    ),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(SECONDARY_TEXT())
                                            .child("草案内容"),
                                    )
                                    .child(
                                        div()
                                            .h(px(260.0))
                                            .p_3()
                                            .rounded_lg()
                                            .bg(CANVAS_BG())
                                            .border_1()
                                            .border_color(BORDER_LIGHT())
                                            .id("soul-next")
                                            .overflow_y_scroll()
                                            .text_xs()
                                            .text_color(PRIMARY_TEXT())
                                            .child(new_content),
                                    ),
                            ),
                    )
                    .when(queue_more > 0, |this| {
                        this.child(
                            div()
                                .text_xs()
                                .text_color(SECONDARY_TEXT())
                                .child(format!("队列中还有 {} 份草案待审核", queue_more)),
                        )
                    })
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
                                                this.deny_soul_proposal(cx);
                                            },
                                        ),
                                    )
                                    .child("拒绝"),
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
                                            |this, _: &gpui::MouseDownEvent, _window, cx| {
                                                this.approve_soul_proposal(cx);
                                            },
                                        ),
                                    )
                                    .child(
                                        div()
                                            .text_color(gpui::white())
                                            .child("应用草案"),
                                    ),
                            ),
                    ),
            )
            .into_any_element()
    }

    pub(crate) fn render_skill_card_dialog(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(card) = self.skill_card.as_ref() else {
            return div().into_any_element();
        };
        let manifest = card.manifest.clone();
        let stage = card.stage.clone();

        let mut body = div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .text_base()
                    .text_color(PRIMARY_TEXT())
                    .font_weight(FontWeight::BOLD)
                    .child(format!(
                        "Skill · {} ({})",
                        manifest.name,
                        manifest.category.label()
                    )),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(SECONDARY_TEXT())
                    .child(manifest.description.clone()),
            );

        match stage.clone() {
            crate::app_state::SkillCardStage::Previewing => {
                body = body.child(
                    div()
                        .text_xs()
                        .text_color(SECONDARY_TEXT())
                        .child("正在扫描可清理项..."),
                );
            }
            crate::app_state::SkillCardStage::PreviewReady(preview) => {
                body = body.child(
                    div()
                        .text_xs()
                        .text_color(PRIMARY_TEXT())
                        .child(preview.summary.clone()),
                );
                if !preview.items.is_empty() {
                    let mut list = div()
                        .max_h(px(220.0))
                        .p_3()
                        .rounded_lg()
                        .bg(CANVAS_BG())
                        .border_1()
                        .border_color(BORDER_LIGHT())
                        .id("skill-preview-list")
                        .overflow_y_scroll()
                        .flex()
                        .flex_col()
                        .gap_2();
                    for item in preview.items.iter() {
                        list = list.child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .text_xs()
                                .text_color(PRIMARY_TEXT())
                                .child(format!("• {} · {}", item.label, human_bytes(item.bytes)))
                                .child(
                                    div()
                                        .text_color(SECONDARY_TEXT())
                                        .child(item.detail.clone()),
                                ),
                        );
                    }
                    body = body.child(list);
                }
                for w in preview.warnings.iter() {
                    body = body.child(
                        div()
                            .text_xs()
                            .text_color(SECONDARY_TEXT())
                            .child(format!("⚠️ {}", w)),
                    );
                }
            }
            crate::app_state::SkillCardStage::Executing => {
                body = body.child(
                    div()
                        .text_xs()
                        .text_color(SECONDARY_TEXT())
                        .child("Skill 正在执行... 如果是高风险操作，会先弹窗征求授权。"),
                );
            }
            crate::app_state::SkillCardStage::Done(exec) => {
                body = body.child(
                    div()
                        .text_xs()
                        .text_color(PRIMARY_TEXT())
                        .child(exec.summary.clone()),
                );
                if !exec.success_items.is_empty() {
                    let mut list = div()
                        .max_h(px(160.0))
                        .p_3()
                        .rounded_lg()
                        .bg(CANVAS_BG())
                        .border_1()
                        .border_color(BORDER_LIGHT())
                        .id("skill-result-list")
                        .overflow_y_scroll()
                        .flex()
                        .flex_col()
                        .gap_1();
                    for ok in exec.success_items.iter() {
                        list = list.child(
                            div()
                                .text_xs()
                                .text_color(PRIMARY_TEXT())
                                .child(format!("✅ {}", ok)),
                        );
                    }
                    for (label, err) in exec.failed_items.iter() {
                        list = list.child(
                            div()
                                .text_xs()
                                .text_color(SECONDARY_TEXT())
                                .child(format!("❌ {} — {}", label, err)),
                        );
                    }
                    body = body.child(list);
                }
            }
            crate::app_state::SkillCardStage::Failed(err) => {
                body = body.child(
                    div()
                        .text_xs()
                        .text_color(SECONDARY_TEXT())
                        .child(err.clone()),
                );
            }
        }

        let buttons = div()
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
                        cx.listener(|this, _: &gpui::MouseDownEvent, _window, cx| {
                            this.cancel_skill_card(cx);
                        }),
                    )
                    .child(match stage {
                        crate::app_state::SkillCardStage::Done(_)
                        | crate::app_state::SkillCardStage::Failed(_) => "关闭",
                        _ => "取消",
                    }),
            )
            .when(
                matches!(card.stage, crate::app_state::SkillCardStage::PreviewReady(_)),
                |this| {
                    this.child(
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
                                cx.listener(|this, _: &gpui::MouseDownEvent, _window, cx| {
                                    this.approve_skill_card(cx);
                                }),
                            )
                            .child(div().text_color(gpui::white()).child("应用 Skill")),
                    )
                },
            );

        div()
            .absolute()
            .inset_0()
            .bg(gpui::hsla(0., 0., 0., 0.55))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .w(px(520.0))
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
                    .child(body)
                    .child(buttons),
            )
            .into_any_element()
    }

    pub(crate) fn render_cipher_dialog(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let cipher_is_set = crate::agents::remote_auth::RemoteAuth::is_cipher_set();
        let msg = self.cipher_message.clone();
        let msg_is_error = self.cipher_message_is_error;

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
                    this.close_cipher_dialog(cx);
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
                            .child("远程暗号设置"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(SECONDARY_TEXT())
                            .child("暗号用于 Telegram 远程触发危险操作时的确认。暗号仅在本机设置，不经过网络传输。"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(if cipher_is_set {
                                gpui::hsla(0.33, 0.6, 0.5, 1.0)
                            } else {
                                SECONDARY_TEXT()
                            })
                            .child(if cipher_is_set {
                                "✅ 暗号已设置"
                            } else {
                                "⏹ 暗号未设置"
                            }),
                    )
                    // --- Telegram 绑定区 ---
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .mt_1()
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(PRIMARY_TEXT())
                                    .font_weight(FontWeight::BOLD)
                                    .child("Telegram 远程控制"),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(SECONDARY_TEXT())
                                    .child("绑定后可通过 Telegram 远程执行 Skill 操作。"),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(SECONDARY_TEXT())
                                    .child("Bot Token"),
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
                                    .child(
                                        div()
                                            .flex_1()
                                            .text_xs()
                                            .text_color(SECONDARY_TEXT())
                                            .child("Telegram Bot Token 输入区域"),
                                    ),
                            )
                            .when(!self.telegram_bind_status.is_empty(), |this| {
                                this.child(
                                    div()
                                        .text_xs()
                                        .text_color(if self.telegram_bind_error {
                                            gpui::hsla(0.0, 0.7, 0.5, 1.0)
                                        } else {
                                            gpui::hsla(0.33, 0.6, 0.5, 1.0)
                                        })
                                        .child(self.telegram_bind_status.clone()),
                                )
                            })
                            .child(
                                div()
                                    .flex()
                                    .gap_3()
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
                                                    |this, _: &gpui::MouseDownEvent, _window, cx| {
                                                        this.start_telegram_bind(cx);
                                                    },
                                                ),
                                            )
                                            .child(
                                                div()
                                                    .text_sm()
                                                    .text_color(gpui::white())
                                                    .child("绑定 Telegram"),
                                            ),
                                    )
                                    .when(
                                        crate::services::load_config()
                                            .telegram_bot_token
                                            .is_some(),
                                        |this| {
                                            this.child(
                                                div()
                                                    .flex_1()
                                                    .h(px(36.0))
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .rounded_lg()
                                                    .border_1()
                                                    .border_color(gpui::hsla(0.0, 0.6, 0.5, 1.0))
                                                    .bg(CANVAS_BG())
                                                    .cursor_pointer()
                                                    .on_mouse_down(
                                                        gpui::MouseButton::Left,
                                                        cx.listener(
                                                            |this,
                                                             _: &gpui::MouseDownEvent,
                                                             _window,
                                                             cx| {
                                                                this.handle_telegram_unbind(cx);
                                                            },
                                                        ),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_sm()
                                                            .text_color(gpui::hsla(
                                                                0.0, 0.6, 0.5, 1.0,
                                                            ))
                                                            .child("解绑"),
                                                    ),
                                            )
                                        },
                                    ),
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
                                    .child("新暗号"),
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
                                    .child(
                                        div()
                                            .flex_1()
                                            .child("暗号输入区域 - 请在设置页面编辑"),
                                    ),
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
                                    .child("确认暗号"),
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
                                    .child(
                                        div()
                                            .flex_1()
                                            .child("确认暗号输入区域"),
                                    ),
                            ),
                    )
                    .when(!msg.is_empty(), |this| {
                        this.child(
                            div()
                                .text_xs()
                                .text_color(if msg_is_error {
                                    gpui::hsla(0.0, 0.7, 0.5, 1.0)
                                } else {
                                    gpui::hsla(0.33, 0.6, 0.5, 1.0)
                                })
                                .child(msg),
                        )
                    })
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
                                                this.close_cipher_dialog(cx);
                                            },
                                        ),
                                    )
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(PRIMARY_TEXT())
                                            .child("取消"),
                                    ),
                            )
                            .when(cipher_is_set, |this| {
                                this.child(
                                    div()
                                        .flex_1()
                                        .h(px(36.0))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .rounded_lg()
                                        .border_1()
                                        .border_color(gpui::hsla(0.0, 0.6, 0.5, 1.0))
                                        .bg(CANVAS_BG())
                                        .cursor_pointer()
                                        .on_mouse_down(
                                            gpui::MouseButton::Left,
                                            cx.listener(
                                                |this, _: &gpui::MouseDownEvent, _window, cx| {
                                                    this.clear_cipher(cx);
                                                },
                                            ),
                                        )
                                        .child(
                                            div()
                                                .text_sm()
                                                .text_color(gpui::hsla(0.0, 0.6, 0.5, 1.0))
                                                .child("清除暗号"),
                                        ),
                                )
                            })
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
                                            |this, _: &gpui::MouseDownEvent, _window, cx| {
                                                this.save_cipher(cx);
                                            },
                                        ),
                                    )
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(gpui::white())
                                            .child(if cipher_is_set {
                                                "修改暗号"
                                            } else {
                                                "设置暗号"
                                            }),
                                    ),
                            ),
                    ),
            )
            .into_any_element()
    }
}

fn human_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut idx = 0;
    while size >= 1024.0 && idx < UNITS.len() - 1 {
        size /= 1024.0;
        idx += 1;
    }
    format!("{:.1} {}", size, UNITS[idx])
}
