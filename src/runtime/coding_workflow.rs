use std::path::{Path, PathBuf};
use std::process::Stdio;

use gpui::Context;
use sqlez::connection::Connection;
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::memory::types::ChatMessage;
use crate::run_log::{RunEvent, RunKind, RunRecorder, RunStatus};
use crate::{task_db, AppState, TerminalLine};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CodingWorkflowStage {
    PlanningRunning,
    AwaitingApproval,
    Implementing,
    Done,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone)]
pub(crate) struct CodingWorkflowState {
    pub(crate) task_id: usize,
    pub(crate) task_dir: PathBuf,
    pub(crate) user_request: String,
    pub(crate) main_agent_summary: String,
    pub(crate) known_constraints: Vec<String>,
    pub(crate) suggested_direction: Option<String>,
    pub(crate) clarification_focus: Vec<String>,
    pub(crate) plan_path: PathBuf,
    pub(crate) log_path: PathBuf,
    pub(crate) workflow_id: Option<usize>,
    pub(crate) stage: CodingWorkflowStage,
    pub(crate) plan_text: Option<String>,
    pub(crate) approval_notes: Vec<String>,
}

#[derive(Debug)]
enum CodingRunnerEvent {
    Output(String),
    Audit(RunEvent),
    Finished {
        success: bool,
        output: String,
        error: Option<String>,
    },
}

#[derive(Debug, Clone, Copy)]
enum CodingRunKind {
    Planning,
    Implementation,
}

impl AppState {
    pub(crate) fn restore_coding_workflow_context(&mut self) {
        let Some(task_id) = self.active_task_id else {
            if self.job_manager.coding_cancel_tx.is_none() {
                self.job_manager.coding_workflow = None;
            }
            return;
        };

        if self.job_manager.coding_cancel_tx.is_some() {
            return;
        }

        let Ok(Some(row)) = task_db::load_latest_coding_workflow(&self.db.conn, task_id) else {
            self.job_manager.coding_workflow = None;
            return;
        };

        let Some(stage) = coding_stage_from_label(&row.stage) else {
            self.job_manager.coding_workflow = None;
            return;
        };

        if matches!(
            stage,
            CodingWorkflowStage::PlanningRunning | CodingWorkflowStage::Implementing
        ) {
            let _ = task_db::update_coding_workflow_stage(
                &self.db.conn,
                row.id,
                coding_stage_label(CodingWorkflowStage::Failed),
            );
            self.job_manager.coding_workflow = None;
            return;
        }

        if !matches!(stage, CodingWorkflowStage::AwaitingApproval) {
            self.job_manager.coding_workflow = None;
            return;
        }

        let task_dir = self
            .get_active_task_dir_path()
            .unwrap_or_else(|| PathBuf::from(self.get_work_dir()));
        let plan_path = if row.plan_path.trim().is_empty() {
            task_dir.join("CLAUDE_PLAN.md")
        } else {
            PathBuf::from(&row.plan_path)
        };
        let log_path = if row.log_path.trim().is_empty() {
            task_dir.join("claude-code.log")
        } else {
            PathBuf::from(&row.log_path)
        };
        let plan_text = std::fs::read_to_string(&plan_path).ok();

        self.job_manager.coding_workflow = Some(CodingWorkflowState {
            task_id: row.task_id,
            task_dir,
            user_request: row.user_request,
            main_agent_summary: row.main_agent_summary,
            known_constraints: parse_string_vec(&row.known_constraints_json),
            suggested_direction: non_empty_string(row.suggested_direction),
            clarification_focus: parse_string_vec(&row.clarification_focus_json),
            plan_path,
            log_path,
            workflow_id: Some(row.id),
            stage,
            plan_text,
            approval_notes: parse_string_vec(&row.approval_notes_json),
        });
        self.append_coding_restore_hint();
    }

    pub(crate) fn start_coding_workflow(
        &mut self,
        user_request: String,
        main_agent_summary: String,
        known_constraints: Vec<String>,
        suggested_direction: Option<String>,
        clarification_focus: Vec<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(task_id) = self.active_task_id else {
            self.append_active_task_message(
                "assistant",
                "请先选择一个 task，再启动编码工作流。",
                cx,
            );
            return;
        };
        let Some(task_dir) = self.get_active_task_dir_path() else {
            self.append_active_task_message("assistant", "没有找到当前 task 的工作目录。", cx);
            return;
        };

        let _ = std::fs::create_dir_all(&task_dir);
        let plan_path = task_dir.join("CLAUDE_PLAN.md");
        let log_path = task_dir.join("claude-code.log");
        let workflow_id = task_db::insert_coding_workflow(
            &self.db.conn,
            task_id,
            coding_stage_label(CodingWorkflowStage::PlanningRunning),
            &user_request,
            &plan_path.to_string_lossy(),
            &log_path.to_string_lossy(),
        )
        .ok();
        if let Some(workflow_id) = workflow_id {
            let _ = task_db::upsert_task_artifact(
                &self.db.conn,
                task_id,
                Some(workflow_id),
                "claude_plan",
                &plan_path.to_string_lossy(),
                "Claude Code 方案",
            );
            let _ = task_db::upsert_task_artifact(
                &self.db.conn,
                task_id,
                Some(workflow_id),
                "claude_log",
                &log_path.to_string_lossy(),
                "Claude Code 日志",
            );
        }

        let state = CodingWorkflowState {
            task_id,
            task_dir: task_dir.clone(),
            user_request,
            main_agent_summary,
            known_constraints,
            suggested_direction,
            clarification_focus,
            plan_path,
            log_path,
            workflow_id,
            stage: CodingWorkflowStage::PlanningRunning,
            plan_text: None,
            approval_notes: Vec::new(),
        };
        self.persist_coding_workflow_context(&state, None);

        self.job_manager.coding_workflow = Some(state.clone());
        self.job_manager.coding_cancel_tx = None;
        self.job_manager.clear_request_full();
        self.job_manager.orchestrator_user_input_tx = None;
        self.mark_task_inactive(Some(task_id));
        self.terminal_visible = true;
        self.terminal_output.clear();
        self.append_active_task_message(
            "assistant",
            "我已识别这是编码任务。先让 Claude Code 做需求细化、方案调研和任务拆解；这个阶段不会写业务代码。完成后我会总结方案并等你确认。",
            cx,
        );
        self.spawn_coding_runner(state, CodingRunKind::Planning, None, cx);
    }

