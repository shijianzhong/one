use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use gpui::Context;

use super::events::{GeneralAiStreamEvent, OrchestratorWrapperEvent, SummarizeEvent};
use crate::agents::core::factory::AgentFactory;
use crate::agents::core::OrchestratorEvent;
use crate::agents::types::RequestKind;
use crate::i18n::{t, Translations};
use crate::memory::types::ChatMessage;
use crate::run_log::{RunEvent, RunKind, RunRecorder, RunStatus};
use crate::sandbox::backend::{Backend, SandboxBackend};
use crate::services::summarize_conversation_async;
use crate::{
    log_think_boundary_newlines, normalize_single_line_label, parse_tools_from_json,
    strip_think_tags, task_db, AppState, TerminalLine,
};

pub struct JobManager {
    pub request_in_flight: bool,
    pub request_status_text: Option<String>,
    pub request_kind: Option<RequestKind>,

    pub next_general_ai_run_id: u64,
    pub general_ai_run_id: Option<u64>,
    pub general_ai_task_id: Option<usize>,
    pub general_ai_live_text: String,
    pub general_ai_show_live_bubble: bool,

    pub next_summarize_job_id: u64,
    pub summarize_job_id: Option<u64>,
    pub pending_confirmation_tools: Option<(Vec<system_tools::Tool>, String)>,

    /// 取消标志：停止按钮触发后设为 true
    pub cancel_flag: Arc<AtomicBool>,
    /// Orchestrator 等待用户输入时的通道（发送端，由 route_message 使用）
    pub orchestrator_user_input_tx: Option<tokio::sync::mpsc::UnboundedSender<String>>,
}

impl JobManager {
    pub fn new() -> Self {
        Self {
            request_in_flight: false,
            request_status_text: None,
            request_kind: None,
            next_general_ai_run_id: 0,
            general_ai_run_id: None,
            general_ai_task_id: None,
            general_ai_live_text: String::new(),
            general_ai_show_live_bubble: false,
            next_summarize_job_id: 0,
            summarize_job_id: None,
            pending_confirmation_tools: None,
            cancel_flag: Arc::new(AtomicBool::new(false)),
            orchestrator_user_input_tx: None,
        }
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
}

impl AppState {
    fn apply_general_ai_stream_event(
        &mut self,
        run_id: u64,
        event: GeneralAiStreamEvent,
        cx: &mut Context<Self>,
    ) {
        if self.job_manager.general_ai_run_id != Some(run_id) {
            return;
        }

        let Some(run_task_id) = self.job_manager.general_ai_task_id else {
            return;
        };
        let current_active_id = self.active_task_id;
        match event {
            GeneralAiStreamEvent::Delta(delta) => {
                if self.job_manager.general_ai_live_text.is_empty() {
                    self.job_manager.request_status_text =
                        Some(t(self.current_lang, Translations::GENERATING_RESPONSE).to_string());
                }
                self.job_manager.general_ai_live_text.push_str(&delta);
            }
            GeneralAiStreamEvent::Finished { result } => {
                let content = if result.trim().is_empty() {
                    self.job_manager.general_ai_live_text.clone()
                } else {
                    result.clone()
                };

                if content.starts_with("CONFIRM_REQUIRED:") {
                    let tools_json = content.strip_prefix("CONFIRM_REQUIRED:").unwrap_or("");
                    let dangerous_msg = "⚠️ 检测到危险操作：\n\n由于包含危险操作，当前已跳过执行。";

                    self.job_manager.pending_confirmation_tools =
                        Some((Vec::new(), tools_json.to_string()));

                    if let Some(task) = self.task_mut(Some(run_task_id)) {
                        task.messages
                            .push(ChatMessage::new("assistant", &dangerous_msg));
                        if run_task_id == current_active_id.unwrap_or(0) {
                            task.needs_auto_scroll = true;
                        }
                    }

                    self.job_manager.clear_request();
                    self.job_manager.reset_general_ai_run();
                    return;
                }

                log_think_boundary_newlines("general_ai:final", &content);

                self.job_manager.clear_request_full();
                self.job_manager.reset_general_ai_run();

                if let Some(task) = self.task_mut(Some(run_task_id)) {
                    task.messages.push(ChatMessage::new("assistant", &content));
                    if run_task_id == current_active_id.unwrap_or(0) {
                        task.needs_auto_scroll = true;
                    }
                }
                task_db::insert_message(&self.db.conn, run_task_id, "assistant", &content).ok();

                if let Some(task) = self.task_mut(Some(run_task_id)) {
                    eprintln!(
                        "[SUMMARIZE] GeneralAi Finished: pending_summarize={:?}",
                        task.pending_summarize
                    );
                    if task.pending_summarize {
                        task.pending_summarize = false;
                        self.spawn_summarize_job(run_task_id, cx);
                    }
                }
            }
            GeneralAiStreamEvent::Failed { error } => {
                let error_message = format!(
                    "AI request failed: {}\n\nPlease check network connectivity, Base URL, and API key.",
                    error
                );

                self.job_manager.clear_request_full();
                self.job_manager.reset_general_ai_run();

                if let Some(task) = self.task_mut(Some(run_task_id)) {
                    task.messages
                        .push(ChatMessage::new("assistant", &error_message));
                    if run_task_id == current_active_id.unwrap_or(0) {
                        task.needs_auto_scroll = true;
                    }
                }
                task_db::insert_message(&self.db.conn, run_task_id, "assistant", &error_message)
                    .ok();
            }
        }
    }

