use serde::{Deserialize, Serialize};
use gpui::Hsla;
use crate::i18n::{Lang, Translations, t};
pub(crate) use crate::ui_theme::{
    BRAND_BLUE, ERROR_TEXT, MUTED_TEXT, SECONDARY_TEXT, SUCCESS_TEXT,
};

// ── Routing ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum RoutingDecision {
    ClaudeCode {
        instruction: String,
        session_id: Option<String>,
    },
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
    ClaudeCode,
}

// ── Claude Code run state ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClaudeRunStatus {
    Running,
    Completed,
    Failed,
}

impl ClaudeRunStatus {
    pub fn label(&self, lang: Lang) -> &'static str {
        match self {
            Self::Running => t(lang, Translations::STATUS_RUNNING),
            Self::Completed => t(lang, Translations::STATUS_COMPLETED),
            Self::Failed => t(lang, Translations::STATUS_FAILED),
        }
    }

    pub fn color(&self) -> Hsla {
        match self {
            Self::Running => BRAND_BLUE(),
            Self::Completed => SUCCESS_TEXT(),
            Self::Failed => ERROR_TEXT(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClaudeRunTone {
    Info,
    Success,
    Error,
}

impl ClaudeRunTone {
    pub fn color(&self) -> Hsla {
        match self {
            Self::Info => SECONDARY_TEXT(),
            Self::Success => SUCCESS_TEXT(),
            Self::Error => ERROR_TEXT(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FormattedContent {
    Plain(String),
    Json(String),
    Code(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeRunEvent {
    pub title: String,
    pub tone: ClaudeRunTone,
    pub formatted_detail: FormattedContent,
}

impl ClaudeRunEvent {
    pub fn info(title: impl Into<String>, detail: impl Into<String>) -> Self {
        let detail = detail.into();
        Self {
            title: title.into(),
            formatted_detail: format_event_detail(&detail),
            tone: ClaudeRunTone::Info,
        }
    }

    pub fn success(title: impl Into<String>, detail: impl Into<String>) -> Self {
        let detail = detail.into();
        Self {
            title: title.into(),
            formatted_detail: format_event_detail(&detail),
            tone: ClaudeRunTone::Success,
        }
    }

    pub fn error(title: impl Into<String>, detail: impl Into<String>) -> Self {
        let detail = detail.into();
        Self {
            title: title.into(),
            formatted_detail: format_event_detail(&detail),
            tone: ClaudeRunTone::Error,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeRunPanelState {
    pub run_id: u64,
    pub task_id: Option<usize>,
    pub instruction: String,
    pub work_dir: String,
    pub command_preview: String,
    pub status: ClaudeRunStatus,
    pub status_message: String,
    pub live_text: String,
    pub final_text: Option<String>,
    pub stderr_lines: Vec<String>,
    pub events: Vec<ClaudeRunEvent>,
    pub show_live_bubble: bool,
    pub preview: Option<PreviewState>,
    pub session_id: Option<String>,
    pub artifacts: Vec<ArtifactEntry>,
    pub pending_question: Option<PendingQuestion>,
    pub modified_files: Vec<String>,
}

// ── Subagent card (chat message rendering) ────────────────────────────────────

#[derive(Clone)]
pub struct SubagentMessageState {
    pub instruction: String,
    pub status: SubagentStatus,
    pub status_message: String,
    pub live_text: String,
    pub events: Vec<SubagentEventEntry>,
    pub stderr_lines: Vec<String>,
    pub collapsed: bool,
    pub events_collapsed: bool,
    pub task_id: Option<usize>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SubagentStatus {
    Pending,
    Running,
    Completed,
    Failed,
}

impl SubagentStatus {
    pub fn color(&self) -> Hsla {
        match self {
            Self::Pending => MUTED_TEXT(),
            Self::Running => BRAND_BLUE(),
            Self::Completed => SUCCESS_TEXT(),
            Self::Failed => ERROR_TEXT(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SubagentEventEntry {
    pub title: String,
    pub detail: String,
    pub tone: SubagentEventTone,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SubagentEventTone {
    Info,
    Error,
}

// ── Preview ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PreviewStatus {
    Idle,
    Ready,
    Failed,
}

impl PreviewStatus {
    pub fn label(&self, lang: Lang) -> &'static str {
        match self {
            Self::Idle => t(lang, Translations::PREVIEW_IDLE),
            Self::Ready => t(lang, Translations::PREVIEW_READY),
            Self::Failed => t(lang, Translations::STATUS_FAILED),
        }
    }

    pub fn color(&self) -> Hsla {
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

// ── Artifacts ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactEntry {
    pub name: String,
    pub relative_path: String,
    pub absolute_path: String,
    pub kind: String,
}

// ── Pending question (Claude Code interactive prompt) ─────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingQuestion {
    pub prompt: String,
    pub options: Vec<String>,
    pub session_id: Option<String>,
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

// ── Event detail formatter ────────────────────────────────────────────────────

pub fn format_event_detail(detail: &str) -> FormattedContent {
    let trimmed = detail.trim();

    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(trimmed) {
            if let Ok(pretty) = serde_json::to_string_pretty(&json) {
                return FormattedContent::Json(pretty);
            }
        }
    }

    let lower = trimmed.to_lowercase();
    let code_markers = [
        "<html",
        "<!doctype",
        "function ",
        "const ",
        "let ",
        "import ",
        "export ",
        "body {",
        "div {",
        "return (",
    ];
    if trimmed.contains('\n') && code_markers.iter().any(|marker| lower.contains(marker)) {
        return FormattedContent::Code(trimmed.to_string());
    }

    FormattedContent::Plain(detail.to_string())
}