    pub(crate) fn try_handle_coding_workflow_input(
        &mut self,
        user_message: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(state) = self.job_manager.coding_workflow.clone() else {
            return false;
        };
        if self.active_task_id != Some(state.task_id) {
            return false;
        }
        if matches!(
            state.stage,
            CodingWorkflowStage::PlanningRunning | CodingWorkflowStage::Implementing
        ) {
            self.append_active_task_message(
                "assistant",
                "Claude Code 当前阶段还在运行中，请先等这一阶段结束；需要中止可以使用停止按钮。",
                cx,
            );
            return true;
        }
        if state.stage != CodingWorkflowStage::AwaitingApproval {
            return false;
        }

        if !looks_like_confirmation(user_message) {
            let mut next = state.clone();
            next.approval_notes.push(user_message.trim().to_string());
            self.persist_coding_workflow_context(&next, None);
            self.job_manager.coding_workflow = Some(next);
            self.append_active_task_message(
                "assistant",
                "我会把这条补充意见带入执行阶段。请回复“确认”或“开始”，我再让 Claude Code 动手编码。",
                cx,
            );
            return true;
        }

        let mut next = state.clone();
        next.stage = CodingWorkflowStage::Implementing;
        if let Some(workflow_id) = next.workflow_id {
            let _ = task_db::update_coding_workflow_stage(
                &self.db.conn,
                workflow_id,
                coding_stage_label(next.stage.clone()),
            );
        }
        self.persist_coding_workflow_context(&next, None);
        self.job_manager.coding_workflow = Some(next.clone());
        self.terminal_visible = true;
        let approval_message = combined_approval_message(&next.approval_notes, user_message);
        self.append_active_task_message(
            "assistant",
            "收到确认。现在让 Claude Code 使用 auto-accept 模式进入编码执行阶段。",
            cx,
        );
        self.spawn_coding_runner(
            next,
            CodingRunKind::Implementation,
            Some(approval_message),
            cx,
        );
        true
    }

    fn spawn_coding_runner(
        &mut self,
        state: CodingWorkflowState,
        kind: CodingRunKind,
        approval_message: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let prompt = match kind {
            CodingRunKind::Planning => build_planning_prompt(&state),
            CodingRunKind::Implementation => {
                build_implementation_prompt(&state, approval_message.as_deref())
            }
        };
        let command_label = match kind {
            CodingRunKind::Planning => "claude -p <planning prompt>",
            CodingRunKind::Implementation => {
                "claude -p <implementation prompt> --permission-mode bypassPermissions"
            }
        };
        self.terminal_output.push(TerminalLine {
            command: Some(command_label.to_string()),
            output: String::new(),
        });
        cx.notify();

        let audit_run_id = RunRecorder::begin(
            &self.db.conn,
            state.task_id,
            RunKind::ClaudeCode,
            format!(
                "{}: {}",
                match kind {
                    CodingRunKind::Planning => "planning",
                    CodingRunKind::Implementation => "implementation",
                },
                state.user_request
            ),
        );
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<CodingRunnerEvent>();
        let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel::<()>();
        self.job_manager.coding_cancel_tx = Some(cancel_tx);
        let task_dir = state.task_dir.clone();
        let log_path = state.log_path.clone();
        gpui_tokio::Tokio::spawn(cx, async move {
            run_claude_code(
                task_dir,
                log_path,
                prompt,
                matches!(kind, CodingRunKind::Implementation),
                cancel_rx,
                sender,
            )
            .await;
        })
        .detach();

        cx.spawn(async move |this, cx| {
            while let Some(event) = receiver.recv().await {
                let _ = this.update(cx, |this, cx| match event {
                    CodingRunnerEvent::Output(chunk) => {
                        if let Some(last) = this.terminal_output.last_mut() {
                            last.output.push_str(&chunk);
                        }
                        cx.notify();
                    }
                    CodingRunnerEvent::Audit(event) => {
                        if let Some(run_id) = audit_run_id {
                            RunRecorder::attach(&this.db.conn, run_id).record(&event);
                        }
                    }
                    CodingRunnerEvent::Finished {
                        success,
                        output,
                        error,
                    } => {
                        this.finish_coding_runner(
                            kind,
                            success,
                            output,
                            error.clone(),
                            audit_run_id,
                            cx,
                        );
                        this.finish_coding_audit(audit_run_id, success, &error);
                    }
                });
            }
        })
        .detach();
    }