    fn spawn_summarize_job(&mut self, task_id: usize, cx: &mut Context<Self>) {
        let all_messages = self
            .task_mut(Some(task_id))
            .map(|t| t.messages.clone())
            .unwrap_or_default();
        let msg_count = all_messages.len();
        eprintln!(
            "[SUMMARIZE] spawn_summarize_job called: task_id={}, messages_count={}",
            task_id, msg_count
        );
        let base_url = self.model_base_url.clone();
        let api_key = self.model_api_key.clone();
        let model = self.model_name.clone();
        let job_id = self.job_manager.allocate_summarize_job_id();
        self.job_manager.summarize_job_id = Some(job_id);

        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<SummarizeEvent>();
        let sender_ok = sender.clone();
        let sender_err = sender;

        gpui_tokio::Tokio::spawn(cx, async move {
            match summarize_conversation_async(&base_url, &api_key, &model, &all_messages).await {
                Ok(summary) => {
                    let _ = sender_ok.send(SummarizeEvent::Finished {
                        job_id,
                        task_id,
                        summary,
                    });
                }
                Err(error) => {
                    let _ = sender_err.send(SummarizeEvent::Failed {
                        job_id,
                        task_id,
                        error,
                    });
                }
            }
        })
        .detach();

        cx.spawn(async move |this, cx| {
            while let Some(event) = receiver.recv().await {
                let _ = this.update(cx, |this, cx| {
                    this.apply_summarize_event(event);
                    cx.notify();
                });
            }
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
                eprintln!(
                    "[SUMMARIZE] Finished event job_id={}, task_id={}, expected_job_id={:?}",
                    job_id, task_id, self.job_manager.summarize_job_id
                );
                if self.job_manager.summarize_job_id != Some(job_id) {
                    eprintln!(
                        "[SUMMARIZE] Skipping stale summarize job job_id={} (expected {:?})",
                        job_id, self.job_manager.summarize_job_id
                    );
                    return;
                }
                self.job_manager.summarize_job_id = None;
                let clean_sum = strip_think_tags(&summary);
                let normalized = normalize_single_line_label(&clean_sum);
                let short_title: String = normalized.chars().take(10).collect();
                eprintln!(
                    "[SUMMARIZE] Updating title task_id={} -> '{}'",
                    task_id, short_title
                );
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
                eprintln!(
                    "[SUMMARIZE] Failed event job_id={}, task_id={}, error={}, expected_job_id={:?}",
                    job_id, task_id, error, self.job_manager.summarize_job_id
                );
                if self.job_manager.summarize_job_id != Some(job_id) {
                    eprintln!(
                        "[SUMMARIZE] Skipping stale summarize failure job_id={} (expected {:?})",
                        job_id, self.job_manager.summarize_job_id
                    );
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

        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel::<GeneralAiStreamEvent>();
        let delta_sender = sender.clone();
        let final_sender = sender;

        let task_for_async = task.clone();
        gpui_tokio::Tokio::spawn(cx, async move {
            let tools_result = system_tools::Tool::from_task_llm_async(
                &task_for_async,
                &base_url,
                &api_key,
                &model,
            )
            .await;

            let tools_with_danger = match tools_result {
                Ok(t) => t,
                Err(e) => {
                    eprintln!(
                        "[SystemTools] LLM parsing failed: {}, falling back to keyword",
                        e
                    );
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
            if let Some(task) = self.active_task_mut() {
                task.messages
                    .push(ChatMessage::new("assistant", "操作已取消。"));
            }
            self.job_manager.pending_confirmation_tools = None;
            cx.notify();
            return;
        }

        let tools_data = self.job_manager.pending_confirmation_tools.take();
        if let Some((_tools, task_json)) = tools_data {
            let tools = parse_tools_from_json(&task_json);
            if tools.is_empty() {
                if let Some(task) = self.active_task_mut() {
                    task.messages
                        .push(ChatMessage::new("assistant", "无法解析操作指令。"));
                }
                cx.notify();
                return;
            }

            let run_id = self.begin_general_ai_run();

            let (sender, receiver) = tokio::sync::mpsc::unbounded_channel::<GeneralAiStreamEvent>();
            let delta_sender = sender.clone();
            let final_sender = sender;

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
        mut receiver: tokio::sync::mpsc::UnboundedReceiver<GeneralAiStreamEvent>,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            while let Some(event) = receiver.recv().await {
                let _ = this.update(cx, |this, cx| {
                    this.apply_general_ai_stream_event(run_id, event, cx);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// 停止当前所有正在运行的任务（Orchestrator）
    pub(crate) fn cancel_current_run(&mut self, cx: &mut Context<Self>) {
        // 1. 设置取消标志
        self.job_manager
            .cancel_flag
            .store(true, std::sync::atomic::Ordering::SeqCst);

        // 2. 清理所有请求状态
        self.job_manager.clear_request_full();
        self.job_manager.reset_general_ai_run();
        self.job_manager.orchestrator_user_input_tx = None;
        let attached_session_id = self.active_task_id.and_then(|task_id| {
            self.coding_sessions
                .lock()
                .ok()
                .and_then(|sessions| sessions.attached_session_id_for_task(task_id))
        });
        if let Some(session_id) = attached_session_id {
            let _ = self
                .coding_sessions
                .lock()
                .map(|mut sessions| sessions.stop_session(&self.db.conn, &session_id));
        }

        // 3. 标记当前 task 不活跃
        if let Some(tid) = self.active_task_id {
            self.mark_task_inactive(Some(tid));
        }

        cx.notify();
    }

    pub(crate) fn spawn_orchestrator_run(&mut self, instruction: String, cx: &mut Context<Self>) {
        let config = crate::services::load_config();
        let workspace_name = match self.get_active_workspace() {
            Some(w) => w.name.clone(),
            None => "Default".to_string(),
        };

        let workspace_root = self
            .get_active_workspace()
            .map(|w| w.path.clone())
            .unwrap_or_else(|| self.default_work_dir.clone());

        let orchestrator = match AgentFactory::create_orchestrator(
            &config,
            &workspace_name,
            workspace_root,
            self.mcp_manager.clone(),
        ) {
            Ok(o) => o,
            Err(e) => {
                if let Some(task) = self.active_task_mut() {
                    task.messages.push(ChatMessage::new(
                        "assistant",
                        &format!("Failed to create orchestrator: {}", e),
                    ));
                }
                return;
            }
        };

        let run_id = self.job_manager.allocate_general_ai_run_id();
        self.job_manager.general_ai_run_id = Some(run_id);
        self.job_manager.general_ai_show_live_bubble = true;
        self.job_manager.general_ai_task_id = self.active_task_id;
        self.job_manager.general_ai_live_text.clear();
        self.job_manager.set_request(
            RequestKind::GeneralAi,
            Some(t(self.current_lang, Translations::ANALYZING_INTENT).to_string()),
        );

        // 标记当前 task 为运行中
        let spawn_task_id = self.active_task_id;
        if let Some(tid) = spawn_task_id {
            self.mark_task_active(tid);
        }

        let log_run_id = self.active_task_id.and_then(|task_id| {
            RunRecorder::begin(
                &self.db.conn,
                task_id,
                RunKind::Orchestrator,
                instruction.clone(),
            )
        });

        let (sender, mut receiver) =
            tokio::sync::mpsc::unbounded_channel::<OrchestratorWrapperEvent>();
        let event_sender = sender.clone();
        let final_sender = sender;

        use std::time::{SystemTime, UNIX_EPOCH};
        let session_id = format!(
            "session-{}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_micros(),
            run_id,
        );
        let instruction_for_task = instruction.clone();
        let history = self
            .active_task_ref()
            .map(|t| t.messages.clone())
            .unwrap_or_default();

        let workspace_name_for_orchestrator = workspace_name.clone();
        let workspace_name_for_snapshot = workspace_name;
        let active_task_id = self.active_task_id;
        // 每次创建独立 cancel_flag，避免旧 Orchestrator 的取消状态污染新 Orchestrator
        let cancel_flag = Arc::new(AtomicBool::new(false));
        self.job_manager.cancel_flag = cancel_flag.clone();

        // ── 创建用户输入通道（支持多轮交互） ──────────────────────
        let (user_input_tx, mut user_input_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        self.job_manager.orchestrator_user_input_tx = Some(user_input_tx);

        gpui_tokio::Tokio::spawn(cx, async move {
            let result = orchestrator
                .run_task(
                    &instruction_for_task,
                    session_id,
                    history,
                    &workspace_name_for_orchestrator,
                    active_task_id,
                    Some(cancel_flag),
                    Some(user_input_rx),
                    |event| {
                        let _ = event_sender.send(OrchestratorWrapperEvent::Event(event));
                    },
                )
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

        cx.spawn(async move |this, cx| {
            while let Some(event) = receiver.recv().await {
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
                                this.job_manager.general_ai_live_text.push_str(&delta);
                                this.job_manager.request_status_text =
                                    Some(t(this.current_lang, Translations::GENERATING_RESPONSE).to_string());
                                if let Some(task) = this.task_mut(active_task_id) {
                                    task.needs_auto_scroll = true;
                                }
                            }
                            OrchestratorEvent::StepStarted {
                                agent_id: _,
                                agent_name,
                            } => {
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
                            OrchestratorEvent::RunInTerminal { command, work_dir } => {
                                // 展开终端并执行命令
                                this.terminal_visible = true;
                                let task_id = active_task_id.unwrap_or(0);
                                let backend = this.sandbox_backend.clone();

                                let base_dir = active_task_id
                                    .and_then(|tid| {
                                        this.workspaces.iter().find_map(|workspace| {
                                            workspace.tasks.iter().find(|task| task.id == tid).map(|task| {
                                                this.get_task_dir_for_ids(workspace.id, tid, &task.title)
                                            })
                                        })
                                    })
                                    .unwrap_or_else(|| PathBuf::from(this.get_work_dir()));
                                let requested_dir = work_dir.trim();
                                let project_dir = if requested_dir.is_empty() || requested_dir == "." {
                                    base_dir
                                } else {
                                    let requested_path = PathBuf::from(requested_dir);
                                    if requested_path.is_absolute() {
                                        requested_path
                                    } else {
                                        base_dir.join(requested_path)
                                    }
                                };
                                let _ = std::fs::create_dir_all(&project_dir);
                                let project_dir_str = project_dir.to_string_lossy().to_string();
                                let quoted_project_dir = project_dir_str.replace('\'', "'\\''");

                                // 在命令前加入 cd 到项目目录
                                let cmd = if !command.starts_with("cd ") {
                                    format!("cd '{}' && {}", quoted_project_dir, command)
                                } else {
                                    command.clone()
                                };

                                // 先显示命令
                                this.terminal_output.push(TerminalLine {
                                    command: Some(cmd.clone()),
                                    output: String::new(),
                                });
                                cx.notify();

                                // 在后台执行命令（通过 sh -c 包装，并 cd 到工作目录）
                                cx.spawn(async move |this, cx| {
                                    let exec_result = gpui_tokio::Tokio::spawn(cx, async move {
                                        let sh_cmd = vec!["sh", "-c", &cmd];
                                        match &backend {
                                            Backend::Docker(b) => {
                                                b.exec_command(task_id, sh_cmd).await
                                            }
                                            Backend::Pty(b) => {
                                                b.exec_command(task_id, sh_cmd).await
                                            }
                                        }
                                    }).await;

                                    let output = match exec_result {
                                        Ok(Ok(out)) => out,
                                        Ok(Err(e)) => format!("Error: {}", e),
                                        Err(e) => format!("Spawn error: {}", e),
                                    };

                                    let _ = this.update(cx, |this, cx| {
                                        // 更新最后一条输出
                                        if let Some(last) = this.terminal_output.last_mut() {
                                            last.output = output;
                                        }
                                        cx.notify();
                                    });
                                }).detach();
                            }
                            OrchestratorEvent::StartCodingSession {
                                agent_kind,
                                prompt,
                                write_mode,
                            } => {
                                let kind = crate::runtime::resolve_coding_agent_provider(&agent_kind)
                                    .unwrap_or_else(crate::runtime::default_coding_agent_provider);
                                this.start_persistent_coding_session(kind, prompt, write_mode, cx);
                            }
                            OrchestratorEvent::SendToCodingSession { session_id, text } => {
                                this.send_to_persistent_coding_session(session_id, text, cx);
                            }
                            OrchestratorEvent::ReadCodingSessionOutput { session_id, limit } => {
                                this.read_persistent_coding_session_output(session_id, limit, cx);
                            }
                            OrchestratorEvent::InspectCodingSession { session_id, limit } => {
                                this.inspect_persistent_coding_session(session_id, limit, cx);
                            }
                            OrchestratorEvent::StopCodingSession { session_id } => {
                                this.stop_persistent_coding_session(session_id, cx);
                            }
                            OrchestratorEvent::ListCodingSessions => {
                                this.list_persistent_coding_sessions(cx);
                            }
                            OrchestratorEvent::GetWorkspaceWriteStatus => {
                                this.describe_workspace_write_status(cx);
                            }
                            OrchestratorEvent::AwaitingUserInput { reply } => {
                                // 将 MainAgent 的回复添加到对应 task 的消息列表
                                if let Some(task) = this.task_mut(active_task_id) {
                                    task.messages.push(ChatMessage::new(
                                        "assistant",
                                        &reply,
                                    ));
                                }
                                // ✅ 写 DB（用 captured active_task_id）
                                if let Some(task_id) = active_task_id {
                                    task_db::insert_message(
                                        &this.db.conn,
                                        task_id,
                                        "assistant",
                                        &reply,
                                    )
                                    .ok();
                                }
                                // 清理运行状态，让用户能够输入
                                this.job_manager.request_in_flight = false;
                                this.job_manager.request_kind = None;
                                this.job_manager.request_status_text = None;
                                this.job_manager.general_ai_show_live_bubble = false;
                                this.mark_task_inactive(this.active_task_id);
                                // ── 首次回复完成时触发 summarize ──────────────────
                                // 如果用户不再继续对话，orchestrator 会一直等待输入而不会结束，
                                // Finished 事件永远不会被发送。所以在 AwaitingUserInput 阶段
                                // 就检查并触发 summarize，不等 Finished。
                                if let Some(task) = this.task_mut(active_task_id) {
                                    if task.pending_summarize {
                                        eprintln!(
                                            "[SUMMARIZE] AwaitingUserInput: triggering summarize for task_id={:?}",
                                            active_task_id
                                        );
                                        task.pending_summarize = false;
                                        if let Some(tid) = active_task_id {
                                            this.spawn_summarize_job(tid, cx);
                                        }
                                        // 已经 summarize 过了，关闭用户输入通道
                                        // 让 orchestrator 正常退出
                                        this.job_manager.orchestrator_user_input_tx = None;
                                    }
                                }
                                cx.notify();
                            }
                        },
                        OrchestratorWrapperEvent::Finished(ref result) => {
                            eprintln!(
                                "[ORCH] Finished received: result_empty={}, has_active_task={:?}, summarize={:?}",
                                result.is_empty(),
                                active_task_id,
                                this.active_task_ref().map(|t| t.pending_summarize),
                            );
                            if let Some(rid) = log_run_id {
                                let recorder =
                                    RunRecorder::attach(&this.db.conn, rid);
                                recorder.record(&RunEvent::Finished {
                                    result: result.clone(),
                                });
                                recorder.finish(RunStatus::Finished);
                            }

                            // ── 异步生成记忆快照 ─────────────────────────────
                            if let Some(task_id) = active_task_id {
                                // ✅ 从 DB 加载消息而非 this.messages（防止切换 task 后数据错误）
                                let messages = task_db::load_messages(&this.db.conn, task_id)
                                    .unwrap_or_default()
                                    .into_iter()
                                    .map(|m| crate::memory::types::ChatMessage::new(&m.role, &m.content))
                                    .collect::<Vec<_>>();
                                let base_url = this.model_base_url.clone();
                                let api_key = this.model_api_key.clone();
                                let model = this.model_name.clone();
                                let ws_name = workspace_name_for_snapshot.clone();
                                let task_title = this.workspaces.iter()
                                    .find(|w| w.name == ws_name)
                                    .and_then(|w| w.tasks.iter().find(|t| t.id == task_id))
                                    .map(|t| t.title.clone())
                                    .unwrap_or_else(|| "task".to_string());

                                std::thread::spawn(move || {
                                    crate::memory::snapshot::generate_snapshot_sync(
                                        &base_url, &api_key, &model,
                                        &messages, task_id, &task_title, &ws_name,
                                    );
                                    // ── 写入 L3 chunk ──────────────────────────
                                    let _ = crate::memory::storage::save_task_memory_async(
                                        ws_name, task_id, task_title, messages,
                                    );
                                });
                            }

                            this.job_manager.orchestrator_user_input_tx = None;
                            this.job_manager.request_in_flight = false;
                            this.job_manager.request_status_text = None;
                            this.job_manager.general_ai_live_text.clear();
                            this.job_manager.general_ai_run_id = None;
                            this.job_manager.general_ai_show_live_bubble = false;
                            this.mark_task_inactive(active_task_id);
                            // 如果 result 为空，说明结果已通过 AwaitingUserInput 处理（channel 关闭），
                            // 不再重复写入。
                            if !result.is_empty() {
                                let cur_active_id = this.active_task_id;
                                if let Some(task) = this.task_mut(active_task_id) {
                                    task.messages.push(ChatMessage::new("assistant", &result));
                                    if cur_active_id == active_task_id {
                                        task.needs_auto_scroll = true;
                                    }
                                }
                                if let Some(task_id) = active_task_id {
                                    task_db::insert_message(
                                        &this.db.conn,
                                        task_id,
                                        "assistant",
                                        &result,
                                    )
                                    .ok();
                                }
                            }
                            if let Some(task) = this.task_mut(active_task_id) {
                                eprintln!(
                                    "[SUMMARIZE] Orchestrator Finished: task_id={:?}, pending_summarize={:?}, result_empty={}",
                                    active_task_id, task.pending_summarize, result.is_empty()
                                );
                                if task.pending_summarize {
                                    task.pending_summarize = false;
                                    if let Some(tid) = active_task_id {
                                        this.spawn_summarize_job(tid, cx);
                                    }
                                }
                            }
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
                            this.job_manager.orchestrator_user_input_tx = None;
                            this.job_manager.request_in_flight = false;
                            this.job_manager.request_status_text = None;
                            this.job_manager.general_ai_live_text.clear();
                            this.job_manager.general_ai_run_id = None;
                            this.job_manager.general_ai_show_live_bubble = false;
                            this.mark_task_inactive(active_task_id);
                            let cur_active_id = this.active_task_id;
                            if let Some(task) = this.task_mut(active_task_id) {
                                task.messages.push(ChatMessage::new(
                                    "assistant",
                                    &format!("Orchestrator failed: {}", error),
                                ));
                                if cur_active_id == active_task_id {
                                    task.needs_auto_scroll = true;
                                }
                            }
                            // ✅ 写 DB（用 captured active_task_id）
                            if let Some(task_id) = active_task_id {
                                task_db::insert_message(
                                    &this.db.conn,
                                    task_id,
                                    "assistant",
                                    &format!("Orchestrator failed: {}", error),
                                )
                                .ok();
                            }
                        }
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }
}
