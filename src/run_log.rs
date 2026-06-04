#![allow(dead_code)]

//! Unified RunEvent log for agent task executions.
//!
//! All agent runtimes (Claude Code, system tools, general AI, orchestrator)
//! should emit events through [`RunRecorder`] so the UI and persistence layer
//! consume a single shape. Each event is appended to `run_events` in the
//! task database; UI layers may also subscribe to events live (future work).

use serde::{Deserialize, Serialize};
use sqlez::connection::Connection;

use crate::task_db;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RunEvent {
    Started {
        kind: String,
        detail: String,
    },
    MessageDelta {
        text: String,
    },
    ToolCall {
        name: String,
        args: String,
    },
    ToolResult {
        name: String,
        result: String,
    },
    ApprovalRequired {
        prompt: String,
        options: Vec<String>,
    },
    ArtifactCreated {
        path: String,
        kind: String,
    },
    Finished {
        result: String,
    },
    Failed {
        error: String,
    },
}

impl RunEvent {
    pub fn kind_str(&self) -> &'static str {
        match self {
            Self::Started { .. } => "started",
            Self::MessageDelta { .. } => "message_delta",
            Self::ToolCall { .. } => "tool_call",
            Self::ToolResult { .. } => "tool_result",
            Self::ApprovalRequired { .. } => "approval_required",
            Self::ArtifactCreated { .. } => "artifact_created",
            Self::Finished { .. } => "finished",
            Self::Failed { .. } => "failed",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum RunKind {
    ClaudeCode,
    SystemTools,
    GeneralAi,
    Orchestrator,
}

impl RunKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            RunKind::ClaudeCode => "claude_code",
            RunKind::SystemTools => "system_tools",
            RunKind::GeneralAi => "general_ai",
            RunKind::Orchestrator => "orchestrator",
        }
    }
}

/// Lightweight handle that owns a `task_runs.id` and appends events to it.
///
/// Construction is cheap and infallible from caller's perspective: if the
/// underlying database write fails (e.g. closed connection), recording is
/// silently skipped so existing UI behaviour is never blocked by audit logging.
pub struct RunRecorder<'a> {
    conn: &'a Connection,
    run_id: Option<usize>,
}

impl<'a> RunRecorder<'a> {
    pub fn start(conn: &'a Connection, task_id: usize, kind: RunKind) -> Self {
        let run_id = task_db::insert_task_run(conn, task_id, kind.as_str()).ok();
        Self { conn, run_id }
    }

    /// Re-attach to an existing `task_runs.id` for incremental event recording.
    pub fn attach(conn: &'a Connection, run_id: usize) -> Self {
        Self {
            conn,
            run_id: Some(run_id),
        }
    }

    /// Open a recorder, emit a `Started` event, and return the persisted
    /// `task_runs.id` so callers can carry it across async boundaries.
    pub fn begin(
        conn: &'a Connection,
        task_id: usize,
        kind: RunKind,
        detail: impl Into<String>,
    ) -> Option<usize> {
        let recorder = Self::start(conn, task_id, kind);
        recorder.record(&RunEvent::Started {
            kind: kind.as_str().to_string(),
            detail: detail.into(),
        });
        recorder.run_id()
    }

    pub fn run_id(&self) -> Option<usize> {
        self.run_id
    }

    pub fn record(&self, event: &RunEvent) {
        let Some(run_id) = self.run_id else { return };
        let payload = match serde_json::to_string(event) {
            Ok(s) => s,
            Err(_) => return,
        };
        let _ = task_db::append_run_event(self.conn, run_id, event.kind_str(), &payload);
    }

    pub fn finish(self, status: RunStatus) {
        let Some(run_id) = self.run_id else { return };
        let _ = task_db::finish_task_run(self.conn, run_id, status.as_str());
    }
}

#[derive(Debug, Clone, Copy)]
pub enum RunStatus {
    Finished,
    Failed,
    Cancelled,
}

impl RunStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            RunStatus::Finished => "finished",
            RunStatus::Failed => "failed",
            RunStatus::Cancelled => "cancelled",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_event_serializes_with_tag() {
        let evt = RunEvent::ToolCall {
            name: "list_processes".into(),
            args: "{}".into(),
        };
        let json = serde_json::to_string(&evt).unwrap();
        assert!(json.contains("\"type\":\"tool_call\""));
        assert!(json.contains("\"name\":\"list_processes\""));
    }

    #[test]
    fn kind_str_matches_variant() {
        assert_eq!(
            RunEvent::Started {
                kind: "x".into(),
                detail: "y".into()
            }
            .kind_str(),
            "started"
        );
        assert_eq!(
            RunEvent::Finished {
                result: "ok".into()
            }
            .kind_str(),
            "finished"
        );
    }
}