    fn finish_coding_runner(
        &mut self,
        kind: CodingRunKind,
        success: bool,
        output: String,
        error: Option<String>,
        audit_run_id: Option<usize>,
        cx: &mut Context<Self>,
    ) {
        let Some(mut state) = self.job_manager.coding_workflow.clone() else {
            return;
        };
        self.job_manager.coding_cancel_tx = None;

        if !success {
            let error_message = error.unwrap_or_else(|| "未知错误，请查看终端日志。".to_string());
            state.stage = if error_message.contains("用户取消") {
                CodingWorkflowStage::Cancelled
            } else {
                CodingWorkflowStage::Failed
            };
            if let Some(workflow_id) = state.workflow_id {
                let _ = task_db::update_coding_workflow_stage(
                    &self.db.conn,
                    workflow_id,
                    coding_stage_label(state.stage.clone()),
                );
            }
            self.persist_coding_workflow_context(&state, Some(&error_message));
            self.job_manager.coding_workflow = Some(state);
            let msg = format!("Claude Code 执行未完成：{}", error_message);
            self.append_active_task_message("assistant", &msg, cx);
            return;
        }

        match kind {
            CodingRunKind::Planning => {
                let plan = output.trim().to_string();
                if !plan.is_empty() {
                    let _ = std::fs::write(&state.plan_path, &plan);
                }
                state.stage = CodingWorkflowStage::AwaitingApproval;
                state.plan_text = Some(plan.clone());
                if let Some(workflow_id) = state.workflow_id {
                    let _ = task_db::update_coding_workflow_stage(
                        &self.db.conn,
                        workflow_id,
                        coding_stage_label(state.stage.clone()),
                    );
                    let _ = task_db::upsert_task_artifact(
                        &self.db.conn,
                        state.task_id,
                        Some(workflow_id),
                        "claude_plan",
                        &state.plan_path.to_string_lossy(),
                        "Claude Code 方案",
                    );
                }
                self.persist_coding_workflow_context(&state, None);
                self.job_manager.coding_workflow = Some(state);
                let summary = summarize_plan_for_chat(&plan);
                self.append_active_task_message("assistant", &summary, cx);
            }
            CodingRunKind::Implementation => {
                state.stage = CodingWorkflowStage::Done;
                let indexed_artifacts = collect_task_artifacts(&state.task_dir);
                if let Some(workflow_id) = state.workflow_id {
                    let _ = task_db::update_coding_workflow_stage(
                        &self.db.conn,
                        workflow_id,
                        coding_stage_label(state.stage.clone()),
                    );
                    let _ = task_db::upsert_task_artifact(
                        &self.db.conn,
                        state.task_id,
                        Some(workflow_id),
                        "claude_log",
                        &state.log_path.to_string_lossy(),
                        "Claude Code 日志",
                    );
                    index_task_artifacts_list(
                        &self.db.conn,
                        &state,
                        workflow_id,
                        &indexed_artifacts,
                    );
                    record_indexed_artifacts(
                        &self.db.conn,
                        audit_run_id,
                        &state,
                        &indexed_artifacts,
                    );
                }
                self.persist_coding_workflow_context(&state, None);
                self.job_manager.coding_workflow = Some(state);
                self.prepare_active_task_preview();
                let summary = summarize_implementation_for_chat(&output, &indexed_artifacts);
                self.append_active_task_message("assistant", &summary, cx);
            }
        }
    }

    fn finish_coding_audit(&self, run_id: Option<usize>, success: bool, error: &Option<String>) {
        let Some(run_id) = run_id else {
            return;
        };
        let recorder = RunRecorder::attach(&self.db.conn, run_id);
        if success {
            recorder.record(&RunEvent::Finished {
                result: "Claude Code 阶段完成。".to_string(),
            });
            recorder.finish(RunStatus::Finished);
        } else {
            let error_message = error
                .clone()
                .unwrap_or_else(|| "Claude Code 阶段失败。".to_string());
            recorder.record(&RunEvent::Failed {
                error: error_message.clone(),
            });
            let status = if error_message.contains("用户取消") {
                RunStatus::Cancelled
            } else {
                RunStatus::Failed
            };
            recorder.finish(status);
        }
    }

    fn append_active_task_message(&mut self, role: &str, content: &str, cx: &mut Context<Self>) {
        if let Some(task) = self.active_task_mut() {
            task.messages.push(ChatMessage::new(role, content));
            task.needs_auto_scroll = true;
        }
        if let Some(task_id) = self.active_task_id {
            let _ = task_db::insert_message(&self.db.conn, task_id, role, content);
        }
        cx.notify();
    }

    fn append_coding_restore_hint(&mut self) {
        const RESTORE_HINT: &str = "已恢复一个待确认的 Claude Code 方案。方案文件在当前 task 目录的 `CLAUDE_PLAN.md`；请回复“确认”或“开始”进入编码阶段，也可以直接补充修改意见。";
        let Some(task) = self.active_task_mut() else {
            return;
        };
        let already_visible = task
            .messages
            .last()
            .map(|message| message.role == "assistant" && message.content == RESTORE_HINT)
            .unwrap_or(false);
        if !already_visible {
            task.messages
                .push(ChatMessage::new("assistant", RESTORE_HINT));
            task.needs_auto_scroll = true;
        }
    }

