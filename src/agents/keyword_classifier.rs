//! KeywordClassifier - Fast keyword-based intent routing.
//!
//! Acts as a second-level router in the message processing pipeline:
//! 1. IntentRouter::quick_route() - fast hardcoded rules
//! 2. KeywordClassifier::classify() - keyword matching against predefined categories
//! 3. If matched → spawn LLM intent agent for deep classification
//! 4. If no match → spawn general AI (main agent)
//!
//! Categories and keywords are hardcoded (not user-configurable) to prevent
//! accidental misconfiguration. The keyword pool is loaded into memory once
//! at application startup.

/// A category with a set of trigger keywords.
///
/// When a user message contains any keyword from the category (case-insensitive
/// substring match), the classifier returns a match. A match means the message
/// should go through LLM-based intent analysis rather than directly to the
/// general AI agent.
#[derive(Debug, Clone)]
pub struct IntentCategory {
    pub name: &'static str,
    pub description: &'static str,
    pub keywords: &'static [&'static str],
}

/// Static keyword classification pool.
///
/// Categories are ordered by priority (descending). The first match wins.
/// All matching is case-insensitive substring matching.
const INTENT_CATEGORIES: &[IntentCategory] = &[
    IntentCategory {
        name: "system_tools",
        description: "系统信息查询、进程管理、系统命令执行",
        keywords: &[
            "进程",
            "cpu",
            "内存",
            "磁盘",
            "杀死",
            "kill",
            "ps aux",
            "top",
            "df -h",
            "系统信息",
            "系统状态",
        ],
    },
    IntentCategory {
        name: "coding",
        description: "代码开发、编程、UI/前端组件创建",
        keywords: &[
            "写代码",
            "写一个",
            "创建一个",
            "实现一个",
            "做UI",
            "做前端",
            "做组件",
            "做页面",
            "做登录",
            "function",
            "impl",
            "struct",
            "trait",
            "fn ",
            "写个",
        ],
    },
    IntentCategory {
        name: "agent_task",
        description: "需要多步骤执行或agent协作的复杂任务",
        keywords: &[
            "帮我做",
            "完成",
            "整个项目",
            "整体",
            "全流程",
            "agent",
            "多步",
            "一步步",
            "分步骤",
        ],
    },
    IntentCategory {
        name: "terminal",
        description: "终端命令执行、shell操作",
        keywords: &[
            "终端",
            "terminal",
            "命令行",
            "运行命令",
            "执行命令",
            "bash",
            "zsh",
            "shell",
            "npm",
            "cargo",
            "git ",
            "ssh",
            "docker",
            "brew",
        ],
    },
    IntentCategory {
        name: "file_operation",
        description: "文件读写、编辑、搜索",
        keywords: &[
            "读取文件",
            "写入文件",
            "编辑文件",
            "删除文件",
            "查找文件",
            "搜索文件",
            "打开文件",
            "文件操作",
            "cat ",
            "vim ",
            "nano ",
        ],
    },
    IntentCategory {
        name: "knowledge_query",
        description: "知识问答、概念解释、技术咨询",
        keywords: &[
            "什么是",
            "为什么",
            "如何",
            "怎样",
            "解释",
            "介绍",
            "说明",
            "原理",
            "区别",
            "对比",
            "优缺点",
        ],
    },
];

/// A classifier that matches user messages against hardcoded intent categories.
///
/// Created once at startup, then used for every user message. The match
/// operation is O(n * k) where n = number of categories and k = average
/// keywords per category. No heap allocation is performed during matching
/// since all data is static.
#[derive(Debug, Clone)]
pub struct KeywordClassifier;

impl KeywordClassifier {
    /// Match a user message against all intent categories.
    ///
    /// Returns the first matching category, or `None` if no category matches.
    /// Categories with higher priority (listed first) are checked first.
    /// Matching is case-insensitive: the message is lowered once, then each
    /// keyword is checked via `contains()`.
    pub fn classify(message: &str) -> Option<&'static IntentCategory> {
        let lower = message.to_lowercase();
        for category in INTENT_CATEGORIES {
            for keyword in category.keywords {
                if lower.contains(keyword) {
                    return Some(category);
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_coding_match() {
        let result = KeywordClassifier::classify("帮我写一个登录页面");
        assert!(result.is_some());
        assert_eq!(result.unwrap().name, "coding");
    }

    #[test]
    fn test_system_tools_match() {
        let result = KeywordClassifier::classify("查看CPU使用率");
        assert!(result.is_some());
        assert_eq!(result.unwrap().name, "system_tools");
    }

    #[test]
    fn test_no_match_falls_to_general() {
        let result = KeywordClassifier::classify("今天天气怎么样");
        assert!(result.is_none());
    }

    #[test]
    fn test_case_insensitive() {
        let result = KeywordClassifier::classify("KILL process");
        assert!(result.is_some());
        assert_eq!(result.unwrap().name, "system_tools");
    }

    #[test]
    fn test_english_coding() {
        let result = KeywordClassifier::classify("implement a login page");
        assert!(result.is_some());
        assert_eq!(result.unwrap().name, "coding");
    }

    #[test]
    fn test_agent_task() {
        let result = KeywordClassifier::classify("帮我完成整个项目的开发");
        assert!(result.is_some());
        assert_eq!(result.unwrap().name, "agent_task");
    }

    #[test]
    fn test_no_agent_task_for_simple_help() {
        let no_match = KeywordClassifier::classify("帮我查一下东西");
        assert!(no_match.is_none());
    }
}