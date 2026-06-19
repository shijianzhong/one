use std::path::{Path, PathBuf};
use std::process::Stdio;

use gpui::Context;
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::memory::types::ChatMessage;
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
    pub(crate) stage: CodingWorkflowStage,
    pub(crate) plan_text: Option<String>,
    pub(crate) approval_notes: Vec<String>,
}

#[derive(Debug)]
enum CodingRunnerEvent {
    Output(String),
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
        let state = CodingWorkflowState {
            task_id,
            task_dir: task_dir.clone(),
            user_request,
            main_agent_summary,
            known_constraints,
            suggested_direction,
            clarification_focus,
            plan_path: task_dir.join("CLAUDE_PLAN.md"),
            log_path: task_dir.join("claude-code.log"),
            stage: CodingWorkflowStage::PlanningRunning,
            plan_text: None,
            approval_notes: Vec::new(),
        };

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
                    CodingRunnerEvent::Finished {
                        success,
                        output,
                        error,
                    } => {
                        this.finish_coding_runner(kind, success, output, error, cx);
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
                self.job_manager.coding_workflow = Some(state);
                let summary = summarize_plan_for_chat(&plan);
                self.append_active_task_message("assistant", &summary, cx);
            }
            CodingRunKind::Implementation => {
                state.stage = CodingWorkflowStage::Done;
                self.job_manager.coding_workflow = Some(state);
                let summary = summarize_implementation_for_chat(&output);
                self.append_active_task_message("assistant", &summary, cx);
            }
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
}

async fn run_claude_code(
    task_dir: PathBuf,
    log_path: PathBuf,
    prompt: String,
    auto_accept: bool,
    mut cancel_rx: tokio::sync::oneshot::Receiver<()>,
    sender: tokio::sync::mpsc::UnboundedSender<CodingRunnerEvent>,
) {
    let mut command = tokio::process::Command::new("claude");
    command
        .arg("-p")
        .arg(prompt)
        .current_dir(&task_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
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
                if let Some(line) = line {
                    output.push_str(&line);
                    append_log(&log_path, &line);
                    let _ = sender.send(CodingRunnerEvent::Output(line));
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
                        while let Some(line) = line_rx.recv().await {
                            output.push_str(&line);
                            append_log(&log_path, &line);
                            let _ = sender.send(CodingRunnerEvent::Output(line));
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

fn summarize_implementation_for_chat(output: &str) -> String {
    let preview = truncate_chars(output.trim(), 1800);
    format!(
        "Claude Code 编码阶段已结束。下面是终端输出摘要：\n\n{}\n\n完整日志已保存到当前 task 目录的 `claude-code.log`。",
        preview
    )
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
