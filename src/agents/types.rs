use serde::{Deserialize, Serialize};

// ── Routing ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum RoutingDecision {
    SystemTools {
        task: String,
    },
    GeneralAI {
        messages: Vec<crate::memory::types::ChatMessage>,
    },
}

// ── Request kind (used by AppState to track what's running) ──────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RequestKind {
    GeneralAi,
}

// ── Preview ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PreviewStatus {
    Idle,
    Ready,
    Failed,
}

impl PreviewStatus {
    pub fn label(&self, lang: crate::i18n::Lang) -> &'static str {
        use crate::i18n::{t, Translations};
        match self {
            Self::Idle => t(lang, Translations::PREVIEW_IDLE),
            Self::Ready => t(lang, Translations::PREVIEW_READY),
            Self::Failed => t(lang, Translations::STATUS_FAILED),
        }
    }

    pub fn color(&self) -> gpui::Hsla {
        use crate::ui_theme::{ERROR_TEXT, MUTED_TEXT, SUCCESS_TEXT};
        match self {
            Self::Idle => MUTED_TEXT(),
            Self::Ready => SUCCESS_TEXT(),
            Self::Failed => ERROR_TEXT(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewState {
    pub status: PreviewStatus,
    pub entry_file: Option<String>,
    pub url: Option<String>,
    pub note: String,
}

#[derive(Debug, Clone)]
pub enum PreviewLaunchResult {
    Ready {
        url: String,
        entry_file: String,
        note: String,
    },
    NotFound {
        note: String,
    },
    Failed {
        note: String,
    },
}

// ── Process display (used by SystemAgent tool results) ────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessDisplayInfo {
    pub name: String,
    pub pid: u32,
    pub cpu_percent: f64,
    pub memory_mb: f64,
    pub is_critical: bool,
}

pub fn try_parse_process_list(content: &str) -> Option<Vec<ProcessDisplayInfo>> {
    let trimmed = content.trim();
    if !trimmed.starts_with('[') {
        return None;
    }
    let Ok(parsed) = serde_json::from_str::<Vec<serde_json::Value>>(trimmed) else {
        return None;
    };
    if parsed.is_empty() {
        return None;
    }
    let has_expected_fields = parsed.iter().all(|v| {
        v.get("pid").is_some()
            && v.get("name").is_some()
            && v.get("cpu_percent").is_some()
            && v.get("memory_mb").is_some()
    });
    if !has_expected_fields {
        return None;
    }
    let processes: Vec<ProcessDisplayInfo> = parsed
        .iter()
        .filter_map(|v| {
            let name = v.get("name")?.as_str()?.to_string();
            let pid = v.get("pid")?.as_u64()?.try_into().ok()?;
            let cpu_percent = v.get("cpu_percent")?.as_f64()?;
            let memory_mb = v.get("memory_mb")?.as_f64()?;
            let is_critical = cpu_percent > 60.0;
            Some(ProcessDisplayInfo {
                name,
                pid,
                cpu_percent,
                memory_mb,
                is_critical,
            })
        })
        .collect();
    if processes.len() < 3 {
        return None;
    }
    Some(processes)
}