#![allow(dead_code)]

use std::env;
use std::sync::{Mutex, OnceLock};

use tokio::sync::oneshot;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionMode {
    Bypass,
    Default,
    Strict,
}

impl PermissionMode {
    fn from_env_value(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "bypass" | "bypasspermissions" => Some(Self::Bypass),
            "default" | "ask" | "prompt" => Some(Self::Default),
            "strict" | "deny" => Some(Self::Strict),
            _ => None,
        }
    }

    pub fn claude_code_flag(&self) -> &'static str {
        match self {
            Self::Bypass => "bypassPermissions",
            Self::Default => "default",
            Self::Strict => "default",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolKind {
    Shell,
    File,
    Process,
    ClaudeCode,
}

impl ToolKind {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Shell => "Shell 命令",
            Self::File => "文件操作",
            Self::Process => "系统进程",
            Self::ClaudeCode => "Claude Code",
        }
    }

    /// Whether this tool kind is considered destructive enough that even
    /// `Default` mode should ask the user before allowing it.
    pub fn requires_prompt_in_default(&self) -> bool {
        matches!(self, Self::Shell | Self::File | Self::Process)
    }
}

#[derive(Debug, Clone)]
pub enum PermissionDecision {
    Allow,
    Deny(String),
    /// Synchronous evaluator decided that the user must be asked.
    /// Callers should fall back to the async [`request_async`] path.
    Ask,
}

pub struct PermissionPolicy {
    mode: PermissionMode,
}

impl PermissionPolicy {
    pub fn new(mode: PermissionMode) -> Self {
        Self { mode }
    }

    pub fn mode(&self) -> PermissionMode {
        self.mode
    }

    /// Cheap synchronous check. Returns `Ask` for cases that require user
    /// confirmation; the caller is expected to invoke [`request_async`] in
    /// that case.
    pub fn evaluate(&self, kind: ToolKind, _detail: &str) -> PermissionDecision {
        match (self.mode, kind) {
            (PermissionMode::Bypass, _) => PermissionDecision::Allow,
            (PermissionMode::Strict, ToolKind::Shell) => {
                PermissionDecision::Deny("shell execution disabled in strict mode".into())
            }
            (PermissionMode::Strict, ToolKind::ClaudeCode) => PermissionDecision::Allow,
            (PermissionMode::Strict, _) => PermissionDecision::Ask,
            (PermissionMode::Default, kind) if kind.requires_prompt_in_default() => {
                PermissionDecision::Ask
            }
            (PermissionMode::Default, _) => PermissionDecision::Allow,
        }
    }

    /// Evaluate and, if the result is `Ask`, suspend until the user answers
    /// via the global approval queue.
    pub async fn request_async(&self, kind: ToolKind, detail: impl Into<String>) -> PermissionDecision {
        let detail = detail.into();
        match self.evaluate(kind, &detail) {
            PermissionDecision::Allow => PermissionDecision::Allow,
            PermissionDecision::Deny(reason) => PermissionDecision::Deny(reason),
            PermissionDecision::Ask => match enqueue_request(kind, detail).await {
                Some(true) => PermissionDecision::Allow,
                Some(false) => PermissionDecision::Deny("user declined".into()),
                None => PermissionDecision::Deny("approval channel unavailable".into()),
            },
        }
    }
}

static POLICY: OnceLock<PermissionPolicy> = OnceLock::new();

pub fn global() -> &'static PermissionPolicy {
    POLICY.get_or_init(|| {
        let mode = env::var("ONE_PERMISSION_MODE")
            .ok()
            .and_then(|v| PermissionMode::from_env_value(&v))
            .unwrap_or(PermissionMode::Default);
        PermissionPolicy::new(mode)
    })
}

// ---------------------------------------------------------------------------
// Approval queue
//
// Background tools call `request_async`, which posts a [`ApprovalRequest`] to
// the queue and waits on a oneshot. The UI thread drains the queue every tick,
// surfaces a confirmation dialog, and resolves the oneshot when the user
// clicks Allow / Deny.
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct ApprovalRequest {
    pub id: u64,
    pub kind: ToolKind,
    pub detail: String,
    responder: oneshot::Sender<bool>,
}

impl ApprovalRequest {
    pub fn approve(self) {
        let _ = self.responder.send(true);
    }

    pub fn deny(self) {
        let _ = self.responder.send(false);
    }
}

#[derive(Default)]
struct ApprovalQueue {
    pending: Vec<ApprovalRequest>,
    next_id: u64,
}

static QUEUE: OnceLock<Mutex<ApprovalQueue>> = OnceLock::new();

fn queue() -> &'static Mutex<ApprovalQueue> {
    QUEUE.get_or_init(|| Mutex::new(ApprovalQueue::default()))
}

async fn enqueue_request(kind: ToolKind, detail: String) -> Option<bool> {
    let (tx, rx) = oneshot::channel();
    {
        let mut q = queue().lock().ok()?;
        q.next_id = q.next_id.wrapping_add(1);
        let id = q.next_id;
        q.pending.push(ApprovalRequest {
            id,
            kind,
            detail,
            responder: tx,
        });
    }
    rx.await.ok()
}

/// UI side: pop the next pending approval request, if any.
pub fn drain_next() -> Option<ApprovalRequest> {
    let mut q = queue().lock().ok()?;
    if q.pending.is_empty() {
        None
    } else {
        Some(q.pending.remove(0))
    }
}

/// UI side: number of approvals waiting in the queue (excluding the one
/// currently displayed to the user).
pub fn pending_count() -> usize {
    queue().lock().map(|q| q.pending.len()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_modes() {
        assert_eq!(
            PermissionMode::from_env_value("bypass"),
            Some(PermissionMode::Bypass)
        );
        assert_eq!(
            PermissionMode::from_env_value("Default"),
            Some(PermissionMode::Default)
        );
        assert_eq!(
            PermissionMode::from_env_value("STRICT"),
            Some(PermissionMode::Strict)
        );
        assert!(PermissionMode::from_env_value("garbage").is_none());
    }

    #[test]
    fn strict_blocks_shell() {
        let policy = PermissionPolicy::new(PermissionMode::Strict);
        match policy.evaluate(ToolKind::Shell, "ls") {
            PermissionDecision::Deny(_) => {}
            _ => panic!("strict mode should deny shell"),
        }
    }

    #[test]
    fn bypass_allows_shell() {
        let policy = PermissionPolicy::new(PermissionMode::Bypass);
        match policy.evaluate(ToolKind::Shell, "ls") {
            PermissionDecision::Allow => {}
            _ => panic!("bypass mode should allow shell"),
        }
    }

    #[test]
    fn default_asks_for_destructive() {
        let policy = PermissionPolicy::new(PermissionMode::Default);
        match policy.evaluate(ToolKind::Shell, "rm -rf /") {
            PermissionDecision::Ask => {}
            other => panic!("expected Ask, got {:?}", other),
        }
        match policy.evaluate(ToolKind::ClaudeCode, "edit") {
            PermissionDecision::Allow => {}
            other => panic!("expected Allow for ClaudeCode in Default, got {:?}", other),
        }
    }
}
