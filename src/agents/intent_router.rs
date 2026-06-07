use crate::agents::types::RoutingDecision;

#[derive(Debug, Clone)]
pub enum IntentLevel {
    /// System operation request (process, disk, memory, etc.)
    SystemTools,
    /// Coding task, needs Claude Code
    Coding,
    /// Default: general conversation / simple task
    General,
}

pub struct IntentRouter {
    /// Keywords that indicate system tools requests
    system_keywords: Vec<String>,
    /// Keywords that indicate coding tasks
    coding_keywords: Vec<String>,
}

impl IntentRouter {
    pub fn new() -> Self {
        Self {
            system_keywords: vec![],
            coding_keywords: vec![],
        }
    }

    fn matches_any(text: &str, keywords: &[String]) -> bool {
        let text_lower = text.to_lowercase();
        keywords
            .iter()
            .any(|k| text_lower.contains(&k.to_lowercase()))
    }

    pub fn route(&self, message: &str) -> (IntentLevel, Option<RoutingDecision>) {
        let msg_trimmed = message.trim();

        // Check if it matches system tools keywords
        if Self::matches_any(msg_trimmed, &self.system_keywords) {
            return (
                IntentLevel::SystemTools,
                Some(RoutingDecision::SystemTools {
                    task: msg_trimmed.to_string(),
                }),
            );
        }

        // Check if it matches coding keywords
        if Self::matches_any(msg_trimmed, &self.coding_keywords) {
            return (
                IntentLevel::Coding,
                None,
            );
        }

        // Default: general conversation, no precise routing needed
        (IntentLevel::General, None)
    }

    /// Returns true if precise routing matched (system or coding)
    pub fn needs_precise_route(&self, message: &str) -> bool {
        let (level, _) = self.route(message);
        !matches!(level, IntentLevel::General)
    }

    /// Quick sync routing - returns decision directly without LLM
    pub fn quick_route(&self, message: &str) -> Option<RoutingDecision> {
        let (_, decision) = self.route(message);
        decision
    }
}

impl Default for IntentRouter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_keywords_no_fast_route() {
        let router = IntentRouter::new();
        // 关键词池为空，所有请求都应该走 General + None
        for msg in &["查看进程", "我电脑的内存使用情况", "打开 Safari",
                      "帮我写个函数", "你好", "what's your name", "今天天气怎么样"] {
            let (level, decision) = router.route(msg);
            assert!(matches!(level, IntentLevel::General), "expected General for '{}'", msg);
            assert!(decision.is_none(), "expected None decision for '{}'", msg);
            assert!(!router.needs_precise_route(msg), "expected no precise route for '{}'", msg);
        }
    }
}
