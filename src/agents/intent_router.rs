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
                "进程", "cpu", "内存", "磁盘", "硬盘", "空间", "占用",
                "process", "memory", "disk",
                "打开", "打开应用", "启动", "关闭程序", "杀进程", "终止",
                "文件夹", "目录", "文件", "删除", "复制", "移动",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
            coding_keywords: vec![
                "写代码", "代码", "编程", "函数", "调试", "bug",
                "code", "coding", "debug", "function", "class",
                "实现", "开发", "程序", "帮我写", "写个", "创建一个",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
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
                Some(RoutingDecision::ClaudeCode {
                    instruction: msg_trimmed.to_string(),
                    session_id: None,
                }),
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
    fn test_system_keywords() {
        let router = IntentRouter::new();
        assert!(router.needs_precise_route("查看进程"));
        assert!(router.needs_precise_route("我电脑的内存使用情况"));
        assert!(router.needs_precise_route("打开 Safari"));

        let (level, _) = router.route("查看进程");
        assert!(matches!(level, IntentLevel::SystemTools));

        let (level, _) = router.route("帮我写个函数");
        assert!(matches!(level, IntentLevel::Coding));
        assert!(router.needs_precise_route("帮我写个函数"));
    }

    #[test]
    fn test_general_conversation() {
        let router = IntentRouter::new();
        // 所有简单对话都不需要精确路由
        assert!(!router.needs_precise_route("你好"));
        assert!(!router.needs_precise_route("what's your name"));
        assert!(!router.needs_precise_route("今天天气怎么样"));
        assert!(!router.needs_precise_route("帮我整理项目"));

        let (level, decision) = router.route("what's your name");
        assert!(matches!(level, IntentLevel::General));
        assert!(decision.is_none());
    }
}
