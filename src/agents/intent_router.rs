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
            system_keywords: vec![
                "进程列表".into(),
                "系统信息".into(),
                "系统状态".into(),
                "磁盘使用".into(),
                "磁盘空间".into(),
                "磁盘占用".into(),
                "CPU使用".into(),
                "内存使用".into(),
                "内存占用".into(),
                "删除文件".into(),
                // 较长的英文关键词，避免短词误匹配
                "process list".into(),
                "system info".into(),
                "disk usage".into(),
                "disk space".into(),
                "memory usage".into(),
                "cpu usage".into(),
                "list processes".into(),
                "kill process".into(),
            ],
            coding_keywords: vec![
                "写代码".into(),
                "编码".into(),
                "实现".into(),
                "重构".into(),
                "fix".into(),
                "bug".into(),
                "feature".into(),
                "refactor".into(),
                "代码审查".into(),
                "code review".into(),
                "PR".into(),
                "写一个".into(),
                "创建一个".into(),
                "新建".into(),
                "实现一个".into(),
                "开发".into(),
                "编程".into(),
                "做一个".into(),
                "做个".into(),
                "做应用".into(),
                "搭建".into(),
                "code".into(),
                "coding".into(),
                "implement".into(),
                "add".into(),
                "create".into(),
                "修改".into(),
            ],
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
            return (IntentLevel::Coding, None);
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
    fn test_route_current_keyword_behavior() {
        let router = IntentRouter::new();

        let (level, decision) = router.route("我电脑的内存使用情况");
        assert!(matches!(level, IntentLevel::SystemTools));
        assert!(matches!(
            decision,
            Some(RoutingDecision::SystemTools { .. })
        ));

        let (level, decision) = router.route("帮我做一个登录页面");
        assert!(matches!(level, IntentLevel::Coding));
        assert!(decision.is_none());

        let (level, decision) = router.route("你好，今天聊点产品想法");
        assert!(matches!(level, IntentLevel::General));
        assert!(decision.is_none());
    }
}
