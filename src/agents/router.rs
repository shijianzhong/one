#![allow(dead_code)]

use std::collections::HashMap;

use super::{AgentRow, RoutingDecision};
use crate::memory::types::ChatMessage;

pub struct AgentRouter {
    business_agents: HashMap<usize, AgentRow>,
    claude_keywords: Vec<String>,
    system_keywords: Vec<String>,
}

impl Clone for AgentRouter {
    fn clone(&self) -> Self {
        Self {
            business_agents: self.business_agents.clone(),
            claude_keywords: self.claude_keywords.clone(),
            system_keywords: self.system_keywords.clone(),
        }
    }
}

impl AgentRouter {
    pub fn new() -> Self {
        let mut claude_keywords = Vec::new();
        claude_keywords.push("开发".to_string());
        claude_keywords.push("写代码".to_string());
        claude_keywords.push("写程序".to_string());
        claude_keywords.push("build".to_string());
        claude_keywords.push("code".to_string());
        claude_keywords.push("编程".to_string());
        claude_keywords.push("程序".to_string());
        claude_keywords.push("帮我".to_string());
        claude_keywords.push("写一个".to_string());
        claude_keywords.push("创建".to_string());

        let mut system_keywords = Vec::new();
        system_keywords.push("进程".to_string());
        system_keywords.push("内存".to_string());
        system_keywords.push("占用".to_string());
        system_keywords.push("硬盘".to_string());
        system_keywords.push("磁盘".to_string());
        system_keywords.push("空间".to_string());
        system_keywords.push("应用".to_string());
        system_keywords.push("程序".to_string());
        system_keywords.push("杀进程".to_string());
        system_keywords.push("关闭".to_string());
        system_keywords.push("删除".to_string());
        system_keywords.push("打开应用".to_string());
        system_keywords.push("哪个应用".to_string());
        system_keywords.push("什么应用".to_string());
        system_keywords.push("为什么".to_string());
        system_keywords.push("系统".to_string());

        Self {
            business_agents: HashMap::new(),
            claude_keywords,
            system_keywords,
        }
    }

    pub fn register_business_agent(&mut self, agent: AgentRow) {
        self.business_agents.insert(agent.id, agent);
    }

    pub fn classify_intent(&self, message: &str, _context: &[ChatMessage]) -> RoutingDecision {
        let message_lower = message.to_lowercase();

        // Check for Claude Code intent
        for keyword in &self.claude_keywords {
            if message_lower.contains(&keyword.to_lowercase()) {
                return RoutingDecision::ClaudeCode {
                    instruction: message.to_string(),
                    session_id: None,
                };
            }
        }

        // Check for business agent match
        for (agent_id, agent) in &self.business_agents {
            if let Some(desc) = &agent.description {
                if message_lower.contains(&desc.to_lowercase()) {
                    return RoutingDecision::BusinessAgent {
                        agent_id: *agent_id,
                        message: message.to_string(),
                    };
                }
            }
            if message_lower.contains(&agent.name.to_lowercase()) {
                return RoutingDecision::BusinessAgent {
                    agent_id: *agent_id,
                    message: message.to_string(),
                };
            }
        }

        // Check for agent creation intent
        let creation_keywords = ["创建", "生成", "新建", "做一个", "制作"];
        let agent_keywords = ["智能体", "agent", "助手", "业务"];

        let has_creation = creation_keywords.iter().any(|k| message_lower.contains(k));
        let has_agent_keyword = agent_keywords.iter().any(|k| message_lower.contains(k));

        if has_creation && has_agent_keyword {
            // This should trigger business agent generator
            return RoutingDecision::BusinessAgent {
                agent_id: 0, // Special ID for generator
                message: message.to_string(),
            };
        }

        // Check for System Agent intent
        for keyword in &self.system_keywords {
            if message_lower.contains(&keyword.to_lowercase()) {
                return RoutingDecision::SystemAgent {
                    task: message.to_string(),
                };
            }
        }

        // Default to general AI
        RoutingDecision::GeneralAI {
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: message.to_string(),
            }],
        }
    }

    pub fn route_message(
        &self,
        message: &str,
        context: &[ChatMessage],
        _agents: &HashMap<usize, AgentRow>,
    ) -> RoutingDecision {
        self.classify_intent(message, context)
    }

    pub fn check_claude_code_available() -> bool {
        which::which("claude").is_ok()
    }

    pub fn get_claude_session_id(task_id: usize) -> String {
        format!("one_task_{}", task_id)
    }
}

impl Default for AgentRouter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_claude_intent() {
        let router = AgentRouter::new();
        let decision = router.classify_intent("帮我开发一个Web应用", &[]);
        match decision {
            RoutingDecision::ClaudeCode { .. } => {}
            _ => panic!("Expected ClaudeCode decision"),
        }
    }

    #[test]
    fn test_classify_general_intent() {
        let router = AgentRouter::new();
        let decision = router.classify_intent("今天天气怎么样？", &[]);
        match decision {
            RoutingDecision::GeneralAI { .. } => {}
            _ => panic!("Expected GeneralAI decision"),
        }
    }

    #[test]
    fn test_classify_agent_creation() {
        let router = AgentRouter::new();
        let decision = router.classify_intent("我想创建一个客服智能体", &[]);
        match decision {
            RoutingDecision::BusinessAgent { agent_id: 0, .. } => {}
            _ => panic!("Expected BusinessAgent(generator) decision"),
        }
    }
}
