#![allow(dead_code)]

use std::env;
use std::sync::OnceLock;

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

#[derive(Debug, Clone, Copy)]
pub enum ToolKind {
    Shell,
    File,
    Process,
    ClaudeCode,
}

#[derive(Debug, Clone)]
pub enum PermissionDecision {
    Allow,
    Deny(String),
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

    pub fn evaluate(&self, kind: ToolKind, _detail: &str) -> PermissionDecision {
        match (self.mode, kind) {
            (PermissionMode::Strict, ToolKind::Shell) => {
                PermissionDecision::Deny("shell execution disabled in strict mode".into())
            }
            _ => PermissionDecision::Allow,
        }
    }
}

static POLICY: OnceLock<PermissionPolicy> = OnceLock::new();

pub fn global() -> &'static PermissionPolicy {
    POLICY.get_or_init(|| {
        let mode = env::var("ONE_PERMISSION_MODE")
            .ok()
            .and_then(|v| PermissionMode::from_env_value(&v))
            .unwrap_or(PermissionMode::Bypass);
        PermissionPolicy::new(mode)
    })
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
}
