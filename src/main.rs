use gpui::{
    point, prelude::*, px, size, App, Bounds, TitlebarOptions, WindowBounds, WindowOptions,
};
use std::sync::Arc;

use gpui_platform::application;
use settings::{KeymapFile, DEFAULT_KEYMAP_PATH};

mod agents;
mod app_state;
mod runtime;
mod assets;
mod i18n;
mod memory;
mod routing;
mod run_log;
mod sandbox;
mod services;
mod skills_market;
mod task_db;
pub(crate) mod ui_theme;
mod util;
mod workspace;
mod ui;

pub(crate) use app_state::{AppState, MainView, TerminalLine};
pub(crate) use agents::types::RequestKind;
pub(crate) use ui::*;
use services::load_config;

pub(crate) use ui_theme::{
    ACCENT_TEXT, BORDER_LIGHT, BRAND_BLUE, CANVAS_BG, MUTED_TEXT, PRIMARY_TEXT, SECONDARY_TEXT,
    SURFACE_ELEVATED, SURFACE_PANEL,
};

gpui::actions!(
    app,
    [
        OpenModelConfigDialog,
        SaveModelConfig,
        CancelModelConfig,
        SendMessage,
        ToggleLang,
        ToggleTheme,
        ExportChat,
    ]
);

pub(crate) const NAV_WIDTH: f32 = 280.0;
const DEFAULT_WINDOW_WIDTH: f32 = 1200.0;
const DEFAULT_WINDOW_HEIGHT: f32 = 760.0;
pub(crate) const TITLEBAR_HEIGHT: f32 = 44.0;

pub(crate) fn escape_visible_snippet(text: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for ch in text.chars().take(max_chars) {
        match ch {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out
}

pub(crate) fn normalize_single_line_label(text: &str) -> String {
    text.replace(['\r', '\n'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn log_think_boundary_newlines(label: &str, content: &str) {
    if !content.contains("<think>") && !content.contains("</think>") {
        return;
    }

    let mut open_index = 0usize;
    while let Some(rel) = content[open_index..].find("<think>") {
        let i = open_index + rel;
        let after = i + "<think>".len();
        let mut count = 0usize;
        for ch in content[after..].chars() {
            if ch == '\n' || ch == '\r' {
                count += 1;
            } else {
                break;
            }
        }
        let snippet = escape_visible_snippet(&content[after..], 60);
        eprintln!("[THINK-SPACING] {label} open@{i} after_newlines={count} after_snip='{snippet}'");
        open_index = after;
    }

    let mut close_index = 0usize;
    while let Some(rel) = content[close_index..].find("</think>") {
        let i = close_index + rel;
        let after = i + "</think>".len();
        let mut count = 0usize;
        for ch in content[after..].chars() {
            if ch == '\n' || ch == '\r' {
                count += 1;
            } else {
                break;
            }
        }
        let snippet = escape_visible_snippet(&content[after..], 60);
        eprintln!(
            "[THINK-SPACING] {label} close@{i} after_newlines={count} after_snip='{snippet}'"
        );
        close_index = after;
    }
}

pub(crate) fn parse_tools_from_json(json_str: &str) -> Vec<system_tools::Tool> {
    let mut tools = Vec::new();

    if json_str.is_empty() {
        return tools;
    }

    if let Ok(items) = serde_json::from_str::<Vec<String>>(json_str) {
        for item in items {
            let parts: Vec<&str> = item.splitn(2, ':').collect();
            if parts.len() == 2 {
                let action = parts[0];
                let value = parts[1];
                match action {
                    "kill" => {
                        if let Ok(pid) = value.parse::<u32>() {
                            tools.push(system_tools::Tool::KillProcess(pid));
                        }
                    }
                    "delete" => {
                        tools.push(system_tools::Tool::DeleteFile(value.to_string()));
                    }
                    "disk" => {
                        if value == "free" {
                            tools.push(system_tools::Tool::DiskFree);
                        } else {
                            tools.push(system_tools::Tool::DiskUsage(value.to_string()));
                        }
                    }
                    "list_dir" => {
                        tools.push(system_tools::Tool::ListDir(value.to_string()));
                    }
                    "list_processes" => {
                        tools.push(system_tools::Tool::ListProcesses);
                    }
                    "top_memory" => {
                        let n = value.parse().unwrap_or(10);
                        tools.push(system_tools::Tool::TopMemoryProcs(n));
                    }
                    _ => {}
                }
            }
        }
    }

    tools
}

fn main() {
    println!("ONE GUI - Starting...");

    env_logger::init();

    let config = load_config();

    let icon_path = std::path::Path::new("assets/logo.png");
    let icon_image = if icon_path.exists() {
        image::open(icon_path)
            .ok()
            .map(|img| img.to_rgba8())
            .map(|rgba| {
                let (width, height) = rgba.dimensions();
                Arc::new(image::RgbaImage::from_raw(width, height, rgba.into_raw()).unwrap())
            })
    } else {
        eprintln!("[App] Icon not found at assets/logo.png");
        None
    };

    application()
        .with_assets(assets::Assets)
        .run(move |cx: &mut App| {
            settings::init(cx);
            theme_settings::init(theme::LoadThemes::JustBase, cx);
            editor::init(cx);
            gpui_tokio::init(cx);

            cx.bind_keys(
                KeymapFile::load_asset_allow_partial_failure(DEFAULT_KEYMAP_PATH, cx)
                    .expect("failed to load default keymap"),
            );

            let bounds = Bounds::centered(
                None,
                size(px(DEFAULT_WINDOW_WIDTH), px(DEFAULT_WINDOW_HEIGHT)),
                cx,
            );
            cx.open_window(
                WindowOptions {
                    titlebar: Some(TitlebarOptions {
                        title: None,
                        appears_transparent: true,
                        traffic_light_position: if cfg!(target_os = "macos") {
                            Some(point(px(12.0), px(12.0)))
                        } else {
                            None
                        },
                    }),
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    is_resizable: true,
                    window_min_size: Some(size(px(800.0), px(600.0))),
                    icon: icon_image.clone(),
                    ..Default::default()
                },
                move |window, cx| cx.new(|cx| AppState::new(window, cx, config.clone())),
            )
            .unwrap();
            cx.activate(true);
        });
}
