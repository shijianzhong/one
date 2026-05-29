use std::sync::Arc;
use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;

use super::{Agent, AgentContext, AgentResponse, Tool, BaseAgent};

pub struct Coordinator {
    base: BaseAgent,
    sub_agents: std::collections::HashMap<String, Arc<dyn Agent>>,
}

impl Coordinator {
    pub fn new(
        model: String,
        api_base: String,
        api_key: String,
        sub_agents: std::collections::HashMap<String, Arc<dyn Agent>>,
    ) -> Self {
        let system_prompt = format!(
            r#"你是一个高级任务协调者。你的职责是将复杂的任务拆解为可执行的步骤，并分派给最合适的专业 Agent。

当前可用的 Agent 列表：
{}

工作准则：
1. **主动记忆**：当用户提到个人姓名、偏好、习惯或重要背景事实时，务必指派子任务给 `memory` Agent 进行保存。
2. **个性化**：在回答用户之前，如果不确定背景，可以指派 `memory` Agent 查询历史事实。
3. **日常对话**：普通的问候、闲聊、简单问答，请指派给 `general` Agent。
4. **专业分发**：编码任务找 `coding`，系统状态找 `system`。
5. **任务拆解**：如果任务复杂，可以先指派给 A，根据 A 的结果再指派给 B。

始终以专业、高效且具备“长期记忆”意识的方式思考。"#,
            sub_agents.values()
                .map(|a| format!("- {}: {}", a.id(), a.name()))
                .collect::<Vec<_>>()
                .join("\n")
        );

        Self {
            base: BaseAgent {
                id: "coordinator".to_string(),
                name: "Coordinator".to_string(),
                system_prompt,
                tools: vec![Arc::new(DelegateTool::new())],
                model,
                api_base,
                api_key,
            },
            sub_agents,
        }
    }
}

#[async_trait]
impl Agent for Coordinator {
    fn id(&self) -> &str { &self.base.id }
    fn name(&self) -> &str { &self.base.name }
    fn system_prompt(&self) -> &str { &self.base.system_prompt }
    fn tools(&self) -> Vec<Arc<dyn Tool>> { self.base.tools.clone() }

    async fn step(&self, context: &mut AgentContext) -> Result<AgentResponse> {
        self.base.call_llm(context).await
    }
}

struct DelegateTool;

impl DelegateTool {
    fn new() -> Self { Self }
}

#[async_trait]
impl Tool for DelegateTool {
    fn name(&self) -> &str { "delegate" }
    fn description(&self) -> &str { "将子任务指派给特定的专业 Agent" }
    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "agent_id": {
                    "type": "string",
                    "description": "目标 Agent 的 ID"
                },
                "task": {
                    "type": "string",
                    "description": "指派的具体任务描述"
                }
            },
            "required": ["agent_id", "task"]
        })
    }

    async fn call(&self, _arguments: serde_json::Value) -> Result<serde_json::Value> {
        // This is a special tool that is handled by the orchestrator loop
        Ok(json!({ "status": "delegated" }))
    }
}
