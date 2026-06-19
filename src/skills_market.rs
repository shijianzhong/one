use std::path::{Path, PathBuf};
use std::process::Command;

use gpui::{
    div, prelude::*, px, relative, svg, AnyElement, Context, InteractiveElement, Styled, Window,
};

use crate::i18n::{t, Translations};
use crate::ui_theme::{
    ACCENT_TEXT, BORDER_LIGHT, BRAND_BLUE, CANVAS_BG, GHOST_SURFACE_BG, HOVER_BG, MUTED_TEXT,
    PRIMARY_TEXT, SECONDARY_TEXT, SURFACE_ELEVATED, SURFACE_PANEL,
};

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
    pub category_dropdown_open: bool,
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
            category_dropdown_open: false,
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
        // 刷新全局 SkillRegistry，使新增的动态 Skill 可用
        crate::skills::refresh_dynamic_skills();
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

    let tab_market_active = active_tab == SkillsMarketTab::Market;
    let tab_installed_active = active_tab == SkillsMarketTab::Installed;
    let installed_count = state.installed.len();

    div()
        .flex()
        .items_center()
        .justify_between()
        .h_full()
        .px_8()
        .child(
            div()
                .flex()
                .items_center()
                .gap_3()
                .child(
                    div()
                        .text_lg()
                        .text_color(PRIMARY_TEXT())
                        .font_weight(gpui::FontWeight::BOLD)
                        .child(t(lang, Translations::SKILLS)),
                )
                .child(
                    div()
                        .px_2()
                        .py_0p5()
                        .rounded_md()
                        .bg(GHOST_SURFACE_BG())
                        .text_size(px(10.0))
                        .text_color(BRAND_BLUE())
                        .font_weight(gpui::FontWeight::BOLD)
                        .child("BETA"),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap_4()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .px_4()
                        .py_0p5()
                        .rounded_xl()
                        .bg(GHOST_SURFACE_BG())
                        .border_1()
                        .border_color(BORDER_LIGHT())
                        .child(
                            div()
                                .px_4()
                                .py_1()
                                .rounded_lg()
                                .when(tab_market_active, |this| {
                                    this.bg(SURFACE_PANEL()).shadow_sm()
                                })
                                .text_xs()
                                .text_color(if tab_market_active {
                                    ACCENT_TEXT()
                                } else {
                                    SECONDARY_TEXT()
                                })
                                .font_weight(gpui::FontWeight::BOLD)
                                .cursor_pointer()
                                .on_mouse_down(
                                    gpui::MouseButton::Left,
                                    cx.listener(|this, _: &gpui::MouseDownEvent, _w, cx| {
                                        this.skills_market.active_tab = SkillsMarketTab::Market;
                                        cx.notify();
                                    }),
                                )
                                .child(t(lang, Translations::MARKET)),
                        )
                        .child(
                            div()
                                .px_4()
                                .py_1()
                                .rounded_lg()
                                .when(tab_installed_active, |this| {
                                    this.bg(SURFACE_PANEL()).shadow_sm()
                                })
                                .text_xs()
                                .text_color(if tab_installed_active {
                                    ACCENT_TEXT()
                                } else {
                                    SECONDARY_TEXT()
                                })
                                .font_weight(gpui::FontWeight::BOLD)
                                .cursor_pointer()
                                .on_mouse_down(
                                    gpui::MouseButton::Left,
                                    cx.listener(|this, _: &gpui::MouseDownEvent, _w, cx| {
                                        this.skills_market.active_tab = SkillsMarketTab::Installed;
                                        cx.notify();
                                    }),
                                )
                                .child(format!(
                                    "{} ({})",
                                    t(lang, Translations::INSTALLED),
                                    installed_count
                                )),
                        ),
                )
                .child(
                    div()
                        .id("upload-skill-btn")
                        .cursor_pointer()
                        .on_mouse_down(
                            gpui::MouseButton::Left,
                            cx.listener(|this, _: &gpui::MouseDownEvent, _window, cx| {
                                let lang = this.current_lang;
                                cx.spawn(async move |this, cx| {
                                    let path = rfd::AsyncFileDialog::new()
                                        .set_title(t(lang, Translations::UPLOAD_SKILL_PACKAGE))
                                        .add_filter("Skill", &["skill", "zip"])
                                        .pick_file()
                                        .await
                                        .map(|fh| fh.path().to_path_buf());
                                    let _ = this.update(cx, |this, cx| {
                                        if let Some(path) = path {
                                            if let Err(err) =
                                                this.skills_market.install_from_file(path)
                                            {
                                                this.skills_market.status_text = None;
                                                this.skills_market.error_text = Some(err);
                                            }
                                            cx.notify();
                                        }
                                    });
                                })
                                .detach();
                            }),
                        )
                        .px_4()
                        .py_1p5()
                        .rounded_lg()
                        .bg(BRAND_BLUE())
                        .hover(|this| this.opacity(0.9))
                        .text_xs()
                        .text_color(gpui::white())
                        .font_weight(gpui::FontWeight::BOLD)
                        .child(t(lang, Translations::UPLOAD)),
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
    const MAX_CARD_W: f32 = 320.0;
    const MAX_COLS: usize = 4;
    const CARD_H: f32 = 170.0;
    const GAP: f32 = 24.0;
    const SIDE_PAD: f32 = 40.0;
    const SIDEBAR_W: f32 = 200.0;

    let window_w: f32 = window.bounds().size.width.into();
    let mut main_w = window_w - crate::NAV_WIDTH - SIDEBAR_W - 1.0;
    if app.terminal_visible || app.sidebar_visible {
        main_w -= app.right_panel_width + 1.0;
    }
    if main_w < 320.0 {
        main_w = 320.0;
    }

    let available_w = (main_w - SIDE_PAD * 2.0).max(MIN_CARD_W);
    let cols = (((available_w + GAP) / (MIN_CARD_W + GAP)).floor() as usize).clamp(1, MAX_COLS);
    let card_w =
        ((available_w - GAP * (cols as f32 - 1.0)) / cols as f32).clamp(MIN_CARD_W, MAX_CARD_W);

    let category_items: Vec<(&'static str, &'static str)> = vec![
        (t(lang, Translations::ALL), "skill"),
        (t(lang, Translations::DEV_TOOLS), "terminal"),
        (t(lang, Translations::ANALYSIS), "activity"),
        (t(lang, Translations::DESIGN), "side-panel"),
        (t(lang, Translations::CONTENT), "one-ai"),
        (t(lang, Translations::EFFICIENCY), "cpu"),
        (t(lang, Translations::SECURITY), "capabilities"),
        (t(lang, Translations::SOCIAL), "share"),
    ];

    let render_sidebar =
        |active_idx: usize, cx: &mut Context<crate::AppState>| {
            div()
                .w(px(SIDEBAR_W))
                .h_full()
                .border_r_1()
                .border_color(BORDER_LIGHT())
                .bg(SURFACE_PANEL())
                .child(div().flex_col().gap_1().p_4().children(
                    category_items.iter().enumerate().map(|(i, (label, icon))| {
                        let idx = i;
                        let is_active = i == active_idx;
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .px_3()
                            .py_2p5()
                            .rounded_xl()
                            .when(is_active, |this| {
                                this.bg(GHOST_SURFACE_BG()).text_color(BRAND_BLUE())
                            })
                            .when(!is_active, |this| {
                                this.text_color(SECONDARY_TEXT())
                                    .hover(|this| this.bg(HOVER_BG()))
                            })
                            .cursor_pointer()
                            .on_mouse_down(
                                gpui::MouseButton::Left,
                                cx.listener(move |this, _: &gpui::MouseDownEvent, _w, cx| {
                                    this.skills_market.active_category = idx;
                                    cx.notify();
                                }),
                            )
                            .child(crate::render_icon_element(
                                icon,
                                if is_active {
                                    BRAND_BLUE()
                                } else {
                                    MUTED_TEXT()
                                },
                                14.0,
                            ))
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(if is_active {
                                        gpui::FontWeight::BOLD
                                    } else {
                                        gpui::FontWeight::MEDIUM
                                    })
                                    .child(*label),
                            )
                    }),
                ))
        };

    let render_card = |title: String, tag: &'static str, description: String, icon: AnyElement| {
        div()
            .w(px(card_w))
            .h(px(CARD_H))
            .flex_none()
            .p_6()
            .rounded_2xl()
            .border_1()
            .border_color(BORDER_LIGHT())
            .bg(SURFACE_PANEL())
            .shadow_md()
            .hover(|this| this.bg(SURFACE_ELEVATED()).shadow_lg())
            .child(
                div()
                    .flex_col()
                    .h_full()
                    .justify_between()
                    .child(
                        div()
                            .flex_col()
                            .gap_4()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        div()
                                            .size(px(34.0))
                                            .rounded_lg()
                                            .bg(CANVAS_BG())
                                            .border_1()
                                            .border_color(BORDER_LIGHT())
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(icon),
                                    )
                                    .child(
                                        div()
                                            .px_2()
                                            .py_0p5()
                                            .rounded_md()
                                            .bg(GHOST_SURFACE_BG())
                                            .text_xs()
                                            .text_color(BRAND_BLUE())
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .child(tag),
                                    ),
                            )
                            .child(
                                div()
                                    .flex_col()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_base()
                                            .text_color(PRIMARY_TEXT())
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .text_ellipsis()
                                            .child(title),
                                    )
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(SECONDARY_TEXT())
                                            .line_height(relative(1.4))
                                            .max_h(px(40.0))
                                            .overflow_hidden()
                                            .text_ellipsis()
                                            .child(description),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(div().text_xs().text_color(MUTED_TEXT()).child("v1.0.2"))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(BRAND_BLUE())
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .child("Details →"),
                            ),
                    ),
            )
    };

    let mut inner = div()
        .flex_1()
        .overflow_hidden()
        .px(px(SIDE_PAD))
        .pt_10()
        .pb_12()
        .flex_col()
        .gap_12();

    if active_tab == SkillsMarketTab::Market {
        let market_cards: Vec<(usize, &'static str, &'static str, &'static str, &'static str)> = vec![
            (1, "Alipay Integration", "FINTECH", "Best practices for integrating Alipay into products, covering online and offline scenarios.", "thems/attachment.svg"),
            (1, "React Best Practices", "DEVELOPMENT", "Advanced Vercel engineering standards for React and Next.js applications.", "thems/workspace.svg"),
            (4, "Douyin Interaction", "SOCIAL", "Assist creators in building interactive spaces meeting platform standards.", "thems/upload.svg"),
            (2, "Redis Mastery", "DATABASE", "Best practices for Redis data structures and real-time data handling.", "thems/capabilities.svg"),
            (1, "GitHub CLI Master", "GIT", "Full guide for GitHub CLI (gh), covering issues, PRs, and complex automation.", "thems/cmd.svg"),
            (3, "Web Artifacts Builder", "UIDESIGN", "Construct complex multi-component HTML/React/Tailwind artifacts.", "thems/side-panel.svg"),
            (1, "Security Audit", "SECURITY", "Safety audit recommendations for Python and Go production environments.", "thems/capabilities.svg"),
            (0, "MCP Builder", "AI INFRA", "Guide for building high-quality Model Context Protocol servers.", "thems/one-ai.svg"),
        ];

        let cards = market_cards
            .into_iter()
            .filter(|(cat, ..)| active_category == 0 || *cat == active_category)
            .collect::<Vec<_>>();

        if cards.is_empty() {
            inner = inner.child(
                div()
                    .p_12()
                    .rounded_2xl()
                    .border_1()
                    .border_dashed()
                    .border_color(BORDER_LIGHT())
                    .bg(SURFACE_PANEL())
                    .flex_col()
                    .items_center()
                    .gap_4()
                    .child(crate::render_icon_element(
                        "capabilities",
                        MUTED_TEXT(),
                        32.0,
                    ))
                    .child(
                        div()
                            .text_sm()
                            .text_color(MUTED_TEXT())
                            .child("No skills found."),
                    ),
            );
        } else {
            let mut i = 0usize;
            while i < cards.len() {
                let row_items = &cards[i..cards.len().min(i + cols)];
                let mut row = div().flex().gap(px(GAP)).mb_4();
                for (_, title, tag, desc, icon_path) in row_items {
                    let icon = svg()
                        .path(*icon_path)
                        .size(px(16.0))
                        .text_color(BRAND_BLUE())
                        .into_any_element();
                    row = row.child(render_card(title.to_string(), tag, desc.to_string(), icon));
                }
                inner = inner.child(row);
                i += cols;
            }
        }
    } else {
        // Installed logic
        let skills = state.installed.clone();
        if skills.is_empty() {
            inner = inner.child(
                div()
                    .p_12()
                    .rounded_2xl()
                    .border_1()
                    .border_dashed()
                    .border_color(BORDER_LIGHT())
                    .bg(SURFACE_PANEL())
                    .flex_col()
                    .items_center()
                    .gap_4()
                    .child(crate::render_icon_element("skill", MUTED_TEXT(), 32.0))
                    .child(
                        div()
                            .text_sm()
                            .text_color(MUTED_TEXT())
                            .child(t(lang, Translations::NO_SKILLS)),
                    ),
            );
        } else {
            let mut i = 0usize;
            while i < skills.len() {
                let row_items = &skills[i..skills.len().min(i + cols)];
                let mut row = div().flex().gap(px(GAP)).mb_4();
                for skill in row_items {
                    let skill_for_click = skill.clone();
                    let icon = crate::render_icon_element("skill", BRAND_BLUE(), 16.0);
                    row = row.child(
                        render_card(
                            skill.name.clone(),
                            "LOCAL",
                            skill.path.to_string_lossy().to_string(),
                            icon,
                        )
                        .cursor_pointer()
                        .on_mouse_down(
                            gpui::MouseButton::Left,
                            cx.listener(move |this, _: &gpui::MouseDownEvent, _w, cx| {
                                this.skills_market.selected = Some(skill_for_click.clone());
                                this.skills_market.show_detail = true;
                                cx.notify();
                            }),
                        ),
                    );
                }
                inner = inner.child(row);
                i += cols;
            }
        }
    }

    let page = div()
        .flex()
        .flex_1()
        .h_full()
        .bg(CANVAS_BG())
        .child(render_sidebar(active_category, cx))
        .child(inner);

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
                .on_mouse_down(gpui::MouseButton::Left, cx.listener(|this, _: &gpui::MouseDownEvent, _w, cx| {
                    this.skills_market.show_detail = false;
                    this.skills_market.selected = None;
                    cx.notify();
                }))
                .child(
                    div()
                        .flex_col()
                        .gap_6()
                        .w(px(600.0))
                        .p_8()
                        .bg(SURFACE_PANEL())
                        .rounded_2xl()
                        .border_1()
                        .border_color(BORDER_LIGHT())
                        .shadow_xl()
                        .on_mouse_down(gpui::MouseButton::Left, cx.listener(|_, _: &gpui::MouseDownEvent, _, _| {}))
                        .child(
                            div()
                                .flex()
                                .items_start()
                                .justify_between()
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_4()
                                        .child(
                                            div()
                                                .size(px(48.0))
                                                .rounded_xl()
                                                .bg(CANVAS_BG())
                                                .border_1()
                                                .border_color(BORDER_LIGHT())
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .child(crate::render_icon_element("skill", BRAND_BLUE(), 24.0))
                                        )
                                        .child(
                                            div()
                                                .flex_col()
                                                .child(div().text_xl().text_color(PRIMARY_TEXT()).font_weight(gpui::FontWeight::BOLD).child(title))
                                                .child(div().text_xs().text_color(MUTED_TEXT()).child("Local Capability · v1.0.0"))
                                        )
                                )
                                .child(
                                    div()
                                        .px_3()
                                        .py_1()
                                        .rounded_full()
                                        .bg(GHOST_SURFACE_BG())
                                        .text_xs()
                                        .text_color(BRAND_BLUE())
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .child("INSTALLED")
                                )
                        )
                        .child(
                            div()
                                .flex_col()
                                .gap_2()
                                .child(div().text_sm().font_weight(gpui::FontWeight::BOLD).text_color(SECONDARY_TEXT()).child("Installation Path"))
                                .child(div().p_3().rounded_lg().bg(CANVAS_BG()).border_1().border_color(BORDER_LIGHT()).text_xs().font_family("Menlo").text_color(PRIMARY_TEXT()).child(path_str))
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(SECONDARY_TEXT())
                                .line_height(relative(1.5))
                                .child("This capability allows the assistant to interact with local system tools and automation workflows. It is currently installed and ready to use in any conversation.")
                        )
                        .child(
                            div().flex().gap(px(16.0)).mt_4().child(
                                div()
                                    .flex_1()
                                    .h(px(42.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_xl()
                                    .border_1()
                                    .border_color(BORDER_LIGHT())
                                    .bg(CANVAS_BG())
                                    .cursor_pointer()
                                    .hover(|this| this.bg(HOVER_BG()))
                                    .on_mouse_down(gpui::MouseButton::Left, cx.listener(|this, _: &gpui::MouseDownEvent, _w, cx| {
                                        this.skills_market.show_detail = false;
                                        this.skills_market.selected = None;
                                        cx.notify();
                                    }))
                                    .child(t(lang, Translations::CLOSE))
                            ).child(
                                div()
                                    .flex_1()
                                    .h(px(42.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_xl()
                                    .bg(BRAND_BLUE())
                                    .cursor_pointer()
                                    .hover(|this| this.opacity(0.9))
                                    .text_color(gpui::white())
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .child("Configure Settings")
                            )
                        )
                )
                .into_any_element();

            return div()
                .relative()
                .flex()
                .flex_1()
                .child(page)
                .child(overlay)
                .into_any_element();
        }
    }

    page.into_any_element()
}
