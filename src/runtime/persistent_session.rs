use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::{anyhow, Result};

use crate::run_log::{RunEvent, RunKind, RunRecorder, RunStatus};
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
    pub(crate) run_id: Option<usize>,
    pub(crate) last_error: Option<String>,
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
            run_id,
            last_error: None,
        };
        self.sessions.insert(session_id.clone(), session);
        self.task_attached_session
            .insert(task_id, session_id.clone());
        if write_mode {
            self.workspace_write_owner
                .insert(workspace_id, session_id.clone());
        }
        self.send_input(db_conn, &session_id, command_line)?;
        if let Some(input) = initial_input {
            if wait_for_ready {
                let ready = wait_for_runtime_ready(&terminal, std::time::Duration::from_secs(20))?;
                if let Some(run_id) = run_id {
                    RunRecorder::attach(db_conn, run_id).record(&RunEvent::MessageDelta {
                        text: format!("{} runtime ready: {}", agent_label, ready),
                    });
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
            terminal.write_text(text);
        }
        session.status = PersistentSessionStatus::Running;
        session.last_active_at = chrono::Local::now();
        session.output_seq = session.output_seq.saturating_add(1);
        if let Some(run_id) = session.run_id {
            RunRecorder::attach(db_conn, run_id).record(&RunEvent::MessageDelta {
                text: format!("USER INPUT:\n{}", text),
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

fn wait_for_runtime_ready(
    terminal: &Arc<Mutex<TerminalEmulator>>,
    timeout: std::time::Duration,
) -> Result<String> {
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
            return Ok(line.trim().to_string());
        }
        if std::time::Instant::now() >= deadline {
            let recent = lines
                .into_iter()
                .filter(|line| !line.trim().is_empty())
                .rev()
                .take(8)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join("\n");
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
