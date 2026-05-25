use std::path::{Path, PathBuf};
use std::process::Command;

use gpui::{
    div, prelude::*, px, svg, AnyElement, Context, InteractiveElement, StatefulInteractiveElement,
    Window,
};

use crate::i18n::{t, Translations};

#[derive(Debug, Clone)]
pub(crate) struct InstalledSkill {
    pub name: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SkillsMarketTab {
    Market,
    Installed,
}

impl Default for SkillsMarketTab {
    fn default() -> Self {
        Self::Market
    }
}

#[derive(Debug)]
pub(crate) struct SkillsMarketState {
    pub installed: Vec<InstalledSkill>,
    pub selected: Option<InstalledSkill>,
    pub show_detail: bool,
    pub status_text: Option<String>,
    pub error_text: Option<String>,
    pub active_tab: SkillsMarketTab,
    pub active_category: usize,
}

impl Default for SkillsMarketState {
    fn default() -> Self {
        Self {
            installed: Vec::new(),
            selected: None,
            show_detail: false,
            status_text: None,
            error_text: None,
            active_tab: SkillsMarketTab::default(),
            active_category: 0,
        }
    }
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
                    items.push(InstalledSkill { name, path });
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

pub(crate) fn render_skills_market_titlebar(
    app: &crate::AppState,
    _window: &mut Window,
    cx: &mut Context<crate::AppState>,
) -> AnyElement {
    let lang = app.current_lang;
    let state = &app.skills_market;
    let active_tab = state.active_tab;
    let active_category = state.active_category;

    let category_labels: [&'static str; 6] = [
        "All",
        "Dev Tools",
        "Data Analysis",
        "UI Design",
        "Content Creation",
        "Efficiency",
    ];

    let tab_market_active = active_tab == SkillsMarketTab::Market;
    let tab_installed_active = active_tab == SkillsMarketTab::Installed;
    let installed_count = state.installed.len();

    let text_btn = |label: &'static str| {
        div()
            .text_xs()
            .text_color(crate::SECONDARY_TEXT())
            .font_weight(gpui::FontWeight::BOLD)
            .child(label)
    };

    div()
        .flex()
        .items_center()
        .justify_between()
        .h_full()
        .px_8()
        .child(
            div()
                .w(px(320.0))
                .flex_none()
                .flex()
                .items_center()
                .gap_2()
                .overflow_hidden()
                .child(
                    div()
                        .text_size(px(18.0))
                        .text_color(crate::PRIMARY_TEXT())
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_ellipsis()
                        .child(t(lang, Translations::CAPABILITIES)),
                )
                .child(crate::render_icon_element(
                    "skill",
                    crate::SECONDARY_TEXT(),
                    13.0,
                )),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap_6()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_4()
                        .child(
                            div()
                                .text_xs()
                                .text_color(if tab_market_active {
                                    crate::ACCENT_TEXT()
                                } else {
                                    crate::SECONDARY_TEXT()
                                })
                                .font_weight(gpui::FontWeight::BOLD)
                                .when(tab_market_active, |this| this.pb_3().border_b_1().border_color(crate::BRAND_BLUE()))
                                .cursor_pointer()
                                .on_mouse_down(
                                    gpui::MouseButton::Left,
                                    cx.listener(|this, _: &gpui::MouseDownEvent, _w, cx| {
                                        this.skills_market.active_tab = SkillsMarketTab::Market;
                                        cx.notify();
                                    }),
                                )
                                .child("Market"),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(if tab_installed_active {
                                    crate::ACCENT_TEXT()
                                } else {
                                    crate::SECONDARY_TEXT()
                                })
                                .font_weight(gpui::FontWeight::BOLD)
                                .when(tab_installed_active, |this| this.pb_3().border_b_1().border_color(crate::BRAND_BLUE()))
                                .cursor_pointer()
                                .on_mouse_down(
                                    gpui::MouseButton::Left,
                                    cx.listener(|this, _: &gpui::MouseDownEvent, _w, cx| {
                                        this.skills_market.active_tab = SkillsMarketTab::Installed;
                                        cx.notify();
                                    }),
                                )
                                .child(format!("Installed ({})", installed_count)),
                        ),
                )
                .child(
                    div()
                        .id("upload-skill-btn")
                        .cursor_pointer()
                        .on_mouse_down(
                            gpui::MouseButton::Left,
                            cx.listener(|this, _: &gpui::MouseDownEvent, _window, cx| {
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
                            }),
                        )
                        .child(text_btn(t(lang, Translations::UPLOAD))),
                )
                .child(
                    div()
                        .cursor_pointer()
                        .on_mouse_down(
                            gpui::MouseButton::Left,
                            cx.listener(|this, _: &gpui::MouseDownEvent, _window, cx| {
                                let current = this.skills_market.active_category;
                                this.skills_market.active_category = (current + 1) % 6;
                                cx.notify();
                            }),
                        )
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(
                            div()
                                .text_xs()
                                .text_color(crate::MUTED_TEXT())
                                .child(category_labels[active_category]),
                        )
                        .child(text_btn("▼")),
                ),
        )
        .into_any_element()
}

