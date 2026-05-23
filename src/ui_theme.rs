#![allow(non_snake_case)]

use gpui::Hsla;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThemeMode {
    Dark,
    Light,
}

impl Default for ThemeMode {
    fn default() -> Self {
        Self::Dark
    }
}

static THEME_MODE: AtomicU8 = AtomicU8::new(0);

pub fn set_theme_mode(mode: ThemeMode) {
    THEME_MODE.store(
        match mode {
            ThemeMode::Dark => 0,
            ThemeMode::Light => 1,
        },
        Ordering::Relaxed,
    );
}

pub fn get_theme_mode() -> ThemeMode {
    match THEME_MODE.load(Ordering::Relaxed) {
        1 => ThemeMode::Light,
        _ => ThemeMode::Dark,
    }
}

pub fn NAV_BG() -> Hsla {
    match get_theme_mode() {
        ThemeMode::Dark => Hsla {
            h: 0.61,
            s: 0.48,
            l: 0.10,
            a: 1.0,
        },
        ThemeMode::Light => Hsla {
            h: 0.61,
            s: 0.24,
            l: 0.965,
            a: 1.0,
        },
    }
}

pub fn CARD_BG() -> Hsla {
    match get_theme_mode() {
        ThemeMode::Dark => Hsla {
            h: 0.61,
            s: 0.36,
            l: 0.08,
            a: 1.0,
        },
        ThemeMode::Light => Hsla {
            h: 0.61,
            s: 0.18,
            l: 0.985,
            a: 1.0,
        },
    }
}

pub fn PRIMARY_TEXT() -> Hsla {
    match get_theme_mode() {
        ThemeMode::Dark => Hsla {
            h: 0.61,
            s: 0.78,
            l: 0.93,
            a: 1.0,
        },
        ThemeMode::Light => Hsla {
            h: 0.61,
            s: 0.25,
            l: 0.12,
            a: 1.0,
        },
    }
}

pub fn SECONDARY_TEXT() -> Hsla {
    match get_theme_mode() {
        ThemeMode::Dark => Hsla {
            h: 0.61,
            s: 0.26,
            l: 0.76,
            a: 1.0,
        },
        ThemeMode::Light => Hsla {
            h: 0.61,
            s: 0.16,
            l: 0.34,
            a: 1.0,
        },
    }
}

pub fn TERTIARY_TEXT() -> Hsla {
    match get_theme_mode() {
        ThemeMode::Dark => Hsla {
            h: 0.61,
            s: 0.16,
            l: 0.61,
            a: 1.0,
        },
        ThemeMode::Light => Hsla {
            h: 0.61,
            s: 0.12,
            l: 0.48,
            a: 1.0,
        },
    }
}

pub fn MUTED_TEXT() -> Hsla {
    match get_theme_mode() {
        ThemeMode::Dark => Hsla {
            h: 0.61,
            s: 0.14,
            l: 0.50,
            a: 1.0,
        },
        ThemeMode::Light => Hsla {
            h: 0.61,
            s: 0.10,
            l: 0.56,
            a: 1.0,
        },
    }
}

pub fn BRAND_BLUE() -> Hsla {
    match get_theme_mode() {
        ThemeMode::Dark => Hsla {
            h: 0.61,
            s: 0.83,
            l: 0.54,
            a: 1.0,
        },
        ThemeMode::Light => Hsla {
            h: 0.61,
            s: 0.83,
            l: 0.48,
            a: 1.0,
        },
    }
}

pub fn BORDER_LIGHT() -> Hsla {
    match get_theme_mode() {
        ThemeMode::Dark => Hsla {
            h: 0.61,
            s: 0.22,
            l: 0.24,
            a: 1.0,
        },
        ThemeMode::Light => Hsla {
            h: 0.61,
            s: 0.14,
            l: 0.84,
            a: 1.0,
        },
    }
}

pub fn ACTIVE_BG() -> Hsla {
    match get_theme_mode() {
        ThemeMode::Dark => Hsla {
            h: 0.61,
            s: 0.39,
            l: 0.20,
            a: 1.0,
        },
        ThemeMode::Light => Hsla {
            h: 0.61,
            s: 0.55,
            l: 0.925,
            a: 1.0,
        },
    }
}

pub fn WORKSPACE_BG() -> Hsla {
    match get_theme_mode() {
        ThemeMode::Dark => Hsla {
            h: 0.61,
            s: 0.36,
            l: 0.12,
            a: 1.0,
        },
        ThemeMode::Light => Hsla {
            h: 0.61,
            s: 0.16,
            l: 0.972,
            a: 1.0,
        },
    }
}

