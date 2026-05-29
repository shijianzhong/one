use std::sync::Arc;
use anyhow::Result;
use async_trait::async_trait;

use super::{Agent, AgentContext, AgentResponse, Tool, BaseAgent};
use super::tools::MemoryTool;

pub struct MemoryAgent {
    base: BaseAgent,
}

impl MemoryAgent {
    pub fn new(model: String, api_base: String, api_key: String, workspace: String) -> Self {
        let system_prompt = r#"你是一个记忆管理专家。你的职责是：
1. 从对话中提取用户的长期偏好、姓名、习惯、以及重要的历史事实。
2. 当发现值得记住的新信息时，使用 `manage_memory` 工具将其永久保存。
3. 当用户询问或需要个性化建议时，查询长期记忆。
你不需要直接回答用户的问题，而是辅助其他 Agent 提供更有针对性的服务。"#.to_string();

        Self {
            base: BaseAgent {
                id: "memory".to_string(),
                name: "Memory Agent".to_string(),
                system_prompt,
                tools: vec![
                    Arc::new(MemoryTool { workspace }),
                ],
                model,
                api_base,
                api_key,
            },
        }
    }
}

#[async_trait]
impl Agent for MemoryAgent {
    fn id(&self) -> &str { &self.base.id }
    fn name(&self) -> &str { &self.base.name }
    fn system_prompt(&self) -> &str { &self.base.system_prompt }
    fn tools(&self) -> Vec<Arc<dyn Tool>> { self.base.tools.clone() }

    async fn step(&self, context: &mut AgentContext) -> Result<AgentResponse> {
        self.base.call_llm(context).await
    }
}