    fn persist_coding_workflow_context(
        &self,
        state: &CodingWorkflowState,
        last_error: Option<&str>,
    ) {
        let Some(workflow_id) = state.workflow_id else {
            return;
        };
        let known_constraints_json =
            serde_json::to_string(&state.known_constraints).unwrap_or_else(|_| "[]".to_string());
        let clarification_focus_json =
            serde_json::to_string(&state.clarification_focus).unwrap_or_else(|_| "[]".to_string());
        let approval_notes_json =
            serde_json::to_string(&state.approval_notes).unwrap_or_else(|_| "[]".to_string());
        let _ = task_db::update_coding_workflow_context(
            &self.db.conn,
            workflow_id,
            &state.main_agent_summary,
            &known_constraints_json,
            state.suggested_direction.as_deref(),
            &clarification_focus_json,
            &approval_notes_json,
            last_error,
        );
    }
}

async fn run_claude_code(
    task_dir: PathBuf,
    log_path: PathBuf,
    prompt: String,
    auto_accept: bool,
    mut cancel_rx: tokio::sync::oneshot::Receiver<()>,
    sender: tokio::sync::mpsc::UnboundedSender<CodingRunnerEvent>,
) {
    let settings_path = match ensure_claude_guard(&task_dir) {
        Ok(path) => Some(path),
        Err(e) => {
            let line = format!("Claude Code hook guard 初始化失败，将继续运行：{}\n", e);
            append_log(&log_path, &line);
            let _ = sender.send(CodingRunnerEvent::Output(line));
            None
        }
    };

    let mut command = tokio::process::Command::new("claude");
    command
        .arg("-p")
        .arg(prompt)
        .arg("--output-format")
        .arg("stream-json")
        .arg("--verbose")
        .current_dir(&task_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(settings_path) = settings_path {
        command.arg("--settings").arg(settings_path);
    }
    if auto_accept {
        command.arg("--permission-mode").arg("bypassPermissions");
    }

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) => {
            let _ = sender.send(CodingRunnerEvent::Finished {
                success: false,
                output: String::new(),
                error: Some(format!("未能启动 Claude Code：{}", e)),
            });
            return;
        }
    };

    let mut output = String::new();
    let (line_tx, mut line_rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    if let Some(stdout) = child.stdout.take() {
        let tx = line_tx.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let _ = tx.send(format!("{}\n", line));
            }
        });
    }

    if let Some(stderr) = child.stderr.take() {
        let tx = line_tx.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let _ = tx.send(format!("{}\n", line));
            }
        });
    }
    drop(line_tx);

    loop {
        tokio::select! {
            line = line_rx.recv() => {
                if let Some(raw_line) = line {
                    append_log(&log_path, &raw_line);
                    let (display_line, audit_event) = parse_claude_stream_line(&raw_line);
                    output.push_str(&display_line);
                    let _ = sender.send(CodingRunnerEvent::Output(display_line));
                    if let Some(event) = audit_event {
                        let _ = sender.send(CodingRunnerEvent::Audit(event));
                    }
                } else {
                    match child.wait().await {
                        Ok(status) => {
                            let _ = sender.send(CodingRunnerEvent::Finished {
                                success: status.success(),
                                output,
                                error: if status.success() { None } else { Some(format!("Claude Code 退出状态：{}", status)) },
                            });
                        }
                        Err(e) => {
                            let _ = sender.send(CodingRunnerEvent::Finished {
                                success: false,
                                output,
                                error: Some(format!("等待 Claude Code 结束失败：{}", e)),
                            });
                        }
                    }
                    break;
                }
            }
            _ = &mut cancel_rx => {
                let _ = child.kill().await;
                let message = "用户取消了 Claude Code 执行。\n".to_string();
                output.push_str(&message);
                append_log(&log_path, &message);
                let _ = sender.send(CodingRunnerEvent::Output(message));
                let _ = sender.send(CodingRunnerEvent::Finished {
                    success: false,
                    output,
                    error: Some("用户取消了 Claude Code 执行。".to_string()),
                });
                break;
            }
            status = child.wait() => {
                match status {
                    Ok(status) => {
                        while let Some(raw_line) = line_rx.recv().await {
                            append_log(&log_path, &raw_line);
                            let (display_line, audit_event) = parse_claude_stream_line(&raw_line);
                            output.push_str(&display_line);
                            let _ = sender.send(CodingRunnerEvent::Output(display_line));
                            if let Some(event) = audit_event {
                                let _ = sender.send(CodingRunnerEvent::Audit(event));
                            }
                        }
                        let _ = sender.send(CodingRunnerEvent::Finished {
                            success: status.success(),
                            output,
                            error: if status.success() { None } else { Some(format!("Claude Code 退出状态：{}", status)) },
                        });
                    }
                    Err(e) => {
                        let _ = sender.send(CodingRunnerEvent::Finished {
                            success: false,
                            output,
                            error: Some(format!("等待 Claude Code 结束失败：{}", e)),
                        });
                    }
                }
                break;
            }
        }
    }
}

fn append_log(path: &Path, text: &str) {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = file.write_all(text.as_bytes());
    }
}

