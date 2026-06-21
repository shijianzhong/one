#![allow(dead_code)]

//! Soul proposal queue.
//!
//! `MainAgent` used to expose an `update_soul` tool that let the LLM rewrite
//! its own persona file (`soul.md`) directly. That is unsafe: the model can
//! silently mutate its own behavioural contract without the user noticing.
//!
//! The new flow is "propose, do not commit":
//!   1. The LLM calls `propose_soul_update`, which only enqueues a draft into
//!      this module's global queue.
//!   2. A UI pump drains the queue and surfaces a review card.
//!   3. The user explicitly approves the diff before `soul.md` is rewritten.
//!
//! This file owns the data structures and the global queue. The actual file
//! write is performed by [`commit_proposal`] when the UI confirms.

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

const DEFAULT_SOUL_CONTENT: &str = "你是一个通用的 AI 助手。";

/// A pending change to `soul.md` waiting on user review.
#[derive(Debug, Clone)]
pub struct SoulProposal {
    pub id: u64,
    /// Reason the LLM gave for proposing the rewrite.
    pub rationale: String,
    /// Full new contents the LLM wants to write to `soul.md`.
    pub new_content: String,
    /// Snapshot of the current `soul.md` at the moment the proposal was filed.
    pub previous_content: String,
}

impl SoulProposal {
    fn new(id: u64, rationale: String, new_content: String, previous_content: String) -> Self {
        Self {
            id,
            rationale,
            new_content,
            previous_content,
        }
    }
}

#[derive(Default)]
struct ProposalQueue {
    pending: Vec<SoulProposal>,
    next_id: u64,
}

static QUEUE: OnceLock<Mutex<ProposalQueue>> = OnceLock::new();

fn queue() -> &'static Mutex<ProposalQueue> {
    QUEUE.get_or_init(|| Mutex::new(ProposalQueue::default()))
}

static SOUL_NOTIFY: OnceLock<std::sync::Arc<tokio::sync::Notify>> = OnceLock::new();

pub fn soul_notify() -> std::sync::Arc<tokio::sync::Notify> {
    SOUL_NOTIFY
        .get_or_init(|| std::sync::Arc::new(tokio::sync::Notify::new()))
        .clone()
}

pub(crate) fn soul_path() -> PathBuf {
    if let Ok(path) = std::env::var("ONE_SOUL_PATH") {
        let path = PathBuf::from(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        return path;
    }

    let config_dir = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".one");
    std::fs::create_dir_all(&config_dir).ok();
    config_dir.join("soul.md")
}

fn bundled_soul_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("soul.md")
}

pub(crate) fn ensure_soul_file() -> std::io::Result<PathBuf> {
    let path = soul_path();
    if path.exists() {
        return Ok(path);
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let initial_content = std::fs::read_to_string(bundled_soul_path())
        .unwrap_or_else(|_| DEFAULT_SOUL_CONTENT.to_string());
    std::fs::write(&path, initial_content)?;
    Ok(path)
}

pub(crate) fn read_soul_content() -> String {
    ensure_soul_file()
        .and_then(std::fs::read_to_string)
        .unwrap_or_else(|_| DEFAULT_SOUL_CONTENT.to_string())
}

/// Tool side: enqueue a proposal. Returns the assigned id, or `None` if the
/// global mutex is poisoned (in which case the caller should surface an
/// error string to the LLM).
pub fn submit_proposal(rationale: String, new_content: String) -> Option<u64> {
    let previous_content = read_soul_content();
    let mut q = queue().lock().ok()?;
    q.next_id = q.next_id.wrapping_add(1);
    let id = q.next_id;
    q.pending.push(SoulProposal::new(
        id,
        rationale,
        new_content,
        previous_content,
    ));
    soul_notify().notify_one();
    Some(id)
}

/// UI side: pop the next pending proposal, if any.
pub fn drain_next() -> Option<SoulProposal> {
    let mut q = queue().lock().ok()?;
    if q.pending.is_empty() {
        None
    } else {
        Some(q.pending.remove(0))
    }
}

/// UI side: count remaining proposals (excluding the one currently shown).
pub fn pending_count() -> usize {
    queue().lock().map(|q| q.pending.len()).unwrap_or(0)
}

/// UI side: persist an approved proposal to `soul.md`.
pub fn commit_proposal(proposal: &SoulProposal) -> std::io::Result<()> {
    let path = soul_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, &proposal.new_content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn isolated_soul_path(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "one-soul-test-{}-{}-{}.md",
            name,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        path
    }

    #[test]
    fn submit_and_drain_round_trip() {
        let _guard = env_lock().lock().unwrap();
        let path = isolated_soul_path("submit");
        std::env::set_var("ONE_SOUL_PATH", &path);

        let id = submit_proposal("test".into(), "hello".into()).unwrap();
        let popped = drain_next().expect("expected proposal");
        assert_eq!(popped.id, id);
        assert_eq!(popped.new_content, "hello");
        assert_eq!(popped.rationale, "test");
        assert!(popped.previous_content.contains("MainAgent Persona"));

        std::env::remove_var("ONE_SOUL_PATH");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn drain_empty_returns_none() {
        let _guard = env_lock().lock().unwrap();
        // Drain anything left over from earlier tests.
        while drain_next().is_some() {}
        assert!(drain_next().is_none());
        assert_eq!(pending_count(), 0);
    }

    #[test]
    fn missing_soul_file_is_initialized_from_template() {
        let _guard = env_lock().lock().unwrap();
        let path = isolated_soul_path("init");
        std::env::set_var("ONE_SOUL_PATH", &path);

        assert!(!path.exists());
        let content = read_soul_content();
        assert!(path.exists());
        assert!(content.contains("MainAgent Persona"));
        assert!(std::fs::read_to_string(&path)
            .unwrap()
            .contains("MainAgent Persona"));

        std::env::remove_var("ONE_SOUL_PATH");
        let _ = std::fs::remove_file(path);
    }
}
