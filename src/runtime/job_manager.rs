use std::collections::HashMap;
use std::sync::mpsc::{self, TryRecvError};
use std::time::Duration;

use gpui::Context;

use crate::agents::claude_code::{ClaudeStreamEvent, ClaudeCodeAgent};
use crate::agents::core::{AgentFactory, OrchestratorEvent};
use crate::agents::types::{
    ClaudeRunPanelState, ClaudeRunStatus, ClaudeRunEvent, ClaudeRunTone, SubagentMessageState,
    SubagentStatus, SubagentEventEntry, SubagentEventTone, PreviewState, PreviewStatus,
    PendingQuestion, RequestKind,
};
use crate::i18n::{t, Translations};
use crate::memory::types::ChatMessage;
use crate::run_log::{RunEvent, RunKind, RunRecorder, RunStatus};
use crate::services::api::call_chat_api_stream;
use crate::services::summarize_conversation_async;
use crate::{
    escape_visible_snippet, log_think_boundary_newlines, normalize_single_line_label,
    parse_tools_from_json, strip_think_tags, task_db, AppState,
};
use super::events::*;

pub struct JobManager {
    pub current_claude_run: Option<ClaudeRunPanelState>,
    pub next_claude_run_id: u64,
    pub request_in_flight: bool,
    pub request_status_text: Option<String>,
    pub request_kind: Option<RequestKind>,
    pub subagent_messages: HashMap<u64, SubagentMessageState>,
    /// Maps orchestrator agent_id -> subagent card run_id for live stream routing
    pub orchestrator_agent_run_map: HashMap<String, u64>,
    /// Active Claude Code question waiting for user answer (from any path)
    pub pending_claude_question: Option<crate::app_state::PendingClaudeQuestion>,
    
    pub next_general_ai_run_id: u64,
    pub general_ai_run_id: Option<u64>,
    pub general_ai_task_id: Option<usize>,
    pub general_ai_live_text: String,
    pub general_ai_show_live_bubble: bool,
    
    pub next_summarize_job_id: u64,
    pub summarize_job_id: Option<u64>,
    pub pending_confirmation_tools: Option<(Vec<system_tools::Tool>, String)>,
}

impl JobManager {
    pub fn new() -> Self {
        Self {
            current_claude_run: None,
            next_claude_run_id: 0,
            request_in_flight: false,
            request_status_text: None,
            request_kind: None,
            subagent_messages: HashMap::new(),
            orchestrator_agent_run_map: HashMap::new(),
            pending_claude_question: None,
            next_general_ai_run_id: 0,
            general_ai_run_id: None,
            general_ai_task_id: None,
            general_ai_live_text: String::new(),
            general_ai_show_live_bubble: false,
            next_summarize_job_id: 0,
            summarize_job_id: None,
            pending_confirmation_tools: None,
        }
    }

    pub(crate) fn allocate_claude_run_id(&mut self) -> u64 {
        self.next_claude_run_id += 1;
        self.next_claude_run_id
    }

    pub(crate) fn allocate_general_ai_run_id(&mut self) -> u64 {
        self.next_general_ai_run_id += 1;
        self.next_general_ai_run_id
    }

    pub(crate) fn allocate_summarize_job_id(&mut self) -> u64 {
        self.next_summarize_job_id += 1;
        self.next_summarize_job_id
    }

    pub(crate) fn set_request(&mut self, kind: RequestKind, status_text: Option<String>) {
        self.request_in_flight = true;
        self.request_kind = Some(kind);
        self.request_status_text = status_text;
    }

    pub(crate) fn clear_request(&mut self) {
        self.request_in_flight = false;
        self.request_status_text = None;
    }

    pub(crate) fn clear_request_full(&mut self) {
        self.request_in_flight = false;
        self.request_status_text = None;
        self.request_kind = None;
    }

    pub(crate) fn reset_general_ai_run(&mut self) {
        self.general_ai_run_id = None;
        self.general_ai_task_id = None;
        self.general_ai_show_live_bubble = false;
        self.general_ai_live_text.clear();
    }

    pub(crate) fn toggle_subagent_collapsed(&mut self, run_id: u64) {
        if let Some(state) = self.subagent_messages.get_mut(&run_id) {
            state.collapsed = !state.collapsed;
        }
    }

    pub(crate) fn toggle_subagent_events_collapsed(&mut self, run_id: u64) {
        if let Some(state) = self.subagent_messages.get_mut(&run_id) {
            state.events_collapsed = !state.events_collapsed;
        }
    }
}