fn parse_claude_stream_line(raw_line: &str) -> (String, Option<RunEvent>) {
    let trimmed = raw_line.trim();
    if trimmed.is_empty() {
        return (raw_line.to_string(), None);
    }

    let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) else {
        return (raw_line.to_string(), None);
    };

    if let Some((name, args)) = extract_claude_tool_use(&value) {
        return (
            format!("[Claude tool] {} {}\n", name, args),
            Some(RunEvent::ToolCall { name, args }),
        );
    }

    if let Some((name, result)) = extract_claude_tool_result(&value) {
        return (
            format!(
                "[Claude tool result] {} {}\n",
                name,
                truncate_chars(&result, 600)
            ),
            Some(RunEvent::ToolResult { name, result }),
        );
    }

    if let Some(text) = extract_claude_text(&value) {
        if !text.trim().is_empty() {
            return (format!("{}\n", text), Some(RunEvent::MessageDelta { text }));
        }
    }

    if let Some(result) = value.get("result").and_then(|v| v.as_str()) {
        if !result.trim().is_empty() {
            return (format!("[Claude result] {}\n", result), None);
        }
    }

    let event_type = value
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("event");
    let subtype = value
        .get("subtype")
        .and_then(|v| v.as_str())
        .map(|s| format!(":{}", s))
        .unwrap_or_default();
    (format!("[Claude {}{}]\n", event_type, subtype), None)
}

fn extract_claude_text(value: &serde_json::Value) -> Option<String> {
    let mut parts = Vec::new();
    collect_claude_content_blocks(value, "text", |block| {
        if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
            parts.push(text.to_string());
        }
    });
    if parts.is_empty() {
        value
            .get("delta")
            .and_then(|delta| delta.get("text"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    } else {
        Some(parts.join("\n"))
    }
}

fn extract_claude_tool_use(value: &serde_json::Value) -> Option<(String, String)> {
    let mut found = None;
    collect_claude_content_blocks(value, "tool_use", |block| {
        if found.is_none() {
            let name = block
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("tool")
                .to_string();
            let args = block
                .get("input")
                .map(compact_json_value)
                .unwrap_or_else(|| "{}".to_string());
            found = Some((name, args));
        }
    });
    found
}

fn extract_claude_tool_result(value: &serde_json::Value) -> Option<(String, String)> {
    let mut found = None;
    collect_claude_content_blocks(value, "tool_result", |block| {
        if found.is_none() {
            let name = block
                .get("name")
                .or_else(|| block.get("tool_use_id"))
                .and_then(|v| v.as_str())
                .unwrap_or("tool_result")
                .to_string();
            let result = block
                .get("content")
                .and_then(|v| v.as_str().map(|s| s.to_string()))
                .unwrap_or_else(|| compact_json_value(block));
            found = Some((name, result));
        }
    });
    found
}

fn collect_claude_content_blocks<F>(value: &serde_json::Value, kind: &str, mut visit: F)
where
    F: FnMut(&serde_json::Value),
{
    let content = value
        .get("message")
        .and_then(|message| message.get("content"))
        .or_else(|| value.get("content"));
    let Some(content) = content else {
        return;
    };
    if let Some(items) = content.as_array() {
        for item in items {
            if item.get("type").and_then(|v| v.as_str()) == Some(kind) {
                visit(item);
            }
        }
    }
}

fn compact_json_value(value: &serde_json::Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| value.to_string())
}

fn ensure_claude_guard(task_dir: &Path) -> std::io::Result<PathBuf> {
    let claude_dir = task_dir.join(".claude");
    let hooks_dir = claude_dir.join("hooks");
    std::fs::create_dir_all(&hooks_dir)?;

    let guard_path = hooks_dir.join("one-pretool-guard.py");
    let task_dir_literal = task_dir
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let guard = format!(
        r#"#!/usr/bin/env python3
import json
import os
import re
import sys

TASK_DIR = "{}"

try:
    payload = json.load(sys.stdin)
except Exception:
    sys.exit(0)

tool_name = payload.get("tool_name", "")
tool_input = payload.get("tool_input") or {{}}

def deny(message):
    print(message, file=sys.stderr)
    sys.exit(2)

def inside_task(path):
    if not path:
        return True
    path = os.path.abspath(os.path.expanduser(str(path)))
    root = os.path.abspath(TASK_DIR)
    try:
        return os.path.commonpath([root, path]) == root
    except ValueError:
        return False

if tool_name == "Bash":
    command = str(tool_input.get("command", ""))
    risky = [
        r"\brm\s+-rf\s+/",
        r"\bsudo\b",
        r"\bchmod\s+-R\s+777\b",
        r"\bchown\s+-R\b",
        r">\s*/etc/",
        r"\bmv\b.+\s+/",
    ]
    if any(re.search(pattern, command) for pattern in risky):
        deny("ONE guard blocked a potentially destructive shell command.")

for key in ("file_path", "path"):
    if key in tool_input and not inside_task(tool_input.get(key)):
        deny("ONE guard blocked file access outside the current task directory.")

for key in ("files", "paths"):
    value = tool_input.get(key)
    if isinstance(value, list):
        for path in value:
            if not inside_task(path):
                deny("ONE guard blocked file access outside the current task directory.")

sys.exit(0)
"#,
        task_dir_literal
    );
    std::fs::write(&guard_path, guard)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&guard_path)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&guard_path, perms)?;
    }

    let settings_path = claude_dir.join("settings.local.json");
    let settings = serde_json::json!({
        "hooks": {
            "PreToolUse": [
                {
                    "matcher": "*",
                    "hooks": [
                        {
                            "type": "command",
                            "command": guard_path.to_string_lossy()
                        }
                    ]
                }
            ]
        }
    });
    std::fs::write(&settings_path, serde_json::to_vec_pretty(&settings)?)?;
    Ok(settings_path)
}

