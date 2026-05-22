use std::path::{Path, PathBuf};
use std::process::Command;

use gpui::{div, px, AnyElement, Context, InteractiveElement, StatefulInteractiveElement, Window, prelude::*};

use crate::i18n::{t, Translations};

#[derive(Debug, Clone)]
pub(crate) struct InstalledSkill {
    pub name: String,
    pub path: PathBuf,
}

#[derive(Debug, Default)]
pub(crate) struct SkillsMarketState {
    pub installed: Vec<InstalledSkill>,
    pub selected: Option<InstalledSkill>,
    pub show_detail: bool,
    pub status_text: Option<String>,
    pub error_text: Option<String>,
}

impl SkillsMarketState {
    pub fn new() -> Self {
        let mut state = Self::default();
        state.refresh();
        state
    }

    fn skills_root_dir() -> PathBuf {
        if let Some(dir) = dirs::data_dir() {
            return dir.join("one").join("skills");
        }
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".one")
            .join("skills")
    }

    pub fn refresh(&mut self) {
        let root = Self::skills_root_dir();
        std::fs::create_dir_all(&root).ok();

        let mut items = Vec::new();
        if let Ok(read_dir) = std::fs::read_dir(&root) {
            for entry in read_dir.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    items.push(InstalledSkill {
                        name,
                        path,
                    });
                }
            }
        }

        items.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        self.installed = items;
    }

    fn looks_like_zip(path: &Path) -> bool {
        let mut buf = [0u8; 4];
        let Ok(mut f) = std::fs::File::open(path) else {
            return false;
        };
        let Ok(n) = std::io::Read::read(&mut f, &mut buf) else {
            return false;
        };
        n >= 2 && buf[0] == b'P' && buf[1] == b'K'
    }

    fn install_zip_like(&self, src: &Path, dest_dir: &Path) -> Result<(), String> {
        let status = Command::new("unzip")
            .arg("-o")
            .arg(src)
            .arg("-d")
            .arg(dest_dir)
            .status()
            .map_err(|e| format!("unzip 执行失败: {e}"))?;
        if !status.success() {
            return Err("unzip 解压失败".to_string());
        }
        Ok(())
    }

    pub fn install_from_file(&mut self, file_path: PathBuf) -> Result<(), String> {
        let ext = file_path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();

        if ext != "zip" && ext != "skill" {
            return Err("仅支持 .skill 或 .zip".to_string());
        }

        let root = Self::skills_root_dir();
        std::fs::create_dir_all(&root).map_err(|e| format!("创建 skills 目录失败: {e}"))?;

        let stem = file_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("skill")
            .to_string();
        let dest_dir = root.join(stem);
        std::fs::create_dir_all(&dest_dir).map_err(|e| format!("创建安装目录失败: {e}"))?;

        let zip_like = ext == "zip" || Self::looks_like_zip(&file_path);
        if zip_like {
            self.install_zip_like(&file_path, &dest_dir)?;
        } else {
            let file_name = file_path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("skill.skill");
            let dest = dest_dir.join(file_name);
            std::fs::copy(&file_path, &dest).map_err(|e| format!("复制文件失败: {e}"))?;
        }

        self.status_text = Some("安装成功".to_string());
        self.error_text = None;
        self.refresh();
        Ok(())
    }
}