pub(crate) fn render_skills_market(
    app: &crate::AppState,
    window: &mut Window,
    cx: &mut Context<crate::AppState>,
) -> AnyElement {
    let lang = app.current_lang;
    let state = &app.skills_market;
    let active_tab = state.active_tab;
    let active_category = state.active_category;

    const MIN_CARD_W: f32 = 260.0;
    const MAX_CARD_W: f32 = 330.0;
    const MAX_COLS: usize = 6;
    const CARD_H: f32 = 144.0;
    const GAP: f32 = 24.0;
    const SIDE_PAD: f32 = 32.0;

    let window_w: f32 = window.bounds().size.width.into();
    let mut main_w = window_w - crate::NAV_WIDTH - 1.0;
    if app.sidebar_visible {
        main_w -= 340.0 + 1.0;
    }
    if app.terminal_visible {
        main_w -= app.terminal_width + 1.0;
    }
    if main_w < 320.0 {
        main_w = 320.0;
    }

    let max_content_w =
        MAX_CARD_W * MAX_COLS as f32 + GAP * (MAX_COLS as f32 - 1.0) + SIDE_PAD * 2.0;
    let content_w = main_w.min(max_content_w).max(MIN_CARD_W + SIDE_PAD * 2.0);
    let available_w = (content_w - SIDE_PAD * 2.0).max(MIN_CARD_W);

    let mut cols = (((available_w + GAP) / (MIN_CARD_W + GAP)).floor() as usize).clamp(1, MAX_COLS);
    loop {
        if cols >= MAX_COLS {
            break;
        }
        let current_card_w = (available_w - GAP * (cols as f32 - 1.0)) / cols as f32;
        if current_card_w <= MAX_CARD_W {
            break;
        }
        let next_cols = cols + 1;
        let next_card_w = (available_w - GAP * (next_cols as f32 - 1.0)) / next_cols as f32;
        if next_card_w >= MIN_CARD_W {
            cols = next_cols;
        } else {
            break;
        }
    }
    let card_w =
        ((available_w - GAP * (cols as f32 - 1.0)) / cols as f32).clamp(MIN_CARD_W, MAX_CARD_W);

    let render_card = |title: String, tag: &'static str, description: String, icon: AnyElement| {
        div()
            .w(px(card_w))
            .h(px(CARD_H))
            .flex_none()
            .p_5()
            .rounded_xl()
            .border_1()
            .border_color(crate::BORDER_LIGHT())
            .bg(crate::SURFACE_PANEL())
            .shadow_md()
            .hover(|this| this.bg(crate::SURFACE_ELEVATED()))
            .child(
                div()
                    .flex()
                    .items_start()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(
                                div()
                                    .size(px(28.0))
                                    .rounded_md()
                                    .bg(crate::CANVAS_BG())
                                    .border_1()
                                    .border_color(crate::BORDER_LIGHT())
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_2()
                                    .child(
                                        div()
                                            .text_base()
                                            .text_color(crate::PRIMARY_TEXT())
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .text_ellipsis()
                                            .child(title),
                                    )
                                    .child(
                                        div()
                                            .px_2()
                                            .py_1()
                                            .rounded_md()
                                            .bg(crate::CANVAS_BG())
                                            .border_1()
                                            .border_color(crate::BORDER_LIGHT())
                                            .text_xs()
                                            .text_color(crate::MUTED_TEXT())
                                            .child(tag),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .size(px(28.0))
                            .rounded_md()
                            .bg(crate::SURFACE_ELEVATED())
                            .border_1()
                            .border_color(crate::BORDER_LIGHT())
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_sm()
                            .text_color(crate::SECONDARY_TEXT())
                            .child("+"),
                    ),
            )
            .child(
                div()
                    .mt_4()
                    .text_sm()
                    .text_color(crate::SECONDARY_TEXT())
                    .overflow_hidden()
                    .text_ellipsis()
                    .child(description),
            )
    };

    let mut grid = div()
        .flex()
        .flex_col()
        .items_center()
        .bg(crate::CANVAS_BG())
        .id("skills-market-list")
        .overflow_scroll();

    let mut inner = div()
        .w(px(content_w))
        .px_8()
        .pt_6()
        .pb_8()
        .flex()
        .flex_col()
        .gap_6();

    if active_tab == SkillsMarketTab::Market {
        let market_cards: Vec<(usize, &'static str, &'static str, &'static str)> = vec![
            (
                1,
                "alipay-payment-integration",
                "FINTECH",
                "Best practices for integrating Alipay into products, covering online and...",
            ),
            (
                1,
                "react-best-practices",
                "DEVELOPMENT",
                "Advanced Vercel engineering standards for React and Next.js,...",
            ),
            (
                4,
                "douyin-interact-creation",
                "SOCIAL",
                "Assist creators in building interactive spaces meeting platform standards,...",
            ),
            (
                2,
                "redis-development",
                "DATABASE",
                "Best practices for Redis data structures, vector search (RedisVL),...",
            ),
            (
                1,
                "gh-cli-master",
                "GIT",
                "Full guide for GitHub CLI (gh), covering issues, PRs,...",
            ),
            (
                3,
                "web-artifacts-builder",
                "UIDESIGN",
                "Construct complex multi-component HTML/React/Tailwind artifacts with...",
            ),
            (
                1,
                "security-best-practice",
                "SECURITY",
                "Safety audit recommendations for Python, JavaScript/TypeScript, and Go",
            ),
            (
                0,
                "mcp-builder",
                "AI INFRASTRUCTURE",
                "Guide for building high-quality Model Context Protocol servers to connect...",
            ),
            (
                5,
                "webapp-testing",
                "QA",
                "Utilize Playwright for local Web app testing and debugging,...",
            ),
        ];

        let cards = market_cards
            .into_iter()
            .filter(|(cat, ..)| active_category == 0 || *cat == active_category)
            .collect::<Vec<_>>();

        if cards.is_empty() {
            inner = inner.child(
                div()
                    .p_5()
                    .rounded_xl()
                    .border_1()
                    .border_color(crate::BORDER_LIGHT())
                    .bg(crate::SURFACE_PANEL())
                    .text_sm()
                    .text_color(crate::MUTED_TEXT())
                    .child("No items in this category."),
            );
        } else {
            let mut i = 0usize;
            while i < cards.len() {
                let row_items = cards[i..cards.len().min(i + cols)].to_vec();
                let mut row = div().flex().gap_6();
                for (_, title, tag, desc) in row_items.iter() {
                    let icon = div()
                        .text_xs()
                        .text_color(crate::SECONDARY_TEXT())
                        .child(crate::icon_label("skill"))
                        .into_any_element();

                    row = row.child(render_card(
                        (*title).to_string(),
                        *tag,
                        (*desc).to_string(),
                        icon,
                    ));
                }
                if row_items.len() < cols {
                    for _ in 0..(cols - row_items.len()) {
                        row = row.child(div().w(px(card_w)).h(px(CARD_H)).flex_none());
                    }
                }
                inner = inner.child(row);
                i += cols;
            }
        }
    } else if state.installed.is_empty() {
        inner = inner.child(
            div()
                .p_5()
                .rounded_xl()
                .border_1()
                .border_color(crate::BORDER_LIGHT())
                .bg(crate::SURFACE_PANEL())
                .text_sm()
                .text_color(crate::MUTED_TEXT())
                .child(t(lang, Translations::NO_SKILLS)),
        );
    } else {
        let skills = state.installed.clone();
        let mut i = 0usize;
        while i < skills.len() {
            let row_items = skills[i..skills.len().min(i + cols)].to_vec();
            let row_len = row_items.len();
            let mut row = div().flex().gap_6();
            for skill in row_items {
                let skill_for_click = skill.clone();
                let icon = match crate::icon_asset_path("skill") {
                    Some(path) => svg()
                        .path(path)
                        .size(px(16.0))
                        .flex_none()
                        .text_color(crate::SECONDARY_TEXT())
                        .into_any_element(),
                    None => div()
                        .text_xs()
                        .text_color(crate::SECONDARY_TEXT())
                        .child(crate::icon_label("skill"))
                        .into_any_element(),
                };

                row = row.child(
                    render_card(
                        skill.name,
                        "LOCAL",
                        skill.path.to_string_lossy().to_string(),
                        icon,
                    )
                    .cursor_pointer()
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(move |this, _: &gpui::MouseDownEvent, _window, cx| {
                            this.skills_market.selected = Some(skill_for_click.clone());
                            this.skills_market.show_detail = true;
                            cx.notify();
                        }),
                    ),
                );
            }
            if row_len < cols {
                for _ in 0..(cols - row_len) {
                    row = row.child(div().w(px(card_w)).h(px(CARD_H)).flex_none());
                }
            }
            inner = inner.child(row);
            i += cols;
        }
    }

    grid = grid.child(inner);

    let mut page = div()
        .flex()
        .flex_col()
        .flex_1()
        .h_full()
        .bg(crate::CANVAS_BG())
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
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(|this, _: &gpui::MouseDownEvent, _window, cx| {
                        this.skills_market.show_detail = false;
                        this.skills_market.selected = None;
                        cx.notify();
                    }),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_3()
                        .w(px(520.0))
                        .p_5()
                        .bg(crate::SURFACE_PANEL())
                        .rounded_xl()
                        .border_1()
                        .border_color(crate::BORDER_LIGHT())
                        .shadow_md()
                        .on_mouse_down(
                            gpui::MouseButton::Left,
                            cx.listener(|_, _: &gpui::MouseDownEvent, _window, _cx| {}),
                        )
                        .child(
                            div()
                                .text_base()
                                .text_color(crate::PRIMARY_TEXT())
                                .font_weight(gpui::FontWeight::BOLD)
                                .child(title),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(crate::SECONDARY_TEXT())
                                .child(path_str),
                        )
                        .child(
                            div().flex().gap_3().mt_2().child(
                                div()
                                    .flex_1()
                                    .h(px(36.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_lg()
                                    .border_1()
                                    .border_color(crate::BORDER_LIGHT())
                                    .bg(crate::CANVAS_BG())
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        gpui::MouseButton::Left,
                                        cx.listener(
                                            |this, _: &gpui::MouseDownEvent, _window, cx| {
                                                this.skills_market.show_detail = false;
                                                this.skills_market.selected = None;
                                                cx.notify();
                                            },
                                        ),
                                    )
                                    .child(t(lang, Translations::CLOSE)),
                            ),
                        ),
                )
                .into_any_element();

            page = div()
                .relative()
                .flex()
                .flex_1()
                .child(page)
                .child(overlay)
                .into_any_element();
        }
    }

    page
}