fn build_planning_prompt(state: &CodingWorkflowState) -> String {
    format!(
        r#"你是 Claude Code。当前阶段只做需求澄清、方案调研和任务拆解，不要修改任何文件，不要创建任何项目文件。

用户原始需求：
{}

MainAgent 初步梳理：
{}

已知约束：
{}

建议方向：
{}

希望重点澄清：
{}

请只输出 Markdown 方案内容。系统会把你的完整输出保存为当前 task 目录下的 CLAUDE_PLAN.md。

你需要完成：

1. 复述用户目标和边界
2. 检查当前目录结构，判断是否已有项目基础
3. 梳理需要实现的核心功能
4. 调研并比较适合的实现方案
5. 给出推荐方案和理由
6. 拆解成可执行任务清单
7. 列出需要用户确认的问题
8. 明确下一阶段编码时会创建或修改哪些主要文件

重要限制：

- 当前阶段不要写业务代码
- 当前阶段不要初始化项目
- 当前阶段不要安装依赖
- 当前阶段不要修改已有源代码
- 只允许输出分析和方案
"#,
        state.user_request,
        state.main_agent_summary,
        list_or_none(&state.known_constraints),
        state.suggested_direction.as_deref().unwrap_or("无"),
        list_or_none(&state.clarification_focus)
    )
}

fn build_implementation_prompt(
    state: &CodingWorkflowState,
    approval_message: Option<&str>,
) -> String {
    let plan = state
        .plan_text
        .clone()
        .or_else(|| std::fs::read_to_string(&state.plan_path).ok())
        .unwrap_or_default();
    format!(
        r#"你是 Claude Code。现在进入编码执行阶段。

工作目录就是当前 task 目录。所有代码、配置、资源和文档都应创建或修改在当前 task 目录内。

用户原始需求：
{}

MainAgent 初步梳理：
{}

第一阶段方案：
{}

用户确认/补充：
{}

请根据已确认方案执行编码任务。

执行要求：

1. 在当前 task 目录内完成实现
2. 如果需要创建应用项目，直接在当前 task 目录创建
3. 保持结构清晰，避免无关文件
4. 需要依赖时可创建配置文件并安装依赖
5. 完成后运行必要的检查或启动验证
6. 输出最终总结，包括：
   - 创建/修改了哪些文件
   - 如何运行
   - 做了哪些验证
   - 还有哪些后续建议
"#,
        state.user_request,
        state.main_agent_summary,
        plan,
        approval_message.unwrap_or("用户已确认按方案执行。")
    )
}

fn list_or_none(items: &[String]) -> String {
    if items.is_empty() {
        "无".to_string()
    } else {
        items
            .iter()
            .map(|item| format!("- {}", item))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

pub(crate) fn coding_stage_label(stage: CodingWorkflowStage) -> &'static str {
    match stage {
        CodingWorkflowStage::PlanningRunning => "planning_running",
        CodingWorkflowStage::AwaitingApproval => "awaiting_approval",
        CodingWorkflowStage::Implementing => "implementing",
        CodingWorkflowStage::Done => "done",
        CodingWorkflowStage::Failed => "failed",
        CodingWorkflowStage::Cancelled => "cancelled",
    }
}

fn coding_stage_from_label(label: &str) -> Option<CodingWorkflowStage> {
    match label {
        "planning_running" => Some(CodingWorkflowStage::PlanningRunning),
        "awaiting_approval" => Some(CodingWorkflowStage::AwaitingApproval),
        "implementing" => Some(CodingWorkflowStage::Implementing),
        "done" => Some(CodingWorkflowStage::Done),
        "failed" => Some(CodingWorkflowStage::Failed),
        "cancelled" => Some(CodingWorkflowStage::Cancelled),
        _ => None,
    }
}

fn parse_string_vec(json: &str) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(json).unwrap_or_default()
}