pub(crate) fn render_skills_market(
    app: &crate::AppState,
    _window: &mut Window,
    cx: &mut Context<crate::AppState>,
) -> AnyElement {
    let lang = app.current_lang;
    let state = &app.skills_market;

    let mut header = div()
        .flex()
        .items_center()
        .justify_between()
        .px_5()
        .py_4()
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(div().text_base().text_color(crate::PRIMARY_TEXT).font_weight(gpui::FontWeight::BOLD).child(t(lang, Translations::SKILLS)))
                .child(div().text_sm().text_color(crate::MUTED_TEXT).child(t(lang, Translations::SKILLS_HINT))),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .px_3()
                        .py_2()
                        .rounded_md()
                        .bg(crate::BRAND_BLUE)
                        .cursor_pointer()
                        .text_sm()
                        .text_color(gpui::white())
                        .on_mouse_down(gpui::MouseButton::Left, cx.listener(|this, _: &gpui::MouseDownEvent, _window, cx| {
                            if let Some(path) = rfd::FileDialog::new()
                                .set_title(t(this.current_lang, Translations::UPLOAD_SKILL_PACKAGE))
                                .add_filter("Skill", &["skill", "zip"])
                                .pick_file()
                            {
                                if let Err(err) = this.skills_market.install_from_file(path) {
                                    this.skills_market.status_text = None;
                                    this.skills_market.error_text = Some(err);
                                }
                                cx.notify();
                            }
                        }))
                        .child(t(lang, Translations::UPLOAD)),
                ),
        );

    if let Some(status) = state.status_text.clone() {
        header = header.child(
            div()
                .px_5()
                .pb_2()
                .text_sm()
                .text_color(crate::SECONDARY_TEXT)
                .child(status),
        );
    }
    if let Some(error) = state.error_text.clone() {
        header = header.child(
            div()
                .px_5()
                .pb_2()
                .text_sm()
                .text_color(gpui::hsla(0.0, 0.8, 0.55, 1.0))
                .child(error),
        );
    }

    let mut grid = div()
        .flex()
        .flex_col()
        .gap_3()
        .px_5()
        .pb_5()
        .id("skills-market-list")
        .overflow_scroll();

    if state.installed.is_empty() {
        grid = grid.child(
            div()
                .p_4()
                .rounded_md()
                .border_1()
                .border_color(crate::BORDER_LIGHT)
                .bg(crate::CARD_BG)
                .text_sm()
                .text_color(crate::MUTED_TEXT)
                .child(t(lang, Translations::NO_SKILLS)),
        );
    } else {
        for skill in state.installed.clone() {
            let skill_for_click = skill.clone();
            grid = grid.child(
                div()
                    .p_4()
                    .rounded_lg()
                    .border_1()
                    .border_color(crate::BORDER_LIGHT)
                    .bg(crate::CARD_BG)
                    .cursor_pointer()
                    .on_mouse_down(gpui::MouseButton::Left, cx.listener(move |this, _: &gpui::MouseDownEvent, _window, cx| {
                        this.skills_market.selected = Some(skill_for_click.clone());
                        this.skills_market.show_detail = true;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(div().text_base().text_color(crate::PRIMARY_TEXT).font_weight(gpui::FontWeight::BOLD).child(skill.name))
                            .child(div().text_xs().text_color(crate::MUTED_TEXT).child(t(lang, Translations::DETAILS))),
                    )
                    .child(
                        div()
                            .mt_2()
                            .text_sm()
                            .text_color(crate::SECONDARY_TEXT)
                            .child(skill.path.to_string_lossy().to_string()),
                    ),
            );
        }
    }

    let mut page = div()
        .flex()
        .flex_col()
        .flex_1()
        .h_full()
        .bg(crate::CARD_BG)
        .child(header)
        .child(div().h(px(1.0)).bg(crate::BORDER_LIGHT))
        .child(grid)
        .into_any_element();

    if state.show_detail {
        if let Some(selected) = state.selected.clone() {
            let title = selected.name.clone();
            let path_str = selected.path.to_string_lossy().to_string();
            let overlay = div()
                .absolute()
                .inset_0()
                .bg(gpui::hsla(0., 0., 0., 0.5))
                .flex()
                .items_center()
                .justify_center()
                .on_mouse_down(gpui::MouseButton::Left, cx.listener(|this, _: &gpui::MouseDownEvent, _window, cx| {
                    this.skills_market.show_detail = false;
                    this.skills_market.selected = None;
                    cx.notify();
                }))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_3()
                        .w(px(520.0))
                        .p_5()
                        .bg(crate::CARD_BG)
                        .rounded_lg()
                        .border_1()
                        .border_color(crate::BORDER_LIGHT)
                        .shadow_md()
                        .on_mouse_down(gpui::MouseButton::Left, cx.listener(|_, _: &gpui::MouseDownEvent, _window, _cx| {}))
                        .child(
                            div()
                                .text_base()
                                .text_color(crate::PRIMARY_TEXT)
                                .font_weight(gpui::FontWeight::BOLD)
                                .child(title),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(crate::SECONDARY_TEXT)
                                .child(path_str),
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
                                        .rounded_md()
                                        .border_1()
                                        .border_color(crate::BORDER_LIGHT)
                                        .cursor_pointer()
                                        .on_mouse_down(gpui::MouseButton::Left, cx.listener(|this, _: &gpui::MouseDownEvent, _window, cx| {
                                            this.skills_market.show_detail = false;
                                            this.skills_market.selected = None;
                                            cx.notify();
                                        }))
                                        .child(t(lang, Translations::CLOSE)),
                                ),
                        ),
                )
                .into_any_element();

            page = div().relative().flex().flex_1().child(page).child(overlay).into_any_element();
        }
    }

    page
}
