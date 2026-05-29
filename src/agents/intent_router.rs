use crate::agents::types::RoutingDecision;
use crate::memory::types::ChatMessage;

#[derive(Debug, Clone)]
pub enum IntentLevel {
    /// Simple query, can be handled directly by main agent
    General,
    /// System operation request (process, disk, memory, etc.)
    SystemTools,
    /// Coding task, needs Claude Code
    Coding,
    /// Complex task, needs LLM intent analysis
    Complex,
}

pub struct IntentRouter {
    /// Keywords that indicate system tools requests
    system_keywords: Vec<String>,
    /// Keywords that indicate coding tasks
    coding_keywords: Vec<String>,
    /// Keywords that indicate general conversation
    general_keywords: Vec<String>,
    /// Keywords that trigger LLM intent analysis
    complex_keywords: Vec<String>,
}

impl IntentRouter {
    pub fn new() -> Self {
        Self {
            system_keywords: vec![
                "进程", "cpu", "内存", "磁盘", "硬盘", "空间", "占用",
                "process", "memory", "disk",
                "打开", "打开应用", "启动", "关闭程序", "杀进程", "终止",
                "文件夹", "目录", "文件", "删除", "复制", "移动",
            ].into_iter().map(String::from).collect(),
            coding_keywords: vec![
                "写代码", "代码", "编程", "函数", "调试", "bug",
                "code", "coding", "debug", "function", "class",
                "实现", "开发", "程序", "帮我写", "写个", "创建一个",
            ].into_iter().map(String::from).collect(),
            general_keywords: vec![
                "你好", "hi", "hello", "天气", "今天", "怎么样",
                "什么是", "怎么", "为什么", "谁", "哪里",
                "介绍", "解释", "告诉我",
            ].into_iter().map(String::from).collect(),
            complex_keywords: vec![
                "帮我做", "完成", "整个", "复杂的", "多个",
                "multi", "complex", "agent", "插件", "skill",
            ].into_iter().map(String::from).collect(),
        }
    }

    fn matches_any(text: &str, keywords: &[String]) -> bool {
        let text_lower = text.to_lowercase();
        keywords.iter().any(|k| text_lower.contains(&k.to_lowercase()))
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

        // Check if it contains complex indicators (LLM needed)
        if Self::matches_any(msg_trimmed, &self.complex_keywords) {
            return (IntentLevel::Complex, None);
        }

        // Only obvious conversational requests are routed directly to the
        // lightweight general agent. Unknown requests fall through to the
        // orchestrator so it can decide whether delegation is needed.
        if Self::matches_any(msg_trimmed, &self.general_keywords) {
            return (
                IntentLevel::General,
                Some(RoutingDecision::GeneralAI {
                    messages: vec![ChatMessage::new("user", msg_trimmed)],
                }),
            );
        }

        (IntentLevel::Complex, None)
    }

    /// Returns true if LLM intent analysis is needed
    pub fn needs_llm_intent(&self, message: &str) -> bool {
        let (level, _) = self.route(message);
        matches!(level, IntentLevel::Complex)
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

        // System tools are deterministic routes; they do not need the
        // orchestrator/intent LLM just to decide the destination.
        assert!(!router.needs_llm_intent("查看进程"));
        assert!(!router.needs_llm_intent("我电脑的内存使用情况"));
        assert!(!router.needs_llm_intent("打开 Safari"));

        // Should match system tools route
        let (level, _) = router.route("查看进程");
        assert!(matches!(level, IntentLevel::SystemTools));

        // Coding
        let (level, _) = router.route("帮我写个函数");
        assert!(matches!(level, IntentLevel::Coding));
        assert!(!router.needs_llm_intent("帮我写个函数"));
    }

    #[test]
    fn test_general_conversation() {
        let router = IntentRouter::new();

        // General conversation - no LLM needed
        assert!(!router.needs_llm_intent("你好"));
        assert!(!router.needs_llm_intent("今天天气怎么样"));
    }

    #[test]
    fn test_complex_needs_llm() {
        let router = IntentRouter::new();

        // Complex multi-agent tasks need LLM
        assert!(router.needs_llm_intent("帮我用 agent 完成任务"));
        assert!(router.needs_llm_intent("调用 skill 完成这个工作"));
        assert!(router.needs_llm_intent("帮我整理这个项目接下来要做什么"));
        assert!(router.quick_route("帮我整理这个项目接下来要做什么").is_none());
    }
}