fn non_empty_string(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn looks_like_confirmation(text: &str) -> bool {
    let t = text.trim().to_lowercase();
    [
        "确认",
        "开始",
        "可以",
        "继续",
        "执行",
        "按这个做",
        "ok",
        "yes",
        "go",
    ]
    .iter()
    .any(|keyword| t.contains(keyword))
}

fn combined_approval_message(notes: &[String], confirmation: &str) -> String {
    let mut parts = Vec::new();
    if !notes.is_empty() {
        parts.push(format!("用户补充意见：\n{}", list_or_none(notes)));
    }
    parts.push(format!("用户确认：{}", confirmation.trim()));
    parts.join("\n\n")
}

fn summarize_plan_for_chat(plan: &str) -> String {
    let preview = truncate_chars(plan.trim(), 1800);
    format!(
        "Claude Code 已完成方案梳理，并已保存到当前 task 目录的 `CLAUDE_PLAN.md`。\n\n{}\n\n请确认是否按这个方案进入编码阶段；确认后我会让 Claude Code 使用 auto-accept 模式执行。",
        preview
    )
}

fn summarize_implementation_for_chat(output: &str, artifacts: &[IndexedArtifact]) -> String {
    let preview = truncate_chars(output.trim(), 1800);
    let artifact_summary = if artifacts.is_empty() {
        "未发现可索引的代码或文档产物。".to_string()
    } else {
        artifacts
            .iter()
            .take(12)
            .map(|artifact| format!("- {} ({})", artifact.path.to_string_lossy(), artifact.kind))
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!(
        "Claude Code 编码阶段已结束。\n\n产物清单：\n{}\n\n终端输出摘要：\n\n{}\n\n完整日志已保存到当前 task 目录的 `claude-code.log`。",
        artifact_summary, preview
    )
}

fn index_task_artifacts_list(
    conn: &Connection,
    state: &CodingWorkflowState,
    workflow_id: usize,
    artifacts: &[IndexedArtifact],
) {
    for artifact in artifacts {
        let metadata = artifact_metadata_json(&state.task_dir, &artifact.path);
        let _ = task_db::upsert_task_artifact_with_metadata(
            conn,
            state.task_id,
            Some(workflow_id),
            artifact.kind,
            &artifact.path.to_string_lossy(),
            artifact.title,
            "ready",
            metadata.as_deref(),
        );
    }
}

fn record_indexed_artifacts(
    conn: &Connection,
    run_id: Option<usize>,
    state: &CodingWorkflowState,
    artifacts: &[IndexedArtifact],
) {
    let Some(run_id) = run_id else {
        return;
    };
    let recorder = RunRecorder::attach(conn, run_id);
    recorder.record(&RunEvent::ArtifactCreated {
        path: state.plan_path.to_string_lossy().to_string(),
        kind: "claude_plan".to_string(),
    });
    recorder.record(&RunEvent::ArtifactCreated {
        path: state.log_path.to_string_lossy().to_string(),
        kind: "claude_log".to_string(),
    });
    for artifact in artifacts {
        recorder.record(&RunEvent::ArtifactCreated {
            path: artifact.path.to_string_lossy().to_string(),
            kind: artifact.kind.to_string(),
        });
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct IndexedArtifact {
    kind: &'static str,
    path: PathBuf,
    title: &'static str,
}

fn collect_task_artifacts(task_dir: &Path) -> Vec<IndexedArtifact> {
    fn walk(dir: &Path, task_dir: &Path, out: &mut Vec<IndexedArtifact>, depth: usize) {
        if depth > 4 || out.len() >= 80 {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        let mut entries = entries.flatten().collect::<Vec<_>>();
        entries.sort_by_key(|entry| entry.path());

        for entry in entries {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if path.is_dir() {
                if should_skip_artifact_dir(name) {
                    continue;
                }
                walk(&path, task_dir, out, depth + 1);
                continue;
            }

            if let Some(artifact) = classify_artifact(task_dir, &path, name) {
                if !out.iter().any(|existing| existing.path == artifact.path) {
                    out.push(artifact);
                }
            }
        }
    }

    let mut artifacts = Vec::new();
    walk(task_dir, task_dir, &mut artifacts, 0);
    artifacts.sort_by_key(|artifact| artifact_rank(artifact));
    artifacts.truncate(40);
    artifacts
}

fn should_skip_artifact_dir(name: &str) -> bool {
    matches!(
        name,
        ".claude"
            | ".git"
            | ".next"
            | ".nuxt"
            | ".svelte-kit"
            | "dist"
            | "build"
            | "node_modules"
            | "target"
            | "vendor"
    )
}

fn classify_artifact(task_dir: &Path, path: &Path, name: &str) -> Option<IndexedArtifact> {
    if matches!(name, "CLAUDE_PLAN.md" | "claude-code.log") {
        return None;
    }

    let name_lower = name.to_ascii_lowercase();
    let relative_depth = path
        .strip_prefix(task_dir)
        .ok()
        .map(|relative| relative.components().count())
        .unwrap_or(usize::MAX);
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .unwrap_or_default();

    let (kind, title) = if name_lower == "index.html" {
        ("html_entry", "HTML 入口")
    } else if extension == "html" {
        ("html_file", "HTML 页面")
    } else if matches!(
        name,
        "package.json"
            | "Cargo.toml"
            | "pyproject.toml"
            | "requirements.txt"
            | "vite.config.ts"
            | "vite.config.js"
            | "next.config.js"
            | "tsconfig.json"
    ) {
        ("project_manifest", "项目配置")
    } else if matches!(
        name_lower.as_str(),
        "readme.md" | "readme.txt" | "design.md" | "design.txt"
    ) || name_lower.ends_with(".md")
    {
        ("documentation", "文档")
    } else if matches!(
        extension.as_str(),
        "rs" | "js" | "jsx" | "ts" | "tsx" | "py" | "go" | "java" | "kt" | "swift" | "css" | "scss"
    ) && relative_depth <= 4
    {
        ("source_file", "源代码")
    } else {
        return None;
    };

    Some(IndexedArtifact {
        kind,
        path: path.to_path_buf(),
        title,
    })
}

fn artifact_rank(artifact: &IndexedArtifact) -> (u8, String) {
    let rank = match artifact.kind {
        "html_entry" => 0,
        "project_manifest" => 1,
        "documentation" => 2,
        "html_file" => 3,
        "source_file" => 4,
        _ => 9,
    };
    (rank, artifact.path.to_string_lossy().to_string())
}

fn artifact_metadata_json(task_dir: &Path, path: &Path) -> Option<String> {
    let metadata = std::fs::metadata(path).ok()?;
    let relative_path = path
        .strip_prefix(task_dir)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/");
    let modified_unix = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs());
    serde_json::to_string(&serde_json::json!({
        "relative_path": relative_path,
        "size_bytes": metadata.len(),
        "modified_unix": modified_unix,
    }))
    .ok()
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (idx, ch) in text.chars().enumerate() {
        if idx >= max_chars {
            out.push_str("\n...");
            return out;
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod artifact_index_tests {
    use super::*;
    use std::io::Write;
    use std::process::{Command, Stdio};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_task_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "one-artifact-index-test-{}-{}",
            std::process::id(),
            nanos
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn indexes_common_task_artifacts() {
        let dir = temp_task_dir();
        std::fs::write(dir.join("index.html"), "<h1>Hello</h1>").unwrap();
        std::fs::write(dir.join("package.json"), "{}").unwrap();
        std::fs::write(dir.join("README.md"), "# App").unwrap();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src").join("main.ts"), "console.log(1)").unwrap();
        std::fs::create_dir_all(dir.join("node_modules")).unwrap();
        std::fs::write(dir.join("node_modules").join("ignored.js"), "").unwrap();
        std::fs::write(dir.join("CLAUDE_PLAN.md"), "plan").unwrap();

        let artifacts = collect_task_artifacts(&dir);
        let names = artifacts
            .iter()
            .map(|artifact| {
                artifact
                    .path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .to_string()
            })
            .collect::<Vec<_>>();

        assert!(names.contains(&"index.html".to_string()));
        assert!(names.contains(&"package.json".to_string()));
        assert!(names.contains(&"README.md".to_string()));
        assert!(names.contains(&"main.ts".to_string()));
        assert!(!names.contains(&"ignored.js".to_string()));
        assert!(!names.contains(&"CLAUDE_PLAN.md".to_string()));
        assert_eq!(artifacts[0].kind, "html_entry");
    }

    #[test]
    fn artifact_metadata_includes_relative_path_and_size() {
        let dir = temp_task_dir();
        let path = dir.join("README.md");
        std::fs::write(&path, "# App").unwrap();

        let metadata = artifact_metadata_json(&dir, &path).unwrap();
        let json: serde_json::Value = serde_json::from_str(&metadata).unwrap();
        assert_eq!(json["relative_path"], "README.md");
        assert_eq!(json["size_bytes"], 5);
    }

    #[test]
    fn implementation_summary_lists_artifacts() {
        let dir = temp_task_dir();
        let path = dir.join("index.html");
        let artifacts = vec![IndexedArtifact {
            kind: "html_entry",
            path,
            title: "HTML 入口",
        }];

        let summary = summarize_implementation_for_chat("done", &artifacts);
        assert!(summary.contains("产物清单"));
        assert!(summary.contains("index.html"));
        assert!(summary.contains("html_entry"));
    }

    #[test]
    fn parses_claude_stream_text_event() {
        let raw = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"hello"}]}}"#;
        let (display, event) = parse_claude_stream_line(raw);
        assert_eq!(display, "hello\n");
        assert!(matches!(
            event,
            Some(RunEvent::MessageDelta { ref text }) if text == "hello"
        ));
    }

    #[test]
    fn parses_claude_stream_tool_use_event() {
        let raw = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Write","input":{"file_path":"index.html"}}]}}"#;
        let (display, event) = parse_claude_stream_line(raw);
        assert!(display.contains("[Claude tool] Write"));
        assert!(matches!(
            event,
            Some(RunEvent::ToolCall { ref name, ref args })
                if name == "Write" && args.contains("index.html")
        ));
    }

    #[test]
    fn parses_claude_stream_tool_result_event() {
        let raw = r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_1","content":"ok"}]}}"#;
        let (display, event) = parse_claude_stream_line(raw);
        assert!(display.contains("[Claude tool result] toolu_1"));
        assert!(matches!(
            event,
            Some(RunEvent::ToolResult { ref name, ref result })
                if name == "toolu_1" && result == "ok"
        ));
    }

    #[test]
    fn parse_claude_stream_falls_back_for_plain_text() {
        let (display, event) = parse_claude_stream_line("plain\n");
        assert_eq!(display, "plain\n");
        assert!(event.is_none());
    }

    fn run_guard(settings_path: &Path, payload: &serde_json::Value) -> std::process::Output {
        let settings: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(settings_path).unwrap()).unwrap();
        let command = settings["hooks"]["PreToolUse"][0]["hooks"][0]["command"]
            .as_str()
            .unwrap();
        let mut child = Command::new(command)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(serde_json::to_string(payload).unwrap().as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    }

    #[test]
    fn claude_guard_allows_paths_inside_task_dir() {
        let dir = temp_task_dir();
        let settings_path = ensure_claude_guard(&dir).unwrap();
        let payload = serde_json::json!({
            "tool_name": "Write",
            "tool_input": {
                "file_path": dir.join("index.html").to_string_lossy()
            }
        });

        let output = run_guard(&settings_path, &payload);
        assert!(output.status.success());
    }

    #[test]
    fn claude_guard_blocks_paths_outside_task_dir() {
        let dir = temp_task_dir();
        let settings_path = ensure_claude_guard(&dir).unwrap();
        let payload = serde_json::json!({
            "tool_name": "Write",
            "tool_input": {
                "file_path": "/tmp/outside-one-task.txt"
            }
        });

        let output = run_guard(&settings_path, &payload);
        assert_eq!(output.status.code(), Some(2));
        assert!(String::from_utf8_lossy(&output.stderr).contains("outside"));
    }

    #[test]
    fn claude_guard_blocks_destructive_shell_commands() {
        let dir = temp_task_dir();
        let settings_path = ensure_claude_guard(&dir).unwrap();
        let payload = serde_json::json!({
            "tool_name": "Bash",
            "tool_input": {
                "command": "sudo rm -rf /"
            }
        });

        let output = run_guard(&settings_path, &payload);
        assert_eq!(output.status.code(), Some(2));
        assert!(String::from_utf8_lossy(&output.stderr).contains("destructive"));
    }
}
