use gpui::{
    div, prelude::FluentBuilder, px, svg, AnyElement, Context, FontWeight, Hsla,
    InteractiveElement, IntoElement, ParentElement, Render, StatefulInteractiveElement, Styled,
    Window,
};
use crate::agents::types::FormattedContent;
use crate::ui_theme::{
    ASSISTANT_BUBBLE_BG, BORDER_LIGHT, BRAND_BLUE, CODE_BG, ERROR_TEXT, FLOATING_PANEL_BG,
    PRIMARY_TEXT, SECONDARY_TEXT, SURFACE_PANEL,
};

pub use crate::agents::types::ProcessDisplayInfo;

pub(crate) struct HeaderTooltip {
    pub(crate) text: String,
}

impl Render for HeaderTooltip {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .max_w(px(420.0))
            .px_3()
            .py_2()
            .rounded_md()
            .bg(SURFACE_PANEL())
            .border_1()
            .border_color(BORDER_LIGHT())
            .text_xs()
            .text_color(PRIMARY_TEXT())
            .whitespace_normal()
            .child(self.text.clone())
    }
}

pub(crate) struct TitleTooltip {
    pub text: String,
}

impl Render for TitleTooltip {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .max_w(px(420.0))
            .px_3()
            .py_2()
            .rounded_md()
            .bg(SURFACE_PANEL())
            .border_1()
            .border_color(BORDER_LIGHT())
            .text_xs()
            .text_color(PRIMARY_TEXT())
            .whitespace_normal()
            .child(self.text.clone())
    }
}

#[derive(Debug, Clone)]
pub enum ContentPart {
    Normal(String),
    Think { text: String, complete: bool },
    ProcessTable { processes: Vec<ProcessDisplayInfo> },
}

pub fn strip_think_tags(content: &str) -> String {
    let mut result = content.to_string();
    while let Some(start) = result.find("<think>") {
        if let Some(end) = result[start..].find("</think>") {
            result = format!(
                "{}{}",
                &result[..start],
                &result[start + end + "</think>".len()..]
            );
        } else {
            break;
        }
    }
    result
}

pub fn parse_think_content(content: &str) -> Vec<ContentPart> {
    let open = "<think>";
    let close = "</think>";

    let mut parts = Vec::new();
    let mut pos = 0;
    let mut last_was_think = false;

    while pos < content.len() {
        let start_rel = match content[pos..].find(open) {
            Some(idx) => idx,
            None => break,
        };
        let start = pos + start_rel;

        if pos < start {
            let mut text = content[pos..start].to_string();
            if last_was_think {
                text = trim_leading_newlines_with_threshold(&text, 4);
            }
            if !text.is_empty() {
                if let Some(procs) = crate::agents::types::try_parse_process_list(&text) {
                    parts.push(ContentPart::ProcessTable { processes: procs });
                } else {
                    parts.push(ContentPart::Normal(text));
                }
            }
        }

        let inner_pos = start + open.len();
        let end_rel = match content[inner_pos..].find(close) {
            Some(idx) => idx,
            None => {
                parts.push(ContentPart::Think {
                    text: content[inner_pos..].to_string(),
                    complete: false,
                });
                pos = content.len();
                continue;
            }
        };
        let end = inner_pos + end_rel;

        parts.push(ContentPart::Think {
            text: content[inner_pos..][..end_rel].to_string(),
            complete: true,
        });

        last_was_think = true;
        pos = end + close.len();
    }

    if pos < content.len() {
        let mut text = content[pos..].to_string();
        if last_was_think {
            text = trim_leading_newlines_with_threshold(&text, 4);
        }
        if !text.is_empty() {
            if let Some(procs) = crate::agents::types::try_parse_process_list(&text) {
                parts.push(ContentPart::ProcessTable { processes: procs });
            } else {
                parts.push(ContentPart::Normal(text));
            }
        }
    }

    parts
}

fn trim_leading_newlines_with_threshold(text: &str, threshold: usize) -> String {
    let mut count = 0;
    let mut start_idx = 0;
    for (i, c) in text.char_indices() {
        if c == '\n' || c == '\r' {
            count += 1;
            start_idx = i + c.len_utf8();
            if count >= threshold {
                break;
            }
        } else {
            break;
        }
    }
    text[start_idx..].to_string()
}

