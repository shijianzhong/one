use crate::agents::claude_code::ClaudeStreamEvent;
use crate::agents::core::OrchestratorEvent;
use crate::run_log::RunEvent;

#[derive(Debug, Clone)]
pub enum GeneralAiStreamEvent {
    Delta(String),
    Finished { result: String },
    Failed { error: String },
}

#[derive(Debug, Clone)]
pub enum SummarizeEvent {
    Finished {
        job_id: u64,
        task_id: usize,
        summary: String,
    },
    Failed {
        job_id: u64,
        task_id: usize,
        error: String,
    },
}

#[derive(Debug, Clone)]
pub enum OrchestratorWrapperEvent {
    Event(OrchestratorEvent),
    Finished(String),
    Failed(String),
}

pub fn map_claude_to_run_event(event: &ClaudeStreamEvent) -> Option<RunEvent> {
    match event {
        ClaudeStreamEvent::Started { command, workdir } => Some(RunEvent::Started {
            kind: "claude_code".to_string(),
            detail: format!("{} (cwd={})", command, workdir),
        }),
        ClaudeStreamEvent::AssistantText(text) => Some(RunEvent::MessageDelta {
            text: text.clone(),
        }),
        ClaudeStreamEvent::Progress { label, detail } => Some(RunEvent::ToolCall {
            name: label.clone(),
            args: detail.clone(),
        }),
        ClaudeStreamEvent::AskUserQuestion { prompt, options } => {
            Some(RunEvent::ApprovalRequired {
                prompt: prompt.clone(),
                options: options.clone(),
            })
        }
        ClaudeStreamEvent::Finished { result } => Some(RunEvent::Finished {
            result: result.clone(),
        }),
        ClaudeStreamEvent::Failed { error } => Some(RunEvent::Failed {
            error: error.clone(),
        }),
        ClaudeStreamEvent::Stderr(_) | ClaudeStreamEvent::Session { .. } | ClaudeStreamEvent::ModifiedFiles { .. } => None,
    }
}