pub fn CANVAS_BG() -> Hsla {
    match get_theme_mode() {
        ThemeMode::Dark => Hsla {
            h: 0.61,
            s: 0.34,
            l: 0.10,
            a: 1.0,
        },
        ThemeMode::Light => Hsla {
            h: 0.61,
            s: 0.14,
            l: 0.988,
            a: 1.0,
        },
    }
}

pub fn SURFACE_ELEVATED() -> Hsla {
    match get_theme_mode() {
        ThemeMode::Dark => Hsla {
            h: 0.61,
            s: 0.28,
            l: 0.16,
            a: 1.0,
        },
        ThemeMode::Light => Hsla {
            h: 0.61,
            s: 0.12,
            l: 0.95,
            a: 1.0,
        },
    }
}

pub fn SURFACE_ACCENT() -> Hsla {
    match get_theme_mode() {
        ThemeMode::Dark => Hsla {
            h: 0.61,
            s: 0.46,
            l: 0.21,
            a: 1.0,
        },
        ThemeMode::Light => Hsla {
            h: 0.61,
            s: 0.28,
            l: 0.92,
            a: 1.0,
        },
    }
}

pub fn SURFACE_PANEL() -> Hsla {
    match get_theme_mode() {
        ThemeMode::Dark => Hsla {
            h: 0.61,
            s: 0.26,
            l: 0.13,
            a: 1.0,
        },
        ThemeMode::Light => Hsla {
            h: 0.61,
            s: 0.12,
            l: 0.975,
            a: 1.0,
        },
    }
}

pub fn USER_BUBBLE_BG() -> Hsla {
    match get_theme_mode() {
        ThemeMode::Dark => Hsla {
            h: 0.61,
            s: 0.76,
            l: 0.50,
            a: 1.0,
        },
        ThemeMode::Light => Hsla {
            h: 0.61,
            s: 0.72,
            l: 0.50,
            a: 1.0,
        },
    }
}

pub fn ASSISTANT_BUBBLE_BG() -> Hsla {
    match get_theme_mode() {
        ThemeMode::Dark => Hsla {
            h: 0.61,
            s: 0.24,
            l: 0.145,
            a: 1.0,
        },
        ThemeMode::Light => Hsla {
            h: 0.61,
            s: 0.10,
            l: 0.972,
            a: 1.0,
        },
    }
}

pub fn INPUT_BG() -> Hsla {
    match get_theme_mode() {
        ThemeMode::Dark => Hsla {
            h: 0.61,
            s: 0.25,
            l: 0.18,
            a: 1.0,
        },
        ThemeMode::Light => Hsla {
            h: 0.61,
            s: 0.10,
            l: 0.96,
            a: 1.0,
        },
    }
}

pub fn HEADER_BG() -> Hsla {
    match get_theme_mode() {
        ThemeMode::Dark => Hsla {
            h: 0.61,
            s: 0.40,
            l: 0.11,
            a: 0.88,
        },
        ThemeMode::Light => Hsla {
            h: 0.61,
            s: 0.20,
            l: 0.99,
            a: 0.95,
        },
    }
}

pub fn FLOATING_PANEL_BG() -> Hsla {
    match get_theme_mode() {
        ThemeMode::Dark => Hsla {
            h: 0.61,
            s: 0.28,
            l: 0.14,
            a: 0.96,
        },
        ThemeMode::Light => Hsla {
            h: 0.61,
            s: 0.12,
            l: 0.985,
            a: 0.98,
        },
    }
}

pub fn GHOST_SURFACE_BG() -> Hsla {
    match get_theme_mode() {
        ThemeMode::Dark => Hsla {
            h: 0.61,
            s: 0.26,
            l: 0.18,
            a: 0.70,
        },
        ThemeMode::Light => Hsla {
            h: 0.61,
            s: 0.12,
            l: 0.94,
            a: 0.85,
        },
    }
}

pub fn ACCENT_TEXT() -> Hsla {
    match get_theme_mode() {
        ThemeMode::Dark => Hsla {
            h: 0.61,
            s: 0.86,
            l: 0.84,
            a: 1.0,
        },
        ThemeMode::Light => Hsla {
            h: 0.61,
            s: 0.84,
            l: 0.40,
            a: 1.0,
        },
    }
}

pub fn AVATAR_BG() -> Hsla {
    match get_theme_mode() {
        ThemeMode::Dark => Hsla {
            h: 0.59,
            s: 0.86,
            l: 0.44,
            a: 1.0,
        },
        ThemeMode::Light => Hsla {
            h: 0.59,
            s: 0.82,
            l: 0.48,
            a: 1.0,
        },
    }
}