impl AppState {
    pub(crate) fn persist_current_claude_state(&self) {
        let Some(run) = self.job_manager.current_claude_run.as_ref() else {
            return;
        };
        let Some(task_id) = run.task_id else {
            return;
        };
        let Some(workspace_id) = self.active_workspace_id else {
            return;
        };
        let task_title = self
            .workspaces
            .iter()
            .find(|w| w.id == workspace_id)
            .and_then(|w| w.tasks.iter().find(|t| t.id == task_id))
            .map(|t| t.title.clone())
            .unwrap_or_else(|| "task".to_string());
        let task_dir = self.get_task_dir_for_ids(workspace_id, task_id, &task_title);
        let meta_dir = Self::get_claude_meta_dir_for_task_dir(&task_dir);
        let _ = std::fs::create_dir_all(&meta_dir);
        let state_path = meta_dir.join("run_state.json");
        if let Ok(json) = serde_json::to_string_pretty(run) {
            let _ = std::fs::write(state_path, json);
        }
    }

    pub(crate) fn load_claude_state_for_task(
        &self,
        workspace_id: usize,
        task_id: usize,
        task_title: &str,
    ) -> Option<ClaudeRunPanelState> {
        let task_dir = self.get_task_dir_for_ids(workspace_id, task_id, task_title);
        let state_path = Self::get_claude_meta_dir_for_task_dir(&task_dir).join("run_state.json");
        let content = std::fs::read_to_string(state_path).ok()?;
        let mut state = serde_json::from_str::<ClaudeRunPanelState>(&content).ok()?;
        state.artifacts = Self::load_artifacts_for_task_dir(&task_dir);
        Some(state)
    }

    pub(crate) fn begin_claude_run(&mut self, instruction: &str) -> u64 {
        let run_id = self.job_manager.allocate_claude_run_id();
        self.job_manager.set_request(
            RequestKind::ClaudeCode,
            Some(
                t(
                    self.current_lang,
                    Translations::CLAUDE_CODE_RUNNING_ELLIPSIS,
                )
                .to_string(),
            ),
        );
        let lang = self.current_lang;
        self.job_manager.current_claude_run = Some(ClaudeRunPanelState {
            run_id,
            task_id: self.active_task_id,
            instruction: instruction.to_string(),
            work_dir: self.get_work_dir(),
            command_preview: String::new(),
            status: ClaudeRunStatus::Running,
            status_message: t(lang, Translations::WAITING_FOR_CLAUDE_START).to_string(),
            live_text: String::new(),
            final_text: None,
            stderr_lines: vec![],
            events: vec![ClaudeRunEvent::info(
                t(lang, Translations::RUN_QUEUED),
                format!(
                    "{}: {}",
                    t(lang, Translations::INSTRUCTION_SUBMITTED),
                    instruction
                ),
            )],
            show_live_bubble: true,
            preview: Some(PreviewState {
                status: PreviewStatus::Idle,
                entry_file: None,
                url: None,
                note: t(lang, Translations::PREVIEW_AFTER_RUN_NOTE).to_string(),
            }),
            session_id: None,
            artifacts: self
                .get_active_task_dir_path()
                .map(|dir| Self::load_artifacts_for_task_dir(&dir))
                .unwrap_or_default(),
            pending_question: None,
        });
        self.persist_current_claude_state();
        self.insert_subagent_message(run_id, instruction.to_string());
        run_id
    }

