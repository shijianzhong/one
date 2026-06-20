use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::{anyhow, Result};

use crate::run_log::{RunEvent, RunKind, RunRecorder, RunStatus};
use crate::runtime::coding_supervisor::{
    capture_workspace_snapshot, diff_workspace_snapshot, CodingSupervisionRequest,
    CodingSupervisorDecision, CodingSupervisorState, WorkspaceDelta, WorkspaceSnapshot,
};
use crate::services::{default_coding_agents, CodingAgentConfig};
use crate::terminal_emulator::TerminalEmulator;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CodingAgentProvider {
    pub(crate) id: String,
    pub(crate) label: String,
    pub(crate) command: String,
    pub(crate) args: Vec<String>,
    pub(crate) install_command: Option<String>,
    pub(crate) install_instructions: Option<String>,
}

impl CodingAgentProvider {
    pub(crate) fn command(&self) -> &str {
        &self.command
    }

    pub(crate) fn label(&self) -> &str {
        &self.label
    }

    pub(crate) fn command_line(&self) -> String {
        std::iter::once(self.command.as_str())
            .chain(self.args.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub(crate) fn install_instructions(&self) -> String {
        self.install_instructions.clone().unwrap_or_else(|| {
            format!(
                "请安装 {} 并确保 `{}` 在 PATH 中可用。",
                self.label, self.command
            )
        })
    }
}

impl From<CodingAgentConfig> for CodingAgentProvider {
    fn from(config: CodingAgentConfig) -> Self {
        Self {
            id: normalize_provider_id(&config.id),
            label: if config.label.trim().is_empty() {
                config.id
            } else {
                config.label
            },
            command: config.command,
            args: config.args,
            install_command: config.install_command,
            install_instructions: config.install_instructions,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CodingCliAvailability {
    pub(crate) provider: CodingAgentProvider,
    pub(crate) installed: bool,
    pub(crate) resolved_path: Option<String>,
}

pub(crate) fn configured_coding_agents() -> Vec<CodingAgentProvider> {
    let config = crate::services::load_config();
    let agents = if config.coding_agents.is_empty() {
        default_coding_agents()
    } else {
        config.coding_agents
    };
    agents
        .into_iter()
        .filter(|agent| !agent.id.trim().is_empty() && !agent.command.trim().is_empty())
        .map(CodingAgentProvider::from)
        .collect()
}

pub(crate) fn configured_coding_agent_usage() -> String {
    let ids = configured_coding_agents()
        .into_iter()
        .map(|agent| agent.id)
        .collect::<Vec<_>>();
    if ids.is_empty() {
        "claude".to_string()
    } else {
        ids.join("|")
    }
}

pub(crate) fn resolve_coding_agent_provider(value: &str) -> Option<CodingAgentProvider> {
    let requested = normalize_provider_id(value);
    configured_coding_agents()
        .into_iter()
        .find(|agent| normalize_provider_id(&agent.id) == requested)
        .or_else(|| {
            if matches!(requested.as_str(), "claude-code" | "claude_code") {
                configured_coding_agents()
                    .into_iter()
                    .find(|agent| agent.id == "claude")
            } else {
                None
            }
        })
}

pub(crate) fn default_coding_agent_provider() -> CodingAgentProvider {
    resolve_coding_agent_provider("claude")
        .or_else(|| configured_coding_agents().into_iter().next())
        .unwrap_or_else(|| CodingAgentProvider {
            id: "claude".to_string(),
            label: "Claude".to_string(),
            command: "claude".to_string(),
            args: Vec::new(),
            install_command: Some("curl -fsSL https://claude.ai/install.sh | bash".to_string()),
            install_instructions: Some(
                "Claude Code 官方安装：macOS/Linux/WSL 可运行 `curl -fsSL https://claude.ai/install.sh | bash`，或 macOS 使用 `brew install --cask claude-code`。安装后在项目目录运行 `claude` 并按提示登录。文档：https://code.claude.com/docs"
                    .to_string(),
            ),
        })
}

fn normalize_provider_id(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

pub(crate) fn detect_coding_cli(provider: &CodingAgentProvider) -> CodingCliAvailability {
    let resolved_path = resolve_command_path(&provider.command);
    CodingCliAvailability {
        provider: provider.clone(),
        installed: resolved_path.is_some(),
        resolved_path,
    }
}

pub(crate) fn detect_configured_coding_clis() -> Vec<CodingCliAvailability> {
    configured_coding_agents()
        .into_iter()
        .map(|provider| detect_coding_cli(&provider))
        .collect()
}

fn resolve_command_path(command: &str) -> Option<String> {
    let command = command.trim();
    if command.is_empty() {
        return None;
    }
    if command.contains(std::path::MAIN_SEPARATOR) {
        let path = std::path::Path::new(command);
        return path.is_file().then(|| path.to_string_lossy().to_string());
    }
    let output = std::process::Command::new("which")
        .arg(command)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!path.is_empty()).then_some(path)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PersistentSessionStatus {
    Starting,
    Running,
    WaitingInput,
    Idle,
    Exited,
    Failed,
    Stopped,
}

impl PersistentSessionStatus {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Running => "running",
            Self::WaitingInput => "waiting_input",
            Self::Idle => "idle",
            Self::Exited => "exited",
            Self::Failed => "failed",
            Self::Stopped => "stopped",
        }
    }

    pub(crate) fn is_active(self) -> bool {
        matches!(
            self,
            Self::Starting | Self::Running | Self::WaitingInput | Self::Idle
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GitBaseline {
    pub(crate) branch: Option<String>,
    pub(crate) head: Option<String>,
    pub(crate) status_short: String,
}

#[derive(Clone)]
pub(crate) struct PersistentCliSession {
    pub(crate) session_id: String,
    pub(crate) workspace_id: usize,
    pub(crate) task_id: usize,
    pub(crate) agent_kind: CodingAgentProvider,
    pub(crate) cwd: PathBuf,
    pub(crate) status: PersistentSessionStatus,
    pub(crate) write_mode: bool,
    pub(crate) started_at: chrono::DateTime<chrono::Local>,
    pub(crate) last_active_at: chrono::DateTime<chrono::Local>,
    pub(crate) output_seq: u64,
    pub(crate) terminal: Arc<Mutex<TerminalEmulator>>,
    pub(crate) git_baseline: Option<GitBaseline>,
    pub(crate) workspace_baseline: Option<WorkspaceSnapshot>,
    pub(crate) run_id: Option<usize>,
    pub(crate) last_error: Option<String>,
    pub(crate) last_user_action_fingerprint: Option<String>,
    pub(crate) submitted_task: Option<String>,
    pub(crate) supervisor_in_flight: bool,
    pub(crate) last_supervised_fingerprint: Option<String>,
    pub(crate) last_notified_fingerprint: Option<String>,
}

impl PersistentCliSession {
    pub(crate) fn status_label(&self) -> &'static str {
        self.status.label()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PersistentCliSessionSummary {
    pub(crate) session_id: String,
    pub(crate) workspace_id: usize,
    pub(crate) task_id: usize,
    pub(crate) agent_kind: CodingAgentProvider,
    pub(crate) cwd: PathBuf,
    pub(crate) status: PersistentSessionStatus,
    pub(crate) write_mode: bool,
    pub(crate) output_seq: u64,
    pub(crate) last_error: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) enum CodingSessionNotification {
    UserAction { task_id: usize, message: String },
    Completed { task_id: usize, message: String },
    Failed { task_id: usize, message: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CodingRuntimeInspection {
    pub(crate) session_id: String,
    pub(crate) status: String,
    pub(crate) kind: String,
    pub(crate) summary: String,
    pub(crate) suggested_message: String,
    pub(crate) recent_output: Vec<String>,
    pub(crate) fingerprint: Option<String>,
}

pub(crate) enum PendingCodingActionReply {
    Sent {
        session_id: String,
        choice: String,
        meaning: String,
    },
    NeedsExplicitChoice {
        message: String,
    },
}

impl From<&PersistentCliSession> for PersistentCliSessionSummary {
    fn from(session: &PersistentCliSession) -> Self {
        Self {
            session_id: session.session_id.clone(),
            workspace_id: session.workspace_id,
            task_id: session.task_id,
            agent_kind: session.agent_kind.clone(),
            cwd: session.cwd.clone(),
            status: session.status,
            write_mode: session.write_mode,
            output_seq: session.output_seq,
            last_error: session.last_error.clone(),
        }
    }
}

pub(crate) struct PersistentCliSessionManager {
    sessions: HashMap<String, PersistentCliSession>,
    task_attached_session: HashMap<usize, String>,
    workspace_write_owner: HashMap<usize, String>,
    next_session_seq: u64,
}

static GLOBAL_CODING_SESSION_MANAGER: OnceLock<Arc<Mutex<PersistentCliSessionManager>>> =
    OnceLock::new();

pub(crate) fn global_coding_session_manager() -> Arc<Mutex<PersistentCliSessionManager>> {
    GLOBAL_CODING_SESSION_MANAGER
        .get_or_init(|| Arc::new(Mutex::new(PersistentCliSessionManager::new())))
        .clone()
}

impl PersistentCliSessionManager {
    pub(crate) fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            task_attached_session: HashMap::new(),
            workspace_write_owner: HashMap::new(),
            next_session_seq: 0,
        }
    }

    pub(crate) fn start_session(
        &mut self,
        db_conn: &sqlez::connection::Connection,
        task_id: usize,
        workspace_id: usize,
        agent_kind: CodingAgentProvider,
        cwd: PathBuf,
        write_mode: bool,
        initial_input: Option<&str>,
    ) -> Result<String> {
        self.start_session_with_program(
            db_conn,
            task_id,
            workspace_id,
            agent_kind.clone(),
            &agent_kind.command_line(),
            cwd,
            write_mode,
            initial_input,
        )
    }

    fn start_session_with_program(
        &mut self,
        db_conn: &sqlez::connection::Connection,
        task_id: usize,
        workspace_id: usize,
        agent_kind: CodingAgentProvider,
        command_line: &str,
        cwd: PathBuf,
        write_mode: bool,
        initial_input: Option<&str>,
    ) -> Result<String> {
        if write_mode {
            if let Some(owner) = self.active_write_session_for_workspace(workspace_id) {
                return Err(anyhow!(
                    "workspace {} already has write-active session {}",
                    workspace_id,
                    owner.session_id
                ));
            }
        }

        std::fs::create_dir_all(&cwd)?;
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "sh".to_string());
        let terminal = TerminalEmulator::new(Some(&shell), Some(&cwd), 100, 30).map_err(|e| {
            anyhow!(
                "failed to start shell {} for {}: {}",
                shell,
                command_line,
                e
            )
        })?;

        self.next_session_seq += 1;
        let session_id = format!(
            "{}-{}-{}",
            agent_kind.id.as_str(),
            task_id,
            self.next_session_seq
        );
        let baseline = if write_mode {
            Some(read_git_baseline(&cwd))
        } else {
            None
        };
        let workspace_baseline = write_mode.then(|| capture_workspace_snapshot(&cwd));
        let run_id = RunRecorder::begin(
            db_conn,
            task_id,
            RunKind::ClaudeCode,
            format!(
                "{} terminal runtime in {}",
                agent_kind.label(),
                cwd.to_string_lossy()
            ),
        );
        if let Some(run_id) = run_id {
            let recorder = RunRecorder::attach(db_conn, run_id);
            recorder.record(&RunEvent::MessageDelta {
                text: format!(
                    "Started shell and requested {} runtime with `{}` at {}",
                    agent_kind.label(),
                    command_line,
                    cwd.to_string_lossy()
                ),
            });
            if let Some(baseline) = &baseline {
                recorder.record(&RunEvent::MessageDelta {
                    text: format!(
                        "Git baseline: branch={:?}, head={:?}\n{}",
                        baseline.branch, baseline.head, baseline.status_short
                    ),
                });
            }
        }

        let terminal = Arc::new(Mutex::new(terminal));
        let now = chrono::Local::now();
        let wait_for_ready = should_wait_for_runtime_ready(&agent_kind);
        let agent_label = agent_kind.label().to_string();
        let session = PersistentCliSession {
            session_id: session_id.clone(),
            workspace_id,
            task_id,
            agent_kind,
            cwd,
            status: PersistentSessionStatus::Running,
            write_mode,
            started_at: now,
            last_active_at: now,
            output_seq: 0,
            terminal: terminal.clone(),
            git_baseline: baseline,
            workspace_baseline,
            run_id,
            last_error: None,
            last_user_action_fingerprint: None,
            submitted_task: initial_input.map(ToOwned::to_owned),
            supervisor_in_flight: false,
            last_supervised_fingerprint: None,
            last_notified_fingerprint: None,
        };
        self.sessions.insert(session_id.clone(), session);
        self.task_attached_session
            .insert(task_id, session_id.clone());
        if write_mode {
            self.workspace_write_owner
                .insert(workspace_id, session_id.clone());
        }
        self.send_command_line(db_conn, &session_id, command_line)?;
        if let Some(input) = initial_input {
            if wait_for_ready {
                match wait_for_runtime_ready(
                    &session_id,
                    &agent_label,
                    &terminal,
                    std::time::Duration::from_secs(20),
                )? {
                    RuntimeReadyState::Ready(ready) => {
                        if let Some(run_id) = run_id {
                            RunRecorder::attach(db_conn, run_id).record(&RunEvent::MessageDelta {
                                text: format!("{} runtime ready: {}", agent_label, ready),
                            });
                        }
                    }
                    RuntimeReadyState::NeedsUserAction(inspection) => {
                        if let Some(session) = self.sessions.get_mut(&session_id) {
                            session.status = PersistentSessionStatus::WaitingInput;
                            session.last_error = Some(inspection.suggested_message.clone());
                        }
                        if let Some(run_id) = run_id {
                            RunRecorder::attach(db_conn, run_id).record(&RunEvent::MessageDelta {
                                text: format!(
                                    "{} runtime needs user action before task input: {}",
                                    agent_label, inspection.suggested_message
                                ),
                            });
                        }
                        return Ok(session_id);
                    }
                }
            }
            self.send_input(db_conn, &session_id, input)?;
        }
        Ok(session_id)
    }

    pub(crate) fn send_input(
        &mut self,
        db_conn: &sqlez::connection::Connection,
        session_id: &str,
        text: &str,
    ) -> Result<()> {
        let session = self
            .sessions
            .get_mut(session_id)
            .ok_or_else(|| anyhow!("session not found: {}", session_id))?;
        if !session.status.is_active() {
            return Err(anyhow!(
                "session {} is not active ({})",
                session_id,
                session.status_label()
            ));
        }
        {
            let terminal = session
                .terminal
                .lock()
                .map_err(|_| anyhow!("terminal lock poisoned"))?;
            terminal.write_interactive_prompt(text);
        }
        session.status = PersistentSessionStatus::Running;
        session.last_active_at = chrono::Local::now();
        session.output_seq = session.output_seq.saturating_add(1);
        session.submitted_task = Some(text.to_string());
        session.supervisor_in_flight = false;
        session.last_supervised_fingerprint = None;
        session.last_notified_fingerprint = None;
        if let Some(run_id) = session.run_id {
            RunRecorder::attach(db_conn, run_id).record(&RunEvent::MessageDelta {
                text: format!("USER INPUT:\n{}", text),
            });
        }
        Ok(())
    }

    pub(crate) fn reply_to_pending_user_action(
        &mut self,
        db_conn: &sqlez::connection::Connection,
        task_id: usize,
        user_message: &str,
    ) -> Result<Option<PendingCodingActionReply>> {
        let Some(session_id) = self.attached_session_id_for_task(task_id) else {
            return Ok(None);
        };
        self.refresh_session_status(db_conn, &session_id);
        let inspection = self.inspect_runtime(db_conn, &session_id, 80)?;
        if !requires_user_action(&inspection.kind) {
            return Ok(None);
        }
        let Some(choice) = map_user_message_to_choice(user_message) else {
            if inspection.kind == "choice_required" && asks_agent_to_choose(user_message) {
                return Ok(Some(PendingCodingActionReply::NeedsExplicitChoice {
                    message: format!(
                        "{}\n\n你可以直接回复“同意/选1”“选2”或“拒绝/选3”，我会帮你发送到右侧终端。",
                        inspection.suggested_message
                    ),
                }));
            }
            return Ok(None);
        };
        let meaning = match choice.as_str() {
            "1" => "允许这一次操作",
            "2" => "本次会话后续类似编辑都允许",
            "3" => "拒绝这次操作",
            _ => "已选择",
        }
        .to_string();
        self.send_choice(db_conn, &session_id, &choice)?;
        if let Some(session) = self.sessions.get_mut(&session_id) {
            session.last_user_action_fingerprint = inspection.fingerprint.or_else(|| {
                Some(format!(
                    "{}:{}:{}",
                    inspection.session_id, inspection.kind, inspection.summary
                ))
            });
        }
        Ok(Some(PendingCodingActionReply::Sent {
            session_id,
            choice,
            meaning,
        }))
    }

    pub(crate) fn collect_supervision_requests(
        &mut self,
        db_conn: &sqlez::connection::Connection,
        limit: usize,
    ) -> Vec<CodingSupervisionRequest> {
        let session_ids = self.sessions.keys().cloned().collect::<Vec<_>>();
        let mut requests = Vec::new();
        for session_id in session_ids {
            self.refresh_session_status(db_conn, &session_id);
            let Some(session) = self.sessions.get(&session_id) else {
                continue;
            };
            if !session.status.is_active() || session.supervisor_in_flight {
                continue;
            }
            let Some(submitted_task) = session.submitted_task.clone() else {
                continue;
            };
            let lines = match session.terminal.lock() {
                Ok(terminal) => {
                    let mut lines = terminal.screen_text_lines();
                    lines.retain(|line| !line.trim().is_empty());
                    if lines.len() > limit {
                        lines.split_off(lines.len() - limit)
                    } else {
                        lines
                    }
                }
                Err(_) => continue,
            };
            if lines.is_empty() {
                continue;
            }
            let workspace_delta = session
                .workspace_baseline
                .as_ref()
                .map(|baseline| {
                    let current = capture_workspace_snapshot(&session.cwd);
                    diff_workspace_snapshot(baseline, &current)
                })
                .unwrap_or_else(|| WorkspaceDelta {
                    added: Vec::new(),
                    modified: Vec::new(),
                    deleted: Vec::new(),
                });
            let fingerprint = supervision_fingerprint(
                &session.session_id,
                session.output_seq,
                &lines,
                &workspace_delta.describe(),
            );
            if session.last_supervised_fingerprint.as_deref() == Some(fingerprint.as_str()) {
                continue;
            }
            let request = CodingSupervisionRequest {
                session_id: session.session_id.clone(),
                agent_label: session.agent_kind.label().to_string(),
                cwd: session.cwd.clone(),
                submitted_task,
                terminal_transcript: lines,
                workspace_delta,
                fingerprint: fingerprint.clone(),
            };
            if let Some(session) = self.sessions.get_mut(&session_id) {
                session.supervisor_in_flight = true;
            }
            requests.push(request);
        }
        requests
    }

    pub(crate) fn apply_supervision_decision(
        &mut self,
        request: &CodingSupervisionRequest,
        decision: CodingSupervisorDecision,
    ) -> Option<CodingSessionNotification> {
        let session = self.sessions.get_mut(&request.session_id)?;
        session.supervisor_in_flight = false;
        session.last_supervised_fingerprint = Some(request.fingerprint.clone());
        if decision.confidence < 60 {
            return None;
        }
        match decision.state {
            CodingSupervisorState::Running | CodingSupervisorState::Unclear => None,
            CodingSupervisorState::WaitingUser => {
                let notify_fingerprint = format!("waiting:{}", request.fingerprint);
                if session.last_notified_fingerprint.as_deref() == Some(notify_fingerprint.as_str())
                {
                    return None;
                }
                session.status = PersistentSessionStatus::WaitingInput;
                session.last_notified_fingerprint = Some(notify_fingerprint);
                Some(CodingSessionNotification::UserAction {
                    task_id: session.task_id,
                    message: normalize_supervisor_message(
                        &decision.user_message,
                        "Claude Code 需要你确认下一步。",
                    ),
                })
            }
            CodingSupervisorState::Completed => {
                let notify_fingerprint = format!("completed:{}", request.fingerprint);
                if session.last_notified_fingerprint.as_deref() == Some(notify_fingerprint.as_str())
                {
                    return None;
                }
                session.status = PersistentSessionStatus::Idle;
                session.last_notified_fingerprint = Some(notify_fingerprint);
                if session.write_mode {
                    if let Some(owner) = self.workspace_write_owner.get(&session.workspace_id) {
                        if owner == &session.session_id {
                            self.workspace_write_owner.remove(&session.workspace_id);
                        }
                    }
                }
                Some(CodingSessionNotification::Completed {
                    task_id: session.task_id,
                    message: normalize_supervisor_message(
                        &decision.user_message,
                        "Claude Code 已完成这次任务。",
                    ),
                })
            }
            CodingSupervisorState::Failed => {
                let notify_fingerprint = format!("failed:{}", request.fingerprint);
                if session.last_notified_fingerprint.as_deref() == Some(notify_fingerprint.as_str())
                {
                    return None;
                }
                session.status = PersistentSessionStatus::Failed;
                session.last_notified_fingerprint = Some(notify_fingerprint);
                session.last_error = Some(decision.user_message.clone());
                Some(CodingSessionNotification::Failed {
                    task_id: session.task_id,
                    message: normalize_supervisor_message(
                        &decision.user_message,
                        "Claude Code 执行失败，需要你查看或调整需求。",
                    ),
                })
            }
        }
    }

    pub(crate) fn mark_supervision_failed(&mut self, session_id: &str, fingerprint: &str) {
        if let Some(session) = self.sessions.get_mut(session_id) {
            session.supervisor_in_flight = false;
            session.last_supervised_fingerprint = Some(fingerprint.to_string());
        }
    }

    fn send_choice(
        &mut self,
        db_conn: &sqlez::connection::Connection,
        session_id: &str,
        text: &str,
    ) -> Result<()> {
        let session = self
            .sessions
            .get_mut(session_id)
            .ok_or_else(|| anyhow!("session not found: {}", session_id))?;
        if !session.status.is_active() {
            return Err(anyhow!(
                "session {} is not active ({})",
                session_id,
                session.status_label()
            ));
        }
        {
            let terminal = session
                .terminal
                .lock()
                .map_err(|_| anyhow!("terminal lock poisoned"))?;
            terminal.write_interactive_choice(text);
        }
        session.status = PersistentSessionStatus::Running;
        session.last_active_at = chrono::Local::now();
        session.output_seq = session.output_seq.saturating_add(1);
        if let Some(run_id) = session.run_id {
            RunRecorder::attach(db_conn, run_id).record(&RunEvent::MessageDelta {
                text: format!("USER CHOICE:\n{}", text),
            });
        }
        Ok(())
    }

    fn send_command_line(
        &mut self,
        db_conn: &sqlez::connection::Connection,
        session_id: &str,
        text: &str,
    ) -> Result<()> {
        let session = self
            .sessions
            .get_mut(session_id)
            .ok_or_else(|| anyhow!("session not found: {}", session_id))?;
        if !session.status.is_active() {
            return Err(anyhow!(
                "session {} is not active ({})",
                session_id,
                session.status_label()
            ));
        }
        {
            let terminal = session
                .terminal
                .lock()
                .map_err(|_| anyhow!("terminal lock poisoned"))?;
            terminal.write_command_line(text);
        }
        session.status = PersistentSessionStatus::Running;
        session.last_active_at = chrono::Local::now();
        session.output_seq = session.output_seq.saturating_add(1);
        if let Some(run_id) = session.run_id {
            RunRecorder::attach(db_conn, run_id).record(&RunEvent::MessageDelta {
                text: format!("SHELL COMMAND:\n{}", text),
            });
        }
        Ok(())
    }

    pub(crate) fn read_recent_output(
        &mut self,
        db_conn: &sqlez::connection::Connection,
        session_id: &str,
        limit: usize,
    ) -> Result<Vec<String>> {
        self.refresh_session_status(db_conn, session_id);
        let session = self
            .sessions
            .get(session_id)
            .ok_or_else(|| anyhow!("session not found: {}", session_id))?;
        let terminal = session
            .terminal
            .lock()
            .map_err(|_| anyhow!("terminal lock poisoned"))?;
        let mut lines = terminal.screen_text_lines();
        lines.retain(|line| !line.trim().is_empty());
        if lines.len() > limit {
            Ok(lines.split_off(lines.len() - limit))
        } else {
            Ok(lines)
        }
    }

    pub(crate) fn inspect_runtime(
        &mut self,
        db_conn: &sqlez::connection::Connection,
        session_id: &str,
        limit: usize,
    ) -> Result<CodingRuntimeInspection> {
        self.refresh_session_status(db_conn, session_id);
        let session = self
            .sessions
            .get(session_id)
            .ok_or_else(|| anyhow!("session not found: {}", session_id))?;
        let terminal = session
            .terminal
            .lock()
            .map_err(|_| anyhow!("terminal lock poisoned"))?;
        let mut lines = terminal.screen_text_lines();
        lines.retain(|line| !line.trim().is_empty());
        if lines.len() > limit {
            lines = lines.split_off(lines.len() - limit);
        }
        Ok(inspect_terminal_lines(session, lines))
    }

    pub(crate) fn stop_session(
        &mut self,
        db_conn: &sqlez::connection::Connection,
        session_id: &str,
    ) -> Result<()> {
        let Some(session) = self.sessions.get_mut(session_id) else {
            return Err(anyhow!("session not found: {}", session_id));
        };
        {
            let mut terminal = session
                .terminal
                .lock()
                .map_err(|_| anyhow!("terminal lock poisoned"))?;
            terminal.shutdown();
        }
        session.status = PersistentSessionStatus::Stopped;
        session.last_active_at = chrono::Local::now();
        if let Some(owner) = self.workspace_write_owner.get(&session.workspace_id) {
            if owner == session_id {
                self.workspace_write_owner.remove(&session.workspace_id);
            }
        }
        if let Some(run_id) = session.run_id {
            let recorder = RunRecorder::attach(db_conn, run_id);
            recorder.record(&RunEvent::Finished {
                result: "Persistent coding session stopped.".to_string(),
            });
            recorder.finish(RunStatus::Cancelled);
        }
        Ok(())
    }

    pub(crate) fn stop_all_sessions(&mut self, db_conn: &sqlez::connection::Connection) {
        let session_ids: Vec<String> = self.sessions.keys().cloned().collect();
        for session_id in session_ids {
            let _ = self.stop_session(db_conn, &session_id);
        }
    }

    pub(crate) fn list_sessions(&self) -> Vec<PersistentCliSessionSummary> {
        self.sessions
            .values()
            .map(PersistentCliSessionSummary::from)
            .collect()
    }

    pub(crate) fn session_for_task(&self, task_id: usize) -> Option<&PersistentCliSession> {
        let session_id = self.task_attached_session.get(&task_id)?;
        self.sessions.get(session_id)
    }

    pub(crate) fn attached_session_id_for_task(&self, task_id: usize) -> Option<String> {
        self.task_attached_session.get(&task_id).cloned()
    }

    pub(crate) fn attach_task_session(&mut self, task_id: usize, session_id: &str) -> Result<()> {
        if !self.sessions.contains_key(session_id) {
            return Err(anyhow!("session not found: {}", session_id));
        }
        self.task_attached_session
            .insert(task_id, session_id.to_string());
        Ok(())
    }

    pub(crate) fn active_write_session_for_workspace(
        &self,
        workspace_id: usize,
    ) -> Option<&PersistentCliSession> {
        let session_id = self.workspace_write_owner.get(&workspace_id)?;
        let session = self.sessions.get(session_id)?;
        if session.write_mode && session.status.is_active() {
            Some(session)
        } else {
            None
        }
    }

    pub(crate) fn refresh_all(&mut self, db_conn: &sqlez::connection::Connection) {
        let ids: Vec<String> = self.sessions.keys().cloned().collect();
        for id in ids {
            self.refresh_session_status(db_conn, &id);
        }
    }

    pub(crate) fn refresh_session_status(
        &mut self,
        db_conn: &sqlez::connection::Connection,
        session_id: &str,
    ) {
        let Some(session) = self.sessions.get_mut(session_id) else {
            return;
        };
        let exited = {
            match session.terminal.lock() {
                Ok(mut terminal) => {
                    terminal.process_events();
                    terminal.is_exited()
                }
                Err(_) => {
                    session.status = PersistentSessionStatus::Failed;
                    session.last_error = Some("terminal lock poisoned".to_string());
                    true
                }
            }
        };
        if exited && session.status.is_active() {
            session.status = PersistentSessionStatus::Exited;
            session.last_active_at = chrono::Local::now();
            if let Some(owner) = self.workspace_write_owner.get(&session.workspace_id) {
                if owner == session_id {
                    self.workspace_write_owner.remove(&session.workspace_id);
                }
            }
            if let Some(run_id) = session.run_id {
                let recorder = RunRecorder::attach(db_conn, run_id);
                recorder.record(&RunEvent::Finished {
                    result: "Persistent coding session exited.".to_string(),
                });
                recorder.finish(RunStatus::Finished);
            }
        }
    }
}

fn read_git_baseline(cwd: &Path) -> GitBaseline {
    GitBaseline {
        branch: git_output(cwd, &["rev-parse", "--abbrev-ref", "HEAD"]),
        head: git_output(cwd, &["rev-parse", "HEAD"]),
        status_short: git_output(cwd, &["status", "--short"]).unwrap_or_default(),
    }
}

fn git_output(cwd: &Path, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn should_wait_for_runtime_ready(agent_kind: &CodingAgentProvider) -> bool {
    agent_kind.command.trim() == "claude"
}

enum RuntimeReadyState {
    Ready(String),
    NeedsUserAction(CodingRuntimeInspection),
}

fn wait_for_runtime_ready(
    session_id: &str,
    agent_label: &str,
    terminal: &Arc<Mutex<TerminalEmulator>>,
    timeout: std::time::Duration,
) -> Result<RuntimeReadyState> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let lines = {
            let mut terminal = terminal
                .lock()
                .map_err(|_| anyhow!("terminal lock poisoned"))?;
            terminal.process_events();
            if terminal.is_exited() {
                return Err(anyhow!("coding CLI exited before it became ready"));
            }
            terminal.screen_text_lines()
        };
        if let Some(line) = lines.iter().find(|line| is_claude_ready_line(line)) {
            return Ok(RuntimeReadyState::Ready(line.trim().to_string()));
        }
        let recent_output = recent_non_empty_lines(lines.clone(), 20);
        let (status, kind, summary, suggested_message) = classify_terminal_lines(
            PersistentSessionStatus::Running,
            agent_label,
            &recent_output,
        );
        if matches!(
            kind.as_str(),
            "auth_required" | "trust_required" | "permission_required" | "command_missing"
        ) {
            return Ok(RuntimeReadyState::NeedsUserAction(
                CodingRuntimeInspection {
                    session_id: session_id.to_string(),
                    status,
                    kind,
                    summary,
                    suggested_message,
                    recent_output,
                    fingerprint: None,
                },
            ));
        }
        if std::time::Instant::now() >= deadline {
            let recent = recent_non_empty_lines(lines, 8).join("\n");
            return Err(anyhow!(
                "timed out waiting for Claude Code welcome/ready output. Recent terminal output:\n{}",
                recent
            ));
        }
        std::thread::sleep(std::time::Duration::from_millis(120));
    }
}

fn is_claude_ready_line(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.contains("welcome to claude code")
        || lower.contains("claude code")
        || lower.contains("? for shortcuts")
        || lower.contains("type /help")
}

fn inspect_terminal_lines(
    session: &PersistentCliSession,
    recent_output: Vec<String>,
) -> CodingRuntimeInspection {
    let (status, kind, summary, suggested_message) =
        classify_terminal_lines(session.status, session.agent_kind.label(), &recent_output);

    CodingRuntimeInspection {
        session_id: session.session_id.clone(),
        status,
        kind,
        summary,
        suggested_message,
        fingerprint: user_action_fingerprint(&recent_output),
        recent_output,
    }
}

fn classify_terminal_lines(
    session_status: PersistentSessionStatus,
    agent_label: &str,
    recent_output: &[String],
) -> (String, String, String, String) {
    let joined = recent_output.join("\n");
    let lower = joined.to_ascii_lowercase();
    let tail_output = tail_lines(recent_output, 12);
    let tail_joined = tail_output.join("\n");
    let tail_lower = tail_joined.to_ascii_lowercase();
    if !session_status.is_active() {
        (
            session_status.label().to_string(),
            "not_active".to_string(),
            format!("{} runtime is {}.", agent_label, session_status.label()),
            "该终端 runtime 已不活跃；如需继续编码，请重新启动。".to_string(),
        )
    } else if contains_any(
        &lower,
        &[
            "command not found",
            "not recognized",
            "no such file or directory",
        ],
    ) {
        (
            "failed".to_string(),
            "command_missing".to_string(),
            format!("{} command appears to be missing.", agent_label),
            format!(
                "{} 命令不可用。请检查安装与 PATH，然后重新启动。",
                agent_label
            ),
        )
    } else if let Some(choice) = parse_numbered_choice_prompt(recent_output) {
        (
            "waiting_user_action".to_string(),
            "choice_required".to_string(),
            choice.summary,
            choice.suggested_message,
        )
    } else if looks_like_auth_required(&tail_lower) {
        (
            "waiting_user_action".to_string(),
            "auth_required".to_string(),
            format!("{} is waiting for authentication.", agent_label),
            "Claude Code 正在等待登录/认证。这个通常需要你在右侧终端或浏览器里完成登录；完成后告诉我继续。".to_string(),
        )
    } else if contains_any(
        &tail_lower,
        &[
            "trust this",
            "do you trust",
            "trusted workspace",
            "trust the files",
        ],
    ) {
        (
            "waiting_user_action".to_string(),
            "trust_required".to_string(),
            format!(
                "{} is asking for workspace trust confirmation.",
                agent_label
            ),
            "Claude Code 正在等待目录信任确认。如果终端里有编号选项，你可以直接在这里回复“同意/选1/拒绝”，我会帮你发送；如果它要求交互式登录或特殊按键，你也可以在右侧终端操作。".to_string(),
        )
    } else if contains_any(
        &tail_lower,
        &[
            "allow",
            "deny",
            "yes/no",
            "y/n",
            "approve",
            "permission",
            "permissions",
        ],
    ) {
        (
            "waiting_user_action".to_string(),
            "permission_required".to_string(),
            format!("{} is waiting for a permission decision.", agent_label),
            "Claude Code 正在等待权限确认。你可以直接在这里回复“同意/选1”“全部允许/选2”或“拒绝/选3”，我会帮你发送到右侧终端。".to_string(),
        )
    } else if recent_output.iter().any(|line| is_claude_ready_line(line)) {
        (
            "ready".to_string(),
            "ready_for_input".to_string(),
            format!("{} appears ready for input.", agent_label),
            "Claude Code 已就绪，可以把用户需求整理后发送给它。".to_string(),
        )
    } else if contains_any(
        &lower,
        &[
            "thinking",
            "working",
            "running",
            "esc to interrupt",
            "ctrl-c",
            "processing",
        ],
    ) {
        (
            "running".to_string(),
            "busy".to_string(),
            format!("{} appears to be working.", agent_label),
            "Claude Code 正在处理任务。可以稍后再次读取输出并总结进度。".to_string(),
        )
    } else {
        (
            session_status.label().to_string(),
            "unknown".to_string(),
            format!(
                "{} runtime is active, but no specific state was recognized.",
                agent_label
            ),
            "已读取终端输出，但没有识别到明确状态。请基于 recent_output 判断下一步。".to_string(),
        )
    }
}

struct ChoicePrompt {
    summary: String,
    suggested_message: String,
}

fn parse_numbered_choice_prompt(recent_output: &[String]) -> Option<ChoicePrompt> {
    parse_confirmation_choice_prompt(recent_output)
        .or_else(|| parse_menu_choice_prompt(recent_output))
}

fn parse_confirmation_choice_prompt(recent_output: &[String]) -> Option<ChoicePrompt> {
    let question = recent_output
        .iter()
        .rev()
        .find(|line| {
            let trimmed = line.trim();
            let lower = trimmed.to_ascii_lowercase();
            trimmed.contains('?')
                && (lower.contains("do you want") || lower.contains("would you like"))
        })?
        .trim()
        .to_string();
    let has_numbered_options = recent_output
        .iter()
        .any(|line| contains_numbered_option(line, "1"))
        && recent_output
            .iter()
            .any(|line| contains_numbered_option(line, "2"));
    if !has_numbered_options {
        return None;
    }

    let question_prefix = question
        .split_once('?')
        .map(|(prefix, _)| prefix)
        .unwrap_or(question.as_str());
    let action = if let Some(target) = extract_after(question_prefix, "Do you want to ") {
        target.to_string()
    } else if let Some(target) = extract_after(question_prefix, "Would you like to ") {
        target.to_string()
    } else {
        question_prefix.to_string()
    };
    let zh_action = translate_common_claude_action(&action);
    Some(ChoicePrompt {
        summary: format!("Claude Code 正在请求确认：{}。", zh_action),
        suggested_message: format!(
            "{}\n\n你可以回复“同意”或“选1”允许这一次；回复“选2”表示本次会话后续类似编辑都允许；回复“拒绝”或“选3”则不允许。",
            zh_action
        ),
    })
}

fn parse_menu_choice_prompt(recent_output: &[String]) -> Option<ChoicePrompt> {
    let has_numbered_options = recent_output
        .iter()
        .any(|line| contains_numbered_option(line, "1"))
        && recent_output
            .iter()
            .any(|line| contains_numbered_option(line, "2"));
    if !has_numbered_options {
        return None;
    }
    let has_menu_hint = recent_output.iter().any(|line| {
        let lower = line.to_ascii_lowercase();
        lower.contains("enter to select")
            || lower.contains("tab/arrow keys")
            || lower.contains("请选择")
            || lower.contains("选择")
            || lower.contains("submit")
    });
    if !has_menu_hint {
        return None;
    }
    let question = recent_output
        .iter()
        .rev()
        .find(|line| {
            let trimmed = line.trim();
            let lower = trimmed.to_ascii_lowercase();
            !trimmed.is_empty()
                && !contains_numbered_option(trimmed, "1")
                && !contains_numbered_option(trimmed, "2")
                && !contains_numbered_option(trimmed, "3")
                && !lower.contains("enter to select")
                && !lower.contains("tab/arrow keys")
                && !lower.contains("esc to cancel")
        })
        .map(|line| line.trim().to_string())
        .unwrap_or_else(|| "Claude Code 正在等待你选择一个选项。".to_string());

    Some(ChoicePrompt {
        summary: format!("Claude Code 正在等待菜单选择：{}。", question),
        suggested_message: format!(
            "{}\n\n你可以回复“选1”“选2”“选3”等，我会把对应数字发送到右侧终端；如果你不确定，也可以直接告诉我你的偏好，我再帮你选择。",
            question
        ),
    })
}

fn contains_numbered_option(line: &str, number: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with(&format!("{}.", number))
        || trimmed.starts_with(&format!("{})", number))
        || trimmed.contains(&format!(" {}.", number))
        || trimmed.contains(&format!(" {})", number))
}

fn extract_after<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    text.strip_prefix(prefix)
}

fn translate_common_claude_action(action: &str) -> String {
    let lower = action.to_ascii_lowercase();
    if let Some(file) = lower.strip_prefix("create ") {
        format!("它想创建 `{}`", file.trim())
    } else if let Some(file) = lower.strip_prefix("edit ") {
        format!("它想编辑 `{}`", file.trim())
    } else if let Some(file) = lower.strip_prefix("overwrite ") {
        format!("它想覆盖 `{}`", file.trim())
    } else {
        format!("它需要你确认：{}", action)
    }
}

fn user_action_fingerprint(recent_output: &[String]) -> Option<String> {
    parse_numbered_choice_prompt(recent_output)
        .map(|choice| format!("choice:{}", choice.summary))
        .or_else(|| {
            let relevant = recent_output
                .iter()
                .rev()
                .find(|line| {
                    let lower = line.to_ascii_lowercase();
                    contains_any(
                        &lower,
                        &[
                            "log in",
                            "login",
                            "sign in",
                            "trust this",
                            "do you trust",
                            "allow",
                            "deny",
                            "permission",
                        ],
                    )
                })
                .map(|line| line.trim().to_string());
            relevant.map(|line| format!("action:{}", line))
        })
}

fn requires_user_action(kind: &str) -> bool {
    matches!(
        kind,
        "auth_required" | "trust_required" | "permission_required" | "choice_required"
    )
}

fn map_user_message_to_choice(message: &str) -> Option<String> {
    let normalized = message
        .trim()
        .to_ascii_lowercase()
        .replace(' ', "")
        .replace('，', ",")
        .replace('。', ".");
    let original = message.trim();
    if normalized.is_empty() {
        return None;
    }
    if matches!(normalized.as_str(), "1" | "选1" | "选择1" | "option1") {
        return Some("1".to_string());
    }
    if matches!(normalized.as_str(), "2" | "选2" | "选择2" | "option2") {
        return Some("2".to_string());
    }
    if matches!(normalized.as_str(), "3" | "选3" | "选择3" | "option3") {
        return Some("3".to_string());
    }
    if contains_any(original, &["全部允许", "都允许", "本次都允许", "一直允许"])
        || contains_any(&normalized, &["allowall", "all"])
    {
        return Some("2".to_string());
    }
    if contains_any(
        original,
        &["同意", "可以", "允许", "确认", "是的", "行", "好", "继续"],
    ) || contains_any(&normalized, &["yes", "y", "ok", "approve", "allow"])
    {
        return Some("1".to_string());
    }
    if contains_any(
        original,
        &["拒绝", "不同意", "不允许", "不要", "取消", "否"],
    ) || contains_any(&normalized, &["no", "n", "deny", "reject"])
    {
        return Some("3".to_string());
    }
    None
}

fn supervision_fingerprint(
    session_id: &str,
    output_seq: u64,
    lines: &[String],
    workspace_delta: &str,
) -> String {
    let mut hasher = DefaultHasher::new();
    session_id.hash(&mut hasher);
    output_seq.hash(&mut hasher);
    workspace_delta.hash(&mut hasher);
    for line in lines.iter().rev().take(80) {
        line.hash(&mut hasher);
    }
    format!("{:x}", hasher.finish())
}

fn normalize_supervisor_message(message: &str, fallback: &str) -> String {
    let message = message.trim();
    if message.is_empty() {
        fallback.to_string()
    } else {
        message.to_string()
    }
}

fn asks_agent_to_choose(message: &str) -> bool {
    contains_any(
        message,
        &[
            "你不能替我选",
            "你能替我选",
            "你帮我选",
            "帮我选",
            "你来选",
            "替我选",
        ],
    )
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

fn looks_like_auth_required(lower_tail: &str) -> bool {
    if contains_any(
        lower_tail,
        &[
            "authenticated",
            "authentication complete",
            "authentication successful",
            "logged in",
            "signed in",
        ],
    ) {
        return false;
    }
    if contains_any(
        lower_tail,
        &[
            "thinking",
            "working",
            "write(",
            "read ",
            "listed ",
            "opened changes",
            "esc to cancel",
        ],
    ) {
        return false;
    }
    contains_any(
        lower_tail,
        &[
            "please log in",
            "please login",
            "log in with",
            "login required",
            "sign in to",
            "please sign in",
            "authenticate with",
            "open your browser",
            "browser login",
        ],
    )
}

fn recent_non_empty_lines(lines: Vec<String>, limit: usize) -> Vec<String> {
    let mut lines = lines
        .into_iter()
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>();
    if lines.len() > limit {
        lines.split_off(lines.len() - limit)
    } else {
        lines
    }
}

fn tail_lines(lines: &[String], limit: usize) -> Vec<String> {
    if lines.len() > limit {
        lines[lines.len() - limit..].to_vec()
    } else {
        lines.to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn provider_resolution_accepts_default_names() {
        assert_eq!(
            resolve_coding_agent_provider("claude").map(|agent| agent.id),
            Some("claude".to_string())
        );
        assert_eq!(
            resolve_coding_agent_provider("claude-code").map(|agent| agent.id),
            Some("claude".to_string())
        );
        assert_eq!(
            resolve_coding_agent_provider("codex").map(|agent| agent.id),
            Some("codex".to_string())
        );
        assert_eq!(resolve_coding_agent_provider("other"), None);
    }

    #[test]
    fn status_active_only_for_live_states() {
        assert!(PersistentSessionStatus::Starting.is_active());
        assert!(PersistentSessionStatus::Running.is_active());
        assert!(!PersistentSessionStatus::Stopped.is_active());
        assert!(!PersistentSessionStatus::Exited.is_active());
    }

    fn temp_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "one-persistent-session-{}-{}-{}",
            name,
            std::process::id(),
            nanos
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn temp_conn() -> sqlez::connection::Connection {
        let path = temp_dir("db").join("test.db");
        sqlez::connection::Connection::open_file(path.to_str().unwrap())
    }

    fn test_provider(id: &str) -> CodingAgentProvider {
        CodingAgentProvider {
            id: id.to_string(),
            label: id.to_string(),
            command: "sh".to_string(),
            args: Vec::new(),
            install_command: None,
            install_instructions: None,
        }
    }

    #[test]
    fn classify_terminal_lines_recognizes_user_action_states() {
        let auth = classify_terminal_lines(
            PersistentSessionStatus::Running,
            "Claude",
            &["Please log in with your browser".to_string()],
        );
        assert_eq!(auth.1, "auth_required");

        let trust = classify_terminal_lines(
            PersistentSessionStatus::Running,
            "Claude",
            &["Do you trust the files in this folder?".to_string()],
        );
        assert_eq!(trust.1, "trust_required");

        let permission = classify_terminal_lines(
            PersistentSessionStatus::Running,
            "Claude",
            &["Allow this command? yes/no".to_string()],
        );
        assert_eq!(permission.1, "permission_required");
    }

    #[test]
    fn classify_terminal_lines_recognizes_claude_numbered_choice() {
        let choice = classify_terminal_lines(
            PersistentSessionStatus::Running,
            "Claude",
            &[
                "Do you want to create index.html?".to_string(),
                "1. Yes".to_string(),
                "2. Yes, allow all edits during this session (shift+tab)".to_string(),
                "3. No".to_string(),
            ],
        );
        assert_eq!(choice.1, "choice_required");
        assert!(choice.2.contains("index.html"));
        assert!(choice.3.contains("选1"));
    }

    #[test]
    fn classify_terminal_lines_recognizes_choice_with_cursor_on_same_line() {
        let choice = classify_terminal_lines(
            PersistentSessionStatus::Running,
            "Claude",
            &[
                "Do you want to create member-list.html? ❯ 1. Yes".to_string(),
                "2. Yes, allow all edits during this session (shift+tab)".to_string(),
                "3. No".to_string(),
            ],
        );
        assert_eq!(choice.1, "choice_required");
        assert!(choice.2.contains("member-list.html"));
    }

    #[test]
    fn classify_terminal_lines_recognizes_claude_plan_mode_menu() {
        let choice = classify_terminal_lines(
            PersistentSessionStatus::Running,
            "Claude",
            &[
                "Entered plan mode".to_string(),
                "Planning: /Users/example/.claude/plans/example.md".to_string(),
                "← □ 技术栈  □ 功能范围  ✔ Submit →".to_string(),
                "您想要什么类型的会员管理系统？请选择技术栈和形式:".to_string(),
                "❯ 1. Web 单页应用（HTML+JS）".to_string(),
                "  2. Vue3 + Vite 项目".to_string(),
                "  3. React + Node.js 全栈".to_string(),
                "Enter to select · Tab/Arrow keys to navigate · Esc to cancel".to_string(),
            ],
        );
        assert_eq!(choice.1, "choice_required");
        assert!(choice.3.contains("选1"));
    }

    #[test]
    fn classify_terminal_lines_prefers_latest_choice_over_stale_auth() {
        let choice = classify_terminal_lines(
            PersistentSessionStatus::Running,
            "Claude",
            &[
                "Please log in with your browser".to_string(),
                "Authenticated successfully".to_string(),
                "Opened changes in Trae".to_string(),
                "Do you want to overwrite login.html?".to_string(),
                "❯ 1. Yes".to_string(),
                "  2. Yes, allow all edits in Desktop/ during this session".to_string(),
                "  3. No".to_string(),
            ],
        );
        assert_eq!(choice.1, "choice_required");
        assert!(choice.2.contains("login.html"));
    }

    #[test]
    fn classify_terminal_lines_does_not_treat_auth_success_as_login_wait() {
        let state = classify_terminal_lines(
            PersistentSessionStatus::Running,
            "Claude",
            &[
                "Authenticated successfully".to_string(),
                "<think>Working on the requested page</think>".to_string(),
                "Read 1 file".to_string(),
                "Write(~/Desktop/login.html)".to_string(),
            ],
        );
        assert_ne!(state.1, "auth_required");
    }

    #[test]
    fn maps_user_replies_to_claude_choices() {
        assert_eq!(map_user_message_to_choice("同意").as_deref(), Some("1"));
        assert_eq!(map_user_message_to_choice("选1").as_deref(), Some("1"));
        assert_eq!(map_user_message_to_choice("全部允许").as_deref(), Some("2"));
        assert_eq!(map_user_message_to_choice("选2").as_deref(), Some("2"));
        assert_eq!(map_user_message_to_choice("拒绝").as_deref(), Some("3"));
        assert_eq!(map_user_message_to_choice("选3").as_deref(), Some("3"));
        assert_eq!(map_user_message_to_choice("你不能替我选么"), None);
    }

    #[test]
    fn classify_terminal_lines_does_not_treat_plain_directory_as_trust() {
        let state = classify_terminal_lines(
            PersistentSessionStatus::Running,
            "Claude",
            &["Working directory: /tmp/project".to_string()],
        );
        assert_ne!(state.1, "trust_required");
    }

    #[test]
    fn shell_session_binds_task_and_releases_write_lease() {
        let conn = temp_conn();
        let cwd = temp_dir("workspace");
        let mut manager = PersistentCliSessionManager::new();

        let session_id = manager
            .start_session_with_program(
                &conn,
                10,
                20,
                test_provider("claude"),
                "sh",
                cwd.clone(),
                true,
                Some("echo ready"),
            )
            .unwrap();

        assert_eq!(
            manager.attached_session_id_for_task(10).as_deref(),
            Some(session_id.as_str())
        );
        assert_eq!(
            manager
                .active_write_session_for_workspace(20)
                .map(|session| session.session_id.as_str()),
            Some(session_id.as_str())
        );

        let blocked = manager.start_session_with_program(
            &conn,
            11,
            20,
            test_provider("codex"),
            "sh",
            cwd.clone(),
            true,
            None,
        );
        assert!(blocked.is_err());

        manager.stop_session(&conn, &session_id).unwrap();
        assert!(manager.active_write_session_for_workspace(20).is_none());

        let next_id = manager
            .start_session_with_program(
                &conn,
                11,
                20,
                test_provider("codex"),
                "sh",
                cwd,
                true,
                None,
            )
            .unwrap();
        manager.stop_session(&conn, &next_id).unwrap();
    }
}
