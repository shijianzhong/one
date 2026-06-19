#![allow(dead_code)]

use std::cell::Cell;
use std::env;
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum DangerLevel {
    #[default]
    Normal,
    Dangerous,
    Extreme,
}

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
    ReadOnly,
    Shell,
    File,
    Process,
    ClaudeCode,
}

impl ToolKind {
    pub fn label(&self) -> &'static str {
        match self {
            Self::ReadOnly => "只读工具",
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

pub fn classify_mcp_tool_kind(tool_name: &str) -> ToolKind {
    let normalized = tool_name.trim().to_ascii_lowercase().replace('-', "_");
    let mut parts = normalized.split('_').filter(|part| !part.is_empty());
    let first = parts.next().unwrap_or(normalized.as_str());
    let read_prefixes = [
        "read", "get", "list", "search", "query", "fetch", "find", "lookup", "describe", "inspect",
        "stat", "show",
    ];
    let write_prefixes = [
        "create", "update", "delete", "remove", "write", "edit", "patch", "run", "exec", "execute",
        "call", "post", "send", "upload", "download", "move", "copy", "rename", "set",
    ];

    if write_prefixes.contains(&first) {
        ToolKind::Process
    } else if read_prefixes.contains(&first) {
        ToolKind::ReadOnly
    } else {
        ToolKind::Process
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
    pub fn evaluate(
        &self,
        kind: ToolKind,
        _detail: &str,
        source: Option<&str>,
    ) -> PermissionDecision {
        let mut mode = self.mode;
        if let Some("remote") = source {
            mode = PermissionMode::Strict;
        }

        match (mode, kind) {
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
    ///
    /// If `source` is `None` but the current thread is inside a
    /// [`RemoteScopeGuard`], the call is automatically treated as
    /// `source = Some("remote")` and the policy is tightened to Strict.
    pub async fn request_async(
        &self,
        kind: ToolKind,
        detail: impl Into<String>,
        source: Option<&str>,
    ) -> PermissionDecision {
        let detail = detail.into();
        // Promote source to "remote" when inside a RemoteScopeGuard.
        let effective_source: Option<&str> = if source.is_none() && RemoteScopeGuard::is_active() {
            Some("remote")
        } else {
            source
        };
        match self.evaluate(kind, &detail, effective_source) {
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

static APPROVAL_NOTIFY: OnceLock<std::sync::Arc<tokio::sync::Notify>> = OnceLock::new();

pub fn approval_notify() -> std::sync::Arc<tokio::sync::Notify> {
    APPROVAL_NOTIFY
        .get_or_init(|| std::sync::Arc::new(tokio::sync::Notify::new()))
        .clone()
}

/// 投递本机审批请求但不等待结果。返回 oneshot Receiver，调用方在需要时 await。
/// 用于 Extreme 双确认：先投递弹窗，等暗号验证通过后再等待弹窗结果。
pub fn enqueue_detached(
    kind: ToolKind,
    detail: String,
) -> Option<tokio::sync::oneshot::Receiver<bool>> {
    let (tx, rx) = tokio::sync::oneshot::channel();
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
    approval_notify().notify_one();
    Some(rx)
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
    approval_notify().notify_one();
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

// ---------------------------------------------------------------------------
// Remote scope guard
//
// When a Trigger (e.g. Telegram) handles a `/run` command it calls
// `RemoteScopeGuard::enter()`.  For the lifetime of the guard, any call to
// `permission().request_async` that passes `source = None` will behave as if
// `source = Some("remote")` were passed — i.e. the policy is automatically
// tightened to Strict.
//
// Implementation: a thread-local `bool` flag.  Trigger handlers run on a
// dedicated tokio worker thread so the flag is isolated from the UI thread.
// ---------------------------------------------------------------------------

thread_local! {
    static REMOTE_SCOPE: Cell<bool> = Cell::new(false);
}

/// RAII guard that marks the current thread as "remote source" for the
/// duration of its lifetime.  Drop restores the previous value so that
/// nested calls are safe.
pub struct RemoteScopeGuard {
    previous: bool,
}

impl RemoteScopeGuard {
    /// Enter remote scope.  Returns the guard; drop it to leave the scope.
    pub fn enter() -> Self {
        let previous = REMOTE_SCOPE.with(|c| c.replace(true));
        Self { previous }
    }

    /// Query whether the current thread is inside a remote scope.
    pub fn is_active() -> bool {
        REMOTE_SCOPE.with(|c| c.get())
    }
}

impl Drop for RemoteScopeGuard {
    fn drop(&mut self) {
        REMOTE_SCOPE.with(|c| c.set(self.previous));
    }
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
        match policy.evaluate(ToolKind::Shell, "ls", None) {
            PermissionDecision::Deny(_) => {}
            _ => panic!("strict mode should deny shell"),
        }
    }

    #[test]
    fn bypass_allows_shell() {
        let policy = PermissionPolicy::new(PermissionMode::Bypass);
        match policy.evaluate(ToolKind::Shell, "ls", None) {
            PermissionDecision::Allow => {}
            _ => panic!("bypass mode should allow shell"),
        }
    }

    #[test]
    fn default_asks_for_destructive() {
        let policy = PermissionPolicy::new(PermissionMode::Default);
        match policy.evaluate(ToolKind::Shell, "rm -rf /", None) {
            PermissionDecision::Ask => {}
            other => panic!("expected Ask, got {:?}", other),
        }
        match policy.evaluate(ToolKind::ClaudeCode, "edit", None) {
            PermissionDecision::Allow => {}
            other => panic!("expected Allow for ClaudeCode in Default, got {:?}", other),
        }
    }

    #[test]
    fn default_allows_read_only_tools() {
        let policy = PermissionPolicy::new(PermissionMode::Default);
        match policy.evaluate(ToolKind::ReadOnly, "MCP filesystem/list", None) {
            PermissionDecision::Allow => {}
            other => panic!(
                "expected Allow for read-only tool in Default, got {:?}",
                other
            ),
        }
    }

    #[test]
    fn classifies_mcp_tool_names_conservatively() {
        assert_eq!(classify_mcp_tool_kind("list_files"), ToolKind::ReadOnly);
        assert_eq!(classify_mcp_tool_kind("search-code"), ToolKind::ReadOnly);
        assert_eq!(classify_mcp_tool_kind("get"), ToolKind::ReadOnly);
        assert_eq!(classify_mcp_tool_kind("write_file"), ToolKind::Process);
        assert_eq!(classify_mcp_tool_kind("execute_command"), ToolKind::Process);
        assert_eq!(classify_mcp_tool_kind("unknown_tool"), ToolKind::Process);
    }

    #[test]
    fn remote_source_enforces_strict_mode() {
        // Even when the global policy is Default...
        let policy = PermissionPolicy::new(PermissionMode::Default);
        // ...a remote source should deny shell, as if in Strict mode.
        match policy.evaluate(ToolKind::Shell, "ls", Some("remote")) {
            PermissionDecision::Deny(_) => {}
            other => panic!("remote source should deny shell, got {:?}", other),
        }
    }

    #[test]
    fn remote_scope_guard_enter_and_drop() {
        // Before entering: not active.
        assert!(!RemoteScopeGuard::is_active());
        {
            let _guard = RemoteScopeGuard::enter();
            assert!(RemoteScopeGuard::is_active());
        }
        // After drop: restored to false.
        assert!(!RemoteScopeGuard::is_active());
    }

    #[test]
    fn remote_scope_guard_nested() {
        // Nested guards: inner drop should not clear the outer guard's flag.
        let _outer = RemoteScopeGuard::enter();
        assert!(RemoteScopeGuard::is_active());
        {
            let _inner = RemoteScopeGuard::enter();
            assert!(RemoteScopeGuard::is_active());
        }
        // Outer still active after inner dropped.
        assert!(RemoteScopeGuard::is_active());
    }

    #[test]
    fn evaluate_remote_scope_auto_strict() {
        // Default policy + no explicit source, but thread is in remote scope
        // → should behave as Strict for Shell (Deny).
        let policy = PermissionPolicy::new(PermissionMode::Default);
        let _guard = RemoteScopeGuard::enter();
        // We call evaluate directly (sync), source=None.
        // evaluate does NOT check the thread-local — that's intentional;
        // only request_async does.  So we verify the async path instead
        // via is_active().
        assert!(RemoteScopeGuard::is_active());
        // Verify that evaluate with source="remote" gives Deny for shell.
        match policy.evaluate(ToolKind::Shell, "ls", Some("remote")) {
            PermissionDecision::Deny(_) => {}
            other => panic!("expected Deny, got {:?}", other),
        }
    }
}