    pub(crate) fn spawn_claude_code_run(
        &mut self,
        instruction: String,
        session_id: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let run_id = self.begin_claude_run(&instruction);
        let project_dir =
            if let Some((workspace_id, task_id, title)) = self.get_active_task_location() {
                self.ensure_task_storage_dir(workspace_id, task_id, &title)
            } else {
                std::path::PathBuf::from(self.get_work_dir())
            };

        let log_run_id = self.active_task_id.and_then(|task_id| {
            RunRecorder::begin(
                &self.db.conn,
                task_id,
                RunKind::ClaudeCode,
                instruction.clone(),
            )
        });

        let (sender, receiver) = mpsc::channel::<ClaudeStreamEvent>();
        let worker_sender = sender.clone();
        let final_sender = sender.clone();
        let instruction_for_worker = instruction.clone();
        let session_id_for_worker = session_id.clone();
        let project_dir_for_worker = project_dir.clone();

        gpui_tokio::Tokio::spawn(cx, async move {
            let result = tokio::task::spawn_blocking(move || {
                ClaudeCodeAgent::execute_instruction_stream(
                    &project_dir_for_worker,
                    &instruction_for_worker,
                    session_id_for_worker.as_deref(),
                    worker_sender,
                )
            })
            .await;

            match result {
                Ok(Ok(output)) => {
                    let _ = final_sender.send(ClaudeStreamEvent::Finished { result: output });
                }
                Ok(Err(error)) => {
                    let _ = final_sender.send(ClaudeStreamEvent::Failed {
                        error: error.to_string(),
                    });
                }
                Err(error) => {
                    let _ = final_sender.send(ClaudeStreamEvent::Failed {
                        error: format!("Tokio join error: {}", error),
                    });
                }
            }
        })
        .detach();

        cx.spawn(async move |this, cx| loop {
            let mut disconnected = false;

            loop {
                match receiver.try_recv() {
                    Ok(event) => {
                        let _ = this.update(cx, |this, cx| {
                            if let Some(rid) = log_run_id {
                                if let Some(mapped) = map_claude_to_run_event(&event) {
                                    let recorder = RunRecorder::attach(&this.db.conn, rid);
                                    recorder.record(&mapped);
                                    match mapped {
                                        RunEvent::Finished { .. } => {
                                            recorder.finish(RunStatus::Finished);
                                        }
                                        RunEvent::Failed { .. } => {
                                            recorder.finish(RunStatus::Failed);
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            this.apply_claude_run_event(run_id, event);
                            cx.notify();
                        });
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }

            if disconnected {
                break;
            }

            cx.background_executor()
                .timer(Duration::from_millis(60))
                .await;
        })
        .detach();
    }

    pub(crate) fn apply_claude_run_event(&mut self, run_id: u64, event: ClaudeStreamEvent) {
        self.update_subagent_message_event(run_id, &event);

        let lang = self.current_lang;
        let mut final_message: Option<String> = None;
        let mut persist_task_id: Option<usize> = None;
        let mut finished_work_dir: Option<String> = None;

        {
            let Some(run) = self.job_manager.current_claude_run.as_mut() else {
                return;
            };

            if run.run_id != run_id {
                return;
            }

            match event {
                ClaudeStreamEvent::Started { command, workdir } => {
                    run.command_preview = command.clone();
                    run.work_dir = workdir.clone();
                    run.status_message = t(lang, Translations::CLAUDE_CODE_RUNNING).to_string();
                    run.events.push(ClaudeRunEvent::info(
                        t(lang, Translations::PROCESS_STARTED),
                        format!(
                            "{}: {}\n{}: {}",
                            t(lang, Translations::WORKDIR),
                            workdir,
                            t(lang, Translations::COMMAND),
                            command
                        ),
                    ));
                }
                ClaudeStreamEvent::AssistantText(text) => {
                    if run.live_text.is_empty() {
                        run.events.push(ClaudeRunEvent::info(
                            t(lang, Translations::STREAMING_RESPONSE),
                            t(lang, Translations::CLAUDE_STARTED_LIVE_CONTENT),
                        ));
                    }
                    if !run.live_text.is_empty() {
                        run.live_text.push('\n');
                    }
                    run.live_text.push_str(&text);
                    run.status_message = t(lang, Translations::GENERATING_RESPONSE).to_string();
                }
                ClaudeStreamEvent::Progress { label, detail } => {
                    run.status_message = format!("{}...", label);
                    run.events.push(ClaudeRunEvent::info(label, detail));
                }
                ClaudeStreamEvent::Stderr(line) => {
                    run.stderr_lines.push(line.clone());
                    let tone = if line.to_lowercase().contains("error") {
                        ClaudeRunTone::Error
                    } else {
                        ClaudeRunTone::Info
                    };
                    run.events.push(ClaudeRunEvent {
                        title: t(lang, Translations::STDERR).to_string(),
                        tone,
                        formatted_detail: crate::agents::types::format_event_detail(&line),
                    });
                }
                ClaudeStreamEvent::Finished { result } => {
                    run.status = ClaudeRunStatus::Completed;
                    run.status_message = t(lang, Translations::RUN_COMPLETED).to_string();
                    if run.live_text.trim().is_empty() {
                        run.live_text = result.clone();
                    }
                    run.final_text = Some(result.clone());
                    run.events.push(ClaudeRunEvent::success(
                        t(lang, Translations::RUN_COMPLETED),
                        format!(
                            "{}: {}",
                            t(lang, Translations::GENERATED_CHARACTERS),
                            result.len()
                        ),
                    ));
                    final_message = Some(result);
                    persist_task_id = run.task_id;
                    finished_work_dir = Some(run.work_dir.clone());
                    self.job_manager.clear_request();
                }
                ClaudeStreamEvent::Failed { error } => {
                    run.status = ClaudeRunStatus::Failed;
                    run.status_message = t(lang, Translations::RUN_FAILED).to_string();
                    run.events.push(ClaudeRunEvent::error(
                        t(lang, Translations::RUN_FAILED),
                        format!("{}{}", t(lang, Translations::CLAUDE_EXECUTION_ERROR), error),
                    ));
                    final_message = Some(format!(
                        "{} {} {}",
                        t(lang, Translations::CLAUDE_CODE_TAG),
                        t(lang, Translations::RUN_FAILED_TAG),
                        error
                    ));
                    persist_task_id = run.task_id;
                    self.job_manager.clear_request();
                }
                ClaudeStreamEvent::Session { session_id } => {
                    run.session_id = Some(session_id.clone());
                    run.events.push(ClaudeRunEvent::info(
                        t(lang, Translations::SESSION_UPDATED),
                        format!("{}: {}", t(lang, Translations::SESSION_ID), session_id),
                    ));
                }
                ClaudeStreamEvent::AskUserQuestion {
                    prompt,
                    options,
                } => {
                    run.status_message = t(lang, Translations::CLAUDE_WAITING_FOR_ANSWER).to_string();
                    run.events.push(ClaudeRunEvent::info(
                        t(lang, Translations::QUESTION),
                        format!("{}\n{}: {:?}", prompt, t(lang, Translations::OPTIONS), options),
                    ));
                    run.pending_question = Some(PendingQuestion {
                        prompt,
                        options,
                        session_id: run.session_id.clone(),
                    });
                    self.job_manager.request_status_text =
                        Some(t(lang, Translations::CLAUDE_WAITING_FOR_ANSWER).to_string());
                }
            }
        }

        if let Some(msg) = final_message {
            if let Some(task_id) = persist_task_id {
                let _ = task_db::insert_message(&self.db.conn, task_id, "assistant", &msg);
                if self.active_task_id == Some(task_id) {
                    self.messages.push(ChatMessage::new("assistant", &msg));
                    self.needs_auto_scroll = true;
                }
            }
        }

        if let Some(work_dir) = finished_work_dir {
            // Re-scan artifacts and try to prepare preview
            if let Some(run) = self.job_manager.current_claude_run.as_mut() {
                run.artifacts = Self::load_artifacts_for_task_dir(&std::path::PathBuf::from(&work_dir));
            }
            let res = self.try_prepare_preview(&work_dir, "");
            if let crate::agents::types::PreviewLaunchResult::Ready { url, entry_file, note } = res {
                if let Some(run) = self.job_manager.current_claude_run.as_mut() {
                    run.preview = Some(PreviewState {
                        status: PreviewStatus::Ready,
                        entry_file: Some(entry_file),
                        url: Some(url),
                        note,
                    });
                    run.events.push(ClaudeRunEvent::success(
                        t(lang, Translations::PREVIEW_READY_EVENT),
                        t(lang, Translations::OPENED_PREVIEW_URL),
                    ));
                }
            }
        }

        self.persist_current_claude_state();
    }

    pub(crate) fn update_subagent_message_event(&mut self, run_id: u64, event: &ClaudeStreamEvent) {
        let lang = self.current_lang;
        let session_id_for_question = self
            .job_manager.current_claude_run
            .as_ref()
            .and_then(|r| r.session_id.clone());

        let Some(state) = self.job_manager.subagent_messages.get_mut(&run_id) else {
            return;
        };

        match event {
            ClaudeStreamEvent::Started { command: _, workdir } => {
                state.status = SubagentStatus::Running;
                state.status_message = t(lang, Translations::CLAUDE_CODE_RUNNING).to_string();
                state.events.push(SubagentEventEntry {
                    title: t(lang, Translations::PROCESS_STARTED).to_string(),
                    detail: format!(
                        "{}: {}",
                        t(lang, Translations::WORKDIR),
                        workdir
                    ),
                    tone: SubagentEventTone::Info,
                });
            }
            ClaudeStreamEvent::AssistantText(text) => {
                if !state.live_text.is_empty() {
                    state.live_text.push('\n');
                }
                state.live_text.push_str(text);
            }
            ClaudeStreamEvent::Progress { label, detail } => {
                state.status_message = format!("{}...", label);
                state.events.push(SubagentEventEntry {
                    title: label.clone(),
                    detail: detail.clone(),
                    tone: SubagentEventTone::Info,
                });
            }
            ClaudeStreamEvent::Stderr(line) => {
                state.stderr_lines.push(line.clone());
                let tone = if line.to_lowercase().contains("error") {
                    SubagentEventTone::Error
                } else {
                    SubagentEventTone::Info
                };
                state.events.push(SubagentEventEntry {
                    title: t(lang, Translations::STDERR).to_string(),
                    detail: line.clone(),
                    tone,
                });
            }
            ClaudeStreamEvent::Finished { result } => {
                state.status = SubagentStatus::Completed;
                state.status_message = t(lang, Translations::CLAUDE_COMPLETED).to_string();
                if state.live_text.trim().is_empty() {
                    state.live_text = result.clone();
                }
            }
            ClaudeStreamEvent::Failed { error } => {
                state.status = SubagentStatus::Failed;
                state.status_message = t(lang, Translations::CLAUDE_FAILED).to_string();
                state.events.push(SubagentEventEntry {
                    title: t(lang, Translations::RUN_FAILED).to_string(),
                    detail: error.clone(),
                    tone: SubagentEventTone::Error,
                });
            }
            ClaudeStreamEvent::AskUserQuestion { prompt, options: _ } => {
                state.status_message = "Waiting for your answer...".to_string();
                state.events.push(SubagentEventEntry {
                    title: "Question".to_string(),
                    detail: prompt.clone(),
                    tone: SubagentEventTone::Info,
                });
            }
            _ => {}
        }

        if let ClaudeStreamEvent::AskUserQuestion { prompt, options } = event {
            self.job_manager.pending_claude_question = Some(crate::app_state::PendingClaudeQuestion {
                prompt: prompt.clone(),
                options: options.clone(),
                source_run_id: run_id,
                session_id: session_id_for_question,
            });
        }
    }

    pub(crate) fn continue_claude_with_answer(&mut self, answer: String, cx: &mut Context<Self>) {
        let lang = self.current_lang;
        let session_id = self
            .job_manager.current_claude_run
            .as_ref()
            .and_then(|run| run.session_id.clone());
        if let Some(run) = self.job_manager.current_claude_run.as_mut() {
            run.pending_question = None;
            run.status = ClaudeRunStatus::Running;
            run.status_message = t(lang, Translations::CONTINUING_CLAUDE_RUN).to_string();
            run.events.push(ClaudeRunEvent::info(
                t(lang, Translations::USER_ANSWERED),
                answer.clone(),
            ));
        }
        self.job_manager.set_request(
            RequestKind::ClaudeCode,
            Some(t(lang, Translations::CLAUDE_CODE_CONTINUING_ELLIPSIS).to_string()),
        );

        self.spawn_claude_code_run(answer, session_id, cx);
    }

    pub(crate) fn insert_subagent_message(&mut self, run_id: u64, instruction: String) {
        let task_id = self.active_task_id;
        self.job_manager.subagent_messages.insert(
            run_id,
            SubagentMessageState {
                instruction,
                status: SubagentStatus::Pending,
                status_message: t(self.current_lang, Translations::CLAUDE_CODE_RUNNING_ELLIPSIS)
                    .to_string(),
                live_text: String::new(),
                events: Vec::new(),
                stderr_lines: Vec::new(),
                collapsed: false,
                events_collapsed: false,
                task_id,
            },
        );
    }

    fn apply_general_ai_stream_event(
        &mut self,
        run_id: u64,
        event: GeneralAiStreamEvent,
        cx: &mut Context<Self>,
    ) {
        if self.job_manager.general_ai_run_id != Some(run_id) {
            return;
        }

        let run_task_id = self.job_manager.general_ai_task_id;
        match event {
            GeneralAiStreamEvent::Delta(delta) => {
                if self.job_manager.general_ai_live_text.is_empty() {
                    self.job_manager.request_status_text =
                        Some(t(self.current_lang, Translations::GENERATING_RESPONSE).to_string());
                }
                self.job_manager.general_ai_live_text.push_str(&delta);
                self.needs_auto_scroll = run_task_id == self.active_task_id;
            }
            GeneralAiStreamEvent::Finished { result } => {
                let content = if result.trim().is_empty() {
                    self.job_manager.general_ai_live_text.clone()
                } else {
                    result.clone()
                };

                if content.starts_with("CONFIRM_REQUIRED:") {
                    let tools_json = content.strip_prefix("CONFIRM_REQUIRED:").unwrap_or("");
                    let dangerous_msg =
                        "⚠️ 检测到危险操作：\n\n由于包含危险操作，当前已跳过执行。";

                    self.job_manager.pending_confirmation_tools = Some((Vec::new(), tools_json.to_string()));

                    if run_task_id == self.active_task_id {
                        self.messages.push(ChatMessage::new("assistant", &dangerous_msg));
                        self.needs_auto_scroll = true;
                    }

                    self.job_manager.clear_request();
                    self.job_manager.reset_general_ai_run();
                    return;
                }

                log_think_boundary_newlines("general_ai:final", &content);

                self.job_manager.clear_request_full();
                self.job_manager.reset_general_ai_run();

                if run_task_id == self.active_task_id {
                    self.messages.push(ChatMessage::new("assistant", &content));
                    self.needs_auto_scroll = true;
                }
                if let Some(task_id) = run_task_id {
                    task_db::insert_message(&self.db.conn, task_id, "assistant", &content).ok();
                }

                if self.pending_summarize && run_task_id == self.active_task_id {
                    self.pending_summarize = false;
                    self.spawn_summarize_job(cx);
                }
            }
            GeneralAiStreamEvent::Failed { error } => {
                let error_message = format!(
                    "AI request failed: {}\n\nPlease check network connectivity, Base URL, and API key.",
                    error
                );

                self.job_manager.clear_request_full();
                self.job_manager.reset_general_ai_run();

                if run_task_id == self.active_task_id {
                    self.messages.push(ChatMessage::new("assistant", &error_message));
                    self.needs_auto_scroll = true;
                }
                if let Some(task_id) = run_task_id {
                    task_db::insert_message(&self.db.conn, task_id, "assistant", &error_message)
                        .ok();
                }
            }
        }
    }

    fn spawn_summarize_job(&mut self, cx: &mut Context<Self>) {
        let task_id = self.active_task_id;
        let all_messages = self.messages.clone();
        let base_url = self.model_base_url.clone();
        let api_key = self.model_api_key.clone();
        let model = self.model_name.clone();
        let Some(tid) = task_id else {
            return;
        };

        let job_id = self.job_manager.allocate_summarize_job_id();
        self.job_manager.summarize_job_id = Some(job_id);

        let (sender, receiver) = mpsc::channel::<SummarizeEvent>();
        let sender_ok = sender.clone();
        let sender_err = sender;

        gpui_tokio::Tokio::spawn(cx, async move {
            match summarize_conversation_async(&base_url, &api_key, &model, &all_messages).await {
                Ok(summary) => {
                    let _ = sender_ok.send(SummarizeEvent::Finished {
                        job_id,
                        task_id: tid,
                        summary,
                    });
                }
                Err(error) => {
                    let _ = sender_err.send(SummarizeEvent::Failed {
                        job_id,
                        task_id: tid,
                        error,
                    });
                }
            }
        })
        .detach();

        cx.spawn(async move |this, cx| loop {
            let mut disconnected = false;

            loop {
                match receiver.try_recv() {
                    Ok(event) => {
                        let _ = this.update(cx, |this, cx| {
                            this.apply_summarize_event(event);
                            cx.notify();
                        });
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }

            if disconnected {
                break;
            }

            cx.background_executor()
                .timer(Duration::from_millis(60))
                .await;
        })
        .detach();
    }

    fn apply_summarize_event(&mut self, event: SummarizeEvent) {
        match event {
            SummarizeEvent::Finished {
                job_id,
                task_id,
                summary,
            } => {
                if self.job_manager.summarize_job_id != Some(job_id) {
                    return;
                }
                self.job_manager.summarize_job_id = None;
                let clean_sum = strip_think_tags(&summary);
                let normalized = normalize_single_line_label(&clean_sum);
                let short_title: String = normalized.chars().take(10).collect();
                task_db::update_task_title(&self.db.conn, task_id, &short_title).ok();
                for ws in &mut self.workspaces {
                    for t in &mut ws.tasks {
                        if t.id == task_id {
                            t.title = short_title.clone();
                            break;
                        }
                    }
                }
            }
            SummarizeEvent::Failed {
                job_id,
                task_id,
                error,
            } => {
                if self.job_manager.summarize_job_id != Some(job_id) {
                    return;
                }
                self.job_manager.summarize_job_id = None;
                eprintln!(
                    "[CHAT-TITLE] summarize failed task_id={} error={}",
                    task_id, error
                );
            }
        }
    }

    pub(crate) fn begin_general_ai_run(&mut self) -> u64 {
        let run_id = self.job_manager.allocate_general_ai_run_id();
        self.job_manager.set_request(
            RequestKind::GeneralAi,
            Some(t(self.current_lang, Translations::WAITING_FOR_AI_RESPONSE).to_string()),
        );
        self.job_manager.general_ai_run_id = Some(run_id);
        self.job_manager.general_ai_task_id = self.active_task_id;
        self.job_manager.general_ai_live_text.clear();
        self.job_manager.general_ai_show_live_bubble = true;
        run_id
    }

    pub(crate) fn spawn_system_tools_run(&mut self, task: String, cx: &mut Context<Self>) {
        let run_id = self.begin_general_ai_run();

        let base_url = self.model_base_url.clone();
        let api_key = self.model_api_key.clone();
        let model = self.model_name.clone();

        let (sender, receiver) = mpsc::channel::<GeneralAiStreamEvent>();
        let delta_sender = sender.clone();
        let final_sender = sender.clone();

        let task_for_async = task.clone();
        gpui_tokio::Tokio::spawn(cx, async move {
            let tools_result =
                system_tools::Tool::from_task_llm_async(&task_for_async, &base_url, &api_key, &model)
                    .await;

            let tools_with_danger = match tools_result {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("[SystemTools] LLM parsing failed: {}, falling back to keyword", e);
                    system_tools::Tool::from_task(&task_for_async)
                        .into_iter()
                        .map(|t| (t, None))
                        .collect()
                }
            };

            if system_tools::requires_confirmation(&tools_with_danger) {
                let dangerous_msg = "⚠️ 检测到危险操作：\n".to_string();
                let mut details = Vec::new();
                let mut tools_for_save = Vec::new();
                for (tool, _) in &tools_with_danger {
                    match tool {
                        system_tools::Tool::KillProcess(pid) => {
                            details.push(format!("  - 终止进程 PID={}", pid));
                            tools_for_save.push(format!("kill:{}", pid));
                        }
                        system_tools::Tool::DeleteFile(path) => {
                            details.push(format!("  - 删除文件 {}", path));
                            tools_for_save.push(format!("delete:{}", path));
                        }
                        _ => {}
                    }
                }
                let msg =
                    dangerous_msg + &details.join("\n") + "\n\n由于包含危险操作，当前已跳过执行。";

                let tools_json = serde_json::to_string(&tools_for_save).unwrap_or_default();
                let _ = delta_sender.send(GeneralAiStreamEvent::Delta(msg));
                let _ = final_sender.send(GeneralAiStreamEvent::Finished {
                    result: format!("CONFIRM_REQUIRED:{}", tools_json),
                });
                return;
            }

            let mut results = Vec::new();
            for (tool, _) in tools_with_danger {
                match tool.execute() {
                    Ok(output) => results.push(output),
                    Err(e) => results.push(format!("Error: {}", e)),
                }
            }

            let response = if results.is_empty() {
                "No operations needed.".to_string()
            } else {
                results.join("\n---\n")
            };

            let _ = delta_sender.send(GeneralAiStreamEvent::Delta(response.clone()));
            let _ = final_sender.send(GeneralAiStreamEvent::Finished { result: response });
        })
        .detach();

        self.poll_general_ai_events(run_id, receiver, cx);
    }

    pub(crate) fn confirm_system_tools_operation(
        &mut self,
        confirmed: bool,
        cx: &mut Context<Self>,
    ) {
        if !confirmed {
            self.messages.push(ChatMessage::new("assistant", "操作已取消。"));
            self.job_manager.pending_confirmation_tools = None;
            cx.notify();
            return;
        }

        let tools_data = self.job_manager.pending_confirmation_tools.take();
        if let Some((_tools, task_json)) = tools_data {
            let tools = parse_tools_from_json(&task_json);
            if tools.is_empty() {
                self.messages
                    .push(ChatMessage::new("assistant", "无法解析操作指令。"));
                cx.notify();
                return;
            }

            let run_id = self.begin_general_ai_run();

            let (sender, receiver) = mpsc::channel::<GeneralAiStreamEvent>();
            let delta_sender = sender.clone();
            let final_sender = sender.clone();

            let tools_for_async = tools;
            gpui_tokio::Tokio::spawn(cx, async move {
                let mut results = Vec::new();
                for tool in &tools_for_async {
                    match tool.execute() {
                        Ok(output) => results.push(output),
                        Err(e) => results.push(format!("Error: {}", e)),
                    }
                }

                let response = if results.is_empty() {
                    "操作完成。".to_string()
                } else {
                    results.join("\n")
                };

                let _ = delta_sender.send(GeneralAiStreamEvent::Delta(response.clone()));
                let _ = final_sender.send(GeneralAiStreamEvent::Finished { result: response });
            })
            .detach();

            self.poll_general_ai_events(run_id, receiver, cx);
        }
        cx.notify();
    }

    fn poll_general_ai_events(
        &mut self,
        run_id: u64,
        receiver: mpsc::Receiver<GeneralAiStreamEvent>,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx| loop {
            let mut disconnected = false;

            loop {
                match receiver.try_recv() {
                    Ok(event) => {
                        let _ = this.update(cx, |this, cx| {
                            this.apply_general_ai_stream_event(run_id, event, cx);
                            cx.notify();
                        });
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }

            if disconnected {
                break;
            }

            cx.background_executor()
                .timer(Duration::from_millis(60))
                .await;
        })
        .detach();
    }

    pub(crate) fn spawn_orchestrator_run(&mut self, instruction: String, cx: &mut Context<Self>) {
        let config = crate::services::load_config();
        let workspace_name = self
            .get_active_workspace()
            .map(|w| w.name.as_str())
            .unwrap_or("Default");

        let orchestrator = match AgentFactory::create_orchestrator(&config, workspace_name) {
            Ok(o) => o,
            Err(e) => {
                self.messages.push(ChatMessage::new(
                    "assistant",
                    &format!("Failed to create orchestrator: {}", e),
                ));
                return;
            }
        };

        let run_id = self.job_manager.allocate_general_ai_run_id();
        self.job_manager.set_request(
            RequestKind::GeneralAi,
            Some(t(self.current_lang, Translations::ANALYZING_INTENT).to_string()),
        );

        let log_run_id = self.active_task_id.and_then(|task_id| {
            RunRecorder::begin(
                &self.db.conn,
                task_id,
                RunKind::Orchestrator,
                instruction.clone(),
            )
        });

        let (sender, receiver) = mpsc::channel::<OrchestratorWrapperEvent>();
        let event_sender = sender.clone();
        let final_sender = sender.clone();

        let session_id = format!("orchestrator-{}", run_id);
        let instruction_for_task = instruction.clone();

        gpui_tokio::Tokio::spawn(cx, async move {
            let result = orchestrator
                .run_task(&instruction_for_task, session_id, |event| {
                    let _ = event_sender.send(OrchestratorWrapperEvent::Event(event));
                })
                .await;

            match result {
                Ok(res) => {
                    let _ = final_sender.send(OrchestratorWrapperEvent::Finished(res));
                }
                Err(e) => {
                    let _ = final_sender.send(OrchestratorWrapperEvent::Failed(e.to_string()));
                }
            }
        })
        .detach();

        cx.spawn(async move |this, cx| loop {
            let mut disconnected = false;
            loop {
                match receiver.try_recv() {
                    Ok(event) => {
                        let _ = this.update(cx, |this, cx| {
                            match event {
                                OrchestratorWrapperEvent::Event(e) => match e {
                                    OrchestratorEvent::Plan { plan } => {
                                        if let Some(rid) = log_run_id {
                                            RunRecorder::attach(&this.db.conn, rid).record(
                                                &RunEvent::MessageDelta {
                                                    text: plan.clone(),
                                                },
                                            );
                                        }
                                        this.job_manager.request_status_text = Some(format!("📋 Plan: {}", plan));
                                    }
                                    OrchestratorEvent::AssistantDelta(delta) => {
                                        this.job_manager.request_status_text = Some(delta);
                                    }
                                    OrchestratorEvent::StepStarted {
                                        agent_id,
                                        agent_name,
                                    } => {
                                        let sub_run_id = this.job_manager.next_claude_run_id + 1;
                                        this.job_manager.next_claude_run_id = sub_run_id;
                                        this.insert_subagent_message(sub_run_id, format!("{}: thinking...", agent_name));
                                        this.job_manager.orchestrator_agent_run_map.insert(agent_id.clone(), sub_run_id);
                                        this.job_manager.request_status_text =
                                            Some(format!("Agent {} is thinking...", agent_name));
                                    }
                                    OrchestratorEvent::ToolCall { name, args } => {
                                        if let Some(rid) = log_run_id {
                                            RunRecorder::attach(&this.db.conn, rid).record(
                                                &RunEvent::ToolCall {
                                                    name: name.clone(),
                                                    args: args.clone(),
                                                },
                                            );
                                        }
                                        this.job_manager.request_status_text =
                                            Some(format!("Calling tool {}...", name));
                                    }
                                    OrchestratorEvent::ToolResult { name, result } => {
                                        if let Some(rid) = log_run_id {
                                            RunRecorder::attach(&this.db.conn, rid).record(
                                                &RunEvent::ToolResult { name, result },
                                            );
                                        }
                                    }
                                    OrchestratorEvent::StepFinished { result: _ } => {}
                                    OrchestratorEvent::SubAgentStream { agent_id, event } => {
                                        if let Some(&sub_run_id) = this.job_manager.orchestrator_agent_run_map.get(&agent_id) {
                                            this.update_subagent_message_event(sub_run_id, &event);
                                        }
                                    }
                                },
                                OrchestratorWrapperEvent::Finished(result) => {
                                    if let Some(rid) = log_run_id {
                                        let recorder =
                                            RunRecorder::attach(&this.db.conn, rid);
                                        recorder.record(&RunEvent::Finished {
                                            result: result.clone(),
                                        });
                                        recorder.finish(RunStatus::Finished);
                                    }
                                    this.job_manager.orchestrator_agent_run_map.clear();
                                    this.job_manager.request_in_flight = false;
                                    this.job_manager.request_status_text = None;
                                    this.messages.push(ChatMessage::new("assistant", &result));
                                    if let Some(task_id) = this.active_task_id {
                                        task_db::insert_message(
                                            &this.db.conn,
                                            task_id,
                                            "assistant",
                                            &result,
                                        )
                                        .ok();
                                    }
                                    this.needs_auto_scroll = true;
                                }
                                OrchestratorWrapperEvent::Failed(error) => {
                                    if let Some(rid) = log_run_id {
                                        let recorder =
                                            RunRecorder::attach(&this.db.conn, rid);
                                        recorder.record(&RunEvent::Failed {
                                            error: error.clone(),
                                        });
                                        recorder.finish(RunStatus::Failed);
                                    }
                                    this.job_manager.orchestrator_agent_run_map.clear();
                                    this.job_manager.request_in_flight = false;
                                    this.job_manager.request_status_text = None;
                                    this.messages.push(ChatMessage::new(
                                        "assistant",
                                        &format!("Orchestrator failed: {}", error),
                                    ));
                                    this.needs_auto_scroll = true;
                                }
                            }
                            cx.notify();
                        });
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }
            if disconnected {
                break;
            }
            cx.background_executor()
                .timer(Duration::from_millis(60))
                .await;
        })
        .detach();
    }
}
