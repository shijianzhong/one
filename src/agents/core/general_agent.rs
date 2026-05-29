use std::sync::Arc;
use anyhow::Result;
use async_trait::async_trait;

use super::{Agent, AgentContext, AgentResponse, Tool, BaseAgent};

pub struct GeneralAgent {
    base: BaseAgent,
}

impl GeneralAgent {
    pub fn new(model: String, api_base: String, api_key: String) -> Self {
        let system_prompt = r#"你是一个友好、专业的通用 AI 助手。
你的目标是：
1. 回答用户的日常问题、闲聊、提供建议。
2. 保持对话的连贯性和上下文感知。
3. 你的回答应该简洁、准确且有帮助。
如果你发现任务涉及复杂的编码或系统操作，协调者（Coordinator）会将任务指派给其他专业 Agent。"#.to_string();

        Self {
            base: BaseAgent {
                id: "general".to_string(),
                name: "General Assistant".to_string(),
                system_prompt,
                tools: vec![], // No specific tools needed for general chat
                model,
                api_base,
                api_key,
            },
        }
    }
}

#[async_trait]
impl Agent for GeneralAgent {
    fn id(&self) -> &str { &self.base.id }
    fn name(&self) -> &str { &self.base.name }
    fn system_prompt(&self) -> &str { &self.base.system_prompt }
    fn tools(&self) -> Vec<Arc<dyn Tool>> { self.base.tools.clone() }

    async fn step(&self, context: &mut AgentContext) -> Result<AgentResponse> {
        self.base.call_llm(context).await
    }
}