pub fn render_process_table(processes: &[ProcessDisplayInfo]) -> gpui::AnyElement {
    let critical_count = processes.iter().filter(|p| p.is_critical).count();
    div()
        .flex_col()
        .w_full()
        .max_w(px(806.0))
        .rounded_xl()
        .bg(ASSISTANT_BUBBLE_BG())
        .border_1()
        .border_color(BORDER_LIGHT())
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .px_6()
                .py_4()
                .bg(FLOATING_PANEL_BG())
                .border_b_1()
                .border_color(BORDER_LIGHT())
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            svg()
                                .path("activity.svg")
                                .size(px(20.0))
                                .text_color(BRAND_BLUE()),
                        )
                        .child(
                            div()
                                .text_base()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(PRIMARY_TEXT())
                                .child("System Process Monitor"),
                        ),
                )
                .when(critical_count > 0, |this| {
                    this.child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .px_3()
                            .py_1()
                            .rounded_sm()
                            .bg(Hsla {
                                h: 0.0,
                                s: 1.0,
                                l: 0.88,
                                a: 1.0,
                            })
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(ERROR_TEXT())
                                    .child(format!("{} High Usage Warnings", critical_count)),
                            ),
                    )
                }),
        )
        .child(
            div()
                .flex()
                .items_center()
                .px_6()
                .py_3()
                .bg(FLOATING_PANEL_BG())
                .border_b_1()
                .border_color(BORDER_LIGHT())
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .w(px(245.0))
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(SECONDARY_TEXT())
                        .child("Process Name"),
                )
                .child(
                    div()
                        .w(px(120.0))
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(SECONDARY_TEXT())
                        .child("PID"),
                )
                .child(
                    div()
                        .w(px(200.0))
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(SECONDARY_TEXT())
                        .child("CPU %"),
                )
                .child(
                    div()
                        .flex_1()
                        .text_right()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(SECONDARY_TEXT())
                        .child("Memory"),
                ),
        )
        .child(
            div()
                .id("process_table_body")
                .flex_1()
                .overflow_scroll()
                .max_h(px(400.0))
                .children(processes.iter().map(|proc| {
                    let cpu_bar_color = if proc.is_critical {
                        ERROR_TEXT()
                    } else {
                        BRAND_BLUE()
                    };
                    let bg_bar_width = (proc.cpu_percent / 100.0 * 180.0).min(180.0);

                    div()
                        .flex()
                        .items_center()
                        .px_6()
                        .py_4()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_3()
                                .w(px(245.0))
                                .child(
                                    div()
                                        .size(px(32.0))
                                        .rounded_sm()
                                        .bg(Hsla {
                                            h: 0.61,
                                            s: 0.62,
                                            l: 0.88,
                                            a: 1.0,
                                        })
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(
                                            svg()
                                                .path("cpu.svg")
                                                .size(px(16.0))
                                                .text_color(BRAND_BLUE()),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex_col()
                                        .child(
                                            div()
                                                .text_sm()
                                                .font_weight(FontWeight::MEDIUM)
                                                .text_color(PRIMARY_TEXT())
                                                .child(proc.name.clone()),
                                        ),
                                ),
                        )
                        .child(
                            div()
                                .w(px(120.0))
                                .text_sm()
                                .text_color(SECONDARY_TEXT())
                                .child(proc.pid.to_string()),
                        )
                        .child(
                            div()
                                .w(px(200.0))
                                .flex_col()
                                .gap_1()
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .child(
                                            div()
                                                .text_xs()
                                                .font_weight(FontWeight::MEDIUM)
                                                .text_color(PRIMARY_TEXT())
                                                .child(format!("{:.1}%", proc.cpu_percent)),
                                        ),
                                )
                                .child(
                                    div()
                                        .w(px(180.0))
                                        .h(px(4.0))
                                        .rounded_full()
                                        .bg(Hsla {
                                            h: 0.0,
                                            s: 0.0,
                                            l: 0.9,
                                            a: 1.0,
                                        })
                                        .child(
                                            div()
                                                .w(px(bg_bar_width as f32))
                                                .h_full()
                                                .rounded_full()
                                                .bg(cpu_bar_color),
                                        ),
                                ),
                        )
                        .child(
                            div()
                                .flex_1()
                                .text_right()
                                .text_sm()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(PRIMARY_TEXT())
                                .child(format!("{:.1} MB", proc.memory_mb)),
                        )
                }))
                .into_any_element(),
        )
        .into_any_element()
}

pub fn icon_label(key: &str) -> &'static str {
    match key {
        "workspace" => "WS",
        "capabilities" => "CP",
        "models" => "ML",
        "settings" => "ST",
        "support" => "SP",
        "assistant" => "ONE",
        "folder" => "DIR",
        "share" => "SHR",
        "terminal" => "TTY",
        "run-panel" => "RUN",
        "add" => "ADD",
        "mic" => "MIC",
        "skill" => "SKL",
        _ => "UI",
    }
}

pub fn icon_asset_path(key: &str) -> Option<&'static str> {
    match key {
        "workspace" => Some("thems/workspace.svg"),
        "capabilities" => Some("thems/capabilities.svg"),
        "models" => Some("thems/models.svg"),
        "assistant" => Some("thems/one-ai.svg"),
        "folder" => Some("folder.svg"),
        "share" => Some("thems/attachment.svg"),
        "terminal" => Some("thems/cmd.svg"),
        "run-panel" => Some("thems/side-panel.svg"),
        "add" => Some("thems/attachment.svg"),
        "mic" => Some("thems/mic.svg"),
        "skill" => Some("thems/upload.svg"),
        "upload" => Some("thems/upload.svg"),
        _ => None,
    }
}

pub fn render_icon_element(icon_key: &str, color: Hsla, size_px: f32) -> AnyElement {
    match icon_asset_path(icon_key) {
        Some(path) => svg()
            .path(path)
            .size(px(size_px))
            .flex_none()
            .text_color(color)
            .into_any_element(),
        None => div()
            .text_xs()
            .text_color(color)
            .child(icon_label(icon_key))
            .into_any_element(),
    }
}

pub fn render_formatted_content(
    content: &FormattedContent,
    plain_color: Hsla,
    block_color: Hsla,
) -> gpui::AnyElement {
    match content {
        FormattedContent::Plain(text) => div()
            .text_xs()
            .text_color(plain_color)
            .whitespace_normal()
            .child(text.clone())
            .into_any_element(),
        FormattedContent::Json(text) => div()
            .p_2()
            .rounded_md()
            .bg(CODE_BG())
            .border_1()
            .border_color(BORDER_LIGHT())
            .child(
                div()
                    .text_xs()
                    .text_color(block_color)
                    .font_family("Menlo")
                    .whitespace_normal()
                    .child(text.clone()),
            )
            .into_any_element(),
        FormattedContent::Code(text) => div()
            .p_2()
            .rounded_md()
            .bg(CODE_BG())
            .border_1()
            .border_color(BORDER_LIGHT())
            .child(
                div()
                    .text_xs()
                    .text_color(block_color)
                    .font_family("Menlo")
                    .whitespace_normal()
                    .child(text.clone()),
            )
            .into_any_element(),
    }
}
