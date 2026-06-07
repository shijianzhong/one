use crate::agents::core::OrchestratorEvent;

#[derive(Debug, Clone)]
pub enum GeneralAiStreamEvent {
    Delta(String),
    Finished { result: String },
    Failed { error: String },
}

#[derive(Debug)]
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

#[derive(Debug)]
pub enum OrchestratorWrapperEvent {
    Event(OrchestratorEvent),
    Finished(String),
    Failed(String),
}