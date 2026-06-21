use gpui::{
    div, prelude::*, px, svg, Context, FontWeight, InteractiveElement, IntoElement, ParentElement,
    Styled, Window,
};

use crate::i18n::{t, Translations};
use crate::task_db::TaskArtifactRow;
use crate::ui_theme::{
    BORDER_LIGHT, BRAND_BLUE, CANVAS_BG, MUTED_TEXT, PRIMARY_TEXT, SECONDARY_TEXT,
    SURFACE_ELEVATED, SURFACE_PANEL, WORKSPACE_BG,
};
use crate::AppState;

// ─── Section header helper ────────────────────────────────────────────────────

fn section_header(label: impl Into<String>) -> impl IntoElement {
    div().flex().items_center().gap_2().px_1().pb_1().child(
        div()
            .text_xs()
            .font_weight(FontWeight::BOLD)
            .text_color(MUTED_TEXT())
            .child(label.into()),
    )
}

// ─── Empty row helper ─────────────────────────────────────────────────────────

fn empty_hint(text: impl Into<String>) -> impl IntoElement {
    div()
        .text_xs()
        .text_color(SECONDARY_TEXT())
        .px_1()
        .child(text.into())
}

// ─── Main render ─────────────────────────────────────────────────────────────

impl AppState {
    pub(crate) fn render_sidebar(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let lang = self.current_lang;

        // Pull data we need up front (avoid borrow issues inside closures)
        let artifacts = self
            .active_task_id
            .and_then(|task_id| crate::task_db::load_task_artifacts(&self.db.conn, task_id).ok())
            .unwrap_or_default();
        let preview = self.preview_state.clone();
        let task_dir = self
            .get_active_task_dir_path()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        // ── outer container ──────────────────────────────────────────────────
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(WORKSPACE_BG())
            // ── header bar ───────────────────────────────────────────────────
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .h(px(42.0))
                    .flex_none()
                    .px_3()
                    .bg(SURFACE_ELEVATED())
                    .border_b_1()
                    .border_color(BORDER_LIGHT())
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::BOLD)
                            .text_color(PRIMARY_TEXT())
                            .child(t(lang, Translations::EXPLORER_SIDEBAR)),
                    ),
            )
            // ── scrollable body ──────────────────────────────────────────────
            .child(
                div()
                    .id("sidebar-scroll")
                    .flex_1()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .p_3()
                    // ── § Artifacts ─────────────────────────────────────────
                    .child(self.render_sidebar_artifacts(lang, &artifacts, &task_dir, cx))
                    // ── § Preview ───────────────────────────────────────────
                    .child(self.render_sidebar_preview(lang, preview.as_ref(), cx))
                    // ── § References ────────────────────────────────────────
                    .child(self.render_sidebar_references(lang)),
            )
    }

    // ─── Artifacts section ───────────────────────────────────────────────────

    fn render_sidebar_artifacts(
        &mut self,
        lang: crate::i18n::Lang,
        artifacts: &[TaskArtifactRow],
        task_dir: &str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let task_dir = task_dir.to_string();

        let mut section = div().flex().flex_col().gap_2().child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(section_header(t(lang, Translations::ARTIFACTS)))
                .when(!task_dir.is_empty(), |this| {
                    let td = task_dir.clone();
                    this.child(
                        div()
                            .text_xs()
                            .text_color(BRAND_BLUE())
                            .cursor_pointer()
                            .on_mouse_down(
                                gpui::MouseButton::Left,
                                cx.listener(move |this, _: &gpui::MouseDownEvent, _, _| {
                                    this.open_folder_in_finder(&td);
                                }),
                            )
                            .child(t(lang, Translations::OPEN_TASK_FOLDER)),
                    )
                }),
        );

        if artifacts.is_empty() {
            section = section.child(empty_hint(t(lang, Translations::NO_ARTIFACTS_YET)));
        } else {
            let mut list = div().flex().flex_col().gap_1();
            for artifact in artifacts {
                let path = artifact.path.clone();
                let label = artifact_label(artifact);
                let detail = artifact_detail(artifact);
                let exists = std::path::Path::new(&path).exists();
                list = list.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_2()
                        .py_2()
                        .rounded_sm()
                        .bg(CANVAS_BG())
                        .border_1()
                        .border_color(BORDER_LIGHT())
                        .cursor_pointer()
                        .hover(|this| this.bg(SURFACE_PANEL()))
                        .on_mouse_down(
                            gpui::MouseButton::Left,
                            cx.listener(move |this, _: &gpui::MouseDownEvent, _, _| {
                                this.reveal_file_in_finder(&path);
                            }),
                        )
                        .child(
                            svg()
                                .path(artifact_icon(&artifact.kind))
                                .size(px(14.0))
                                .flex_none()
                                .text_color(if exists { BRAND_BLUE() } else { MUTED_TEXT() }),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(if exists {
                                            PRIMARY_TEXT()
                                        } else {
                                            MUTED_TEXT()
                                        })
                                        .text_ellipsis()
                                        .overflow_hidden()
                                        .child(label),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(SECONDARY_TEXT())
                                        .text_ellipsis()
                                        .overflow_hidden()
                                        .child(detail),
                                ),
                        ),
                );
            }
            section = section.child(list);
        }

        section
    }

    // ─── Preview section ─────────────────────────────────────────────────────

    fn render_sidebar_preview(
        &mut self,
        lang: crate::i18n::Lang,
        preview: Option<&crate::agents::types::PreviewState>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mut section = div()
            .flex()
            .flex_col()
            .gap_2()
            .child(section_header(t(lang, Translations::PREVIEW)));

        let Some(preview) = preview else {
            return section.child(empty_hint(t(lang, Translations::NO_PREVIEW_INFO)));
        };

        // Status badge
        section = section.child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .px_2()
                        .py_1()
                        .rounded_sm()
                        .bg(CANVAS_BG())
                        .border_1()
                        .border_color(BORDER_LIGHT())
                        .text_xs()
                        .text_color(preview.status.color())
                        .font_weight(FontWeight::BOLD)
                        .child(preview.status.label(lang)),
                )
                .when_some(preview.entry_file.clone(), |this, entry| {
                    this.child(
                        div()
                            .text_xs()
                            .text_color(MUTED_TEXT())
                            .text_ellipsis()
                            .overflow_hidden()
                            .child(entry),
                    )
                }),
        );

        // URL + open button
        if let Some(url) = preview.url.clone() {
            let url_for_btn = url.clone();
            section = section.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_2()
                    .rounded_md()
                    .bg(SURFACE_PANEL())
                    .border_1()
                    .border_color(BORDER_LIGHT())
                    .child(
                        div()
                            .text_xs()
                            .text_color(BRAND_BLUE())
                            .whitespace_normal()
                            .child(url),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_center()
                            .px_3()
                            .py_2()
                            .rounded_sm()
                            .bg(BRAND_BLUE())
                            .text_xs()
                            .text_color(gpui::white())
                            .cursor_pointer()
                            .on_mouse_down(
                                gpui::MouseButton::Left,
                                cx.listener(move |this, _: &gpui::MouseDownEvent, _, _| {
                                    this.open_url_in_browser(&url_for_btn);
                                }),
                            )
                            .child(t(lang, Translations::OPEN_IN_BROWSER)),
                    ),
            );
        } else {
            section = section.child(empty_hint(&preview.note));
        }

        section
    }

    // ─── AI Plan / Task Steps section ────────────────────────────────────────

    fn render_sidebar_plan(&self, lang: crate::i18n::Lang, steps: &[String]) -> impl IntoElement {
        let mut section = div()
            .flex()
            .flex_col()
            .gap_2()
            .child(section_header(t(lang, Translations::AI_PLAN)));

        if steps.is_empty() {
            return section.child(empty_hint(t(lang, Translations::NO_PLAN_YET)));
        }

        let mut list = div().flex().flex_col().gap_1();
        for (i, step) in steps.iter().enumerate() {
            list = list.child(
                div()
                    .flex()
                    .items_start()
                    .gap_2()
                    .px_2()
                    .py_2()
                    .rounded_sm()
                    .bg(CANVAS_BG())
                    .border_1()
                    .border_color(BORDER_LIGHT())
                    .child(
                        div()
                            .flex_none()
                            .size(px(16.0))
                            .rounded_full()
                            .bg(SURFACE_PANEL())
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_xs()
                            .text_color(MUTED_TEXT())
                            .child(format!("{}", i + 1)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_xs()
                            .text_color(SECONDARY_TEXT())
                            .whitespace_normal()
                            .child(step.clone()),
                    ),
            );
        }

        section.child(list)
    }

    // ─── References section ───────────────────────────────────────────────────

    fn render_sidebar_references(&self, lang: crate::i18n::Lang) -> impl IntoElement {
        let refs = self.get_active_references();

        let mut section = div()
            .flex()
            .flex_col()
            .gap_2()
            .child(section_header(t(lang, Translations::REFERENCES_SIDEBAR)));

        if refs.is_empty() {
            return section.child(empty_hint(t(lang, Translations::NO_REFERENCES_YET)));
        }

        let mut list = div().flex().flex_col().gap_1();
        for r in &refs {
            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .py_2()
                    .rounded_sm()
                    .bg(CANVAS_BG())
                    .border_1()
                    .border_color(BORDER_LIGHT())
                    .child(
                        svg()
                            .path("thems/attachment.svg")
                            .size(px(12.0))
                            .flex_none()
                            .text_color(MUTED_TEXT()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_xs()
                            .text_color(SECONDARY_TEXT())
                            .text_ellipsis()
                            .overflow_hidden()
                            .child(r.clone()),
                    ),
            );
        }

        section.child(list)
    }
}

fn artifact_label(artifact: &TaskArtifactRow) -> String {
    if !artifact.title.trim().is_empty() {
        return artifact.title.trim().to_string();
    }
    std::path::Path::new(&artifact.path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(&artifact.kind)
        .to_string()
}

fn artifact_detail(artifact: &TaskArtifactRow) -> String {
    let file_name = std::path::Path::new(&artifact.path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(&artifact.path);
    format!("{} · {}", artifact.kind, file_name)
}

fn artifact_icon(kind: &str) -> &'static str {
    match kind {
        "claude_log" => "thems/cmd.svg",
        _ => "thems/attachment.svg",
    }
}
