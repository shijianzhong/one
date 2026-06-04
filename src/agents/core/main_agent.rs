use std::sync::Arc;
use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;
use std::fs;

use super::{Agent, AgentContext, AgentResponse, Tool, BaseAgent};

pub struct MainAgent {
    base: BaseAgent,
}

impl MainAgent {
    pub fn new(model: String, api_base: String, api_key: String) -> Self {
        let soul_content = fs::read_to_string("soul.md").unwrap_or_else(|_| {
            "你是一个通用的 AI 助手。".to_string()
        });

        let system_prompt = format!(
            "{}\n\n当前日期：{}\n操作环境：{}\n\n请严格按照上述灵魂设定和准则行动。",
            soul_content,
            chrono::Local::now().format("%Y-%m-%d"),
            std::env::consts::OS
        );

        let tools: Vec<Arc<dyn Tool>> = vec![
            Arc::new(RunClaudeCodeTool),
            Arc::new(RunSystemTaskTool),
            Arc::new(RememberTool),
            Arc::new(RecallTool),
            Arc::new(UpdateSoulTool),
        ];

        Self {
            base: BaseAgent {
                id: "main".to_string(),
                name: "Main Agent".to_string(),
                system_prompt,
                tools,
                model,
                api_base,
                api_key,
            },
        }
    }
}

#[async_trait]
impl Agent for MainAgent {
    fn id(&self) -> &str { &self.base.id }
    fn name(&self) -> &str { &self.base.name }
    fn system_prompt(&self) -> &str { &self.base.system_prompt }
    fn tools(&self) -> Vec<Arc<dyn Tool>> { self.base.tools.clone() }

    async fn step(&self, context: &mut AgentContext) -> Result<AgentResponse> {
        self.base.call_llm(context).await
    }

    async fn step_stream(
        &self,
        context: &mut AgentContext,
        on_delta: Box<dyn FnMut(String) + Send>,
    ) -> Result<AgentResponse> {
        self.base.call_llm_stream(context, on_delta).await
    }
}

// --- Tools for MainAgent ---

/// 标识工具：触发 Claude Code 执行。由 Orchestrator 拦截并特殊处理。
struct RunClaudeCodeTool;
#[async_trait]
impl Tool for RunClaudeCodeTool {
    fn name(&self) -> &str { "run_claude_code" }
    fn description(&self) -> &str { "当任务需要编写、修改、调试代码，或者进行复杂的文件系统操作时使用。" }
    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "instruction": { "type": "string", "description": "给 Claude Code 的详细编码任务指令" }
            },
            "required": ["instruction"]
        })
    }
    async fn call(&self, _args: serde_json::Value) -> Result<serde_json::Value> {
        Ok(json!({ "status": "intercepted_by_orchestrator" }))
    }
}

/// 标识工具：触发 SystemAgent 执行系统任务。由 Orchestrator 拦截。
struct RunSystemTaskTool;
#[async_trait]
impl Tool for RunSystemTaskTool {
    fn name(&self) -> &str { "run_system_task" }
    fn description(&self) -> &str { "涉及查询系统进程、磁盘空间、列出文件等基础系统管理任务时使用。" }
    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "task": { "type": "string", "description": "要执行的系统管理任务描述" }
            },
            "required": ["task"]
        })
    }
    async fn call(&self, _args: serde_json::Value) -> Result<serde_json::Value> {
        Ok(json!({ "status": "intercepted_by_orchestrator" }))
    }
}

/// 记忆存储工具
struct RememberTool;
#[async_trait]
impl Tool for RememberTool {
    fn name(&self) -> &str { "remember" }
    fn description(&self) -> &str { "记录关于用户的长期偏好、姓名或重要背景事实。" }
    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "fact": { "type": "string", "description": "需要记住的事实内容" }
            },
            "required": ["fact"]
        })
    }
    async fn call(&self, args: serde_json::Value) -> Result<serde_json::Value> {
        let fact = args["fact"].as_str().unwrap_or_default();
        crate::memory::profile::save_fact(".", fact)?;
        Ok(json!({ "status": "success", "message": "Fact remembered" }))
    }
}

/// 记忆检索工具
struct RecallTool;
#[async_trait]
impl Tool for RecallTool {
    fn name(&self) -> &str { "recall" }
    fn description(&self) -> &str { "查询已保存的关于用户的长期事实和偏好。" }
    fn parameters_schema(&self) -> serde_json::Value {
        json!({ "type": "object", "properties": {} })
    }
    async fn call(&self, _args: serde_json::Value) -> Result<serde_json::Value> {
        let facts = crate::memory::profile::get_all_facts(".");
        Ok(json!(facts))
    }
}

/// 灵魂进化工具：修改 soul.md
struct UpdateSoulTool;
#[async_trait]
impl Tool for UpdateSoulTool {
    fn name(&self) -> &str { "update_soul" }
    fn description(&self) -> &str { "修改你的人格设定、价值观或行为准则（即更新 soul.md）。当你意识到需要调整自己的交流风格或行为逻辑以更好地服务用户时使用。" }
    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "new_soul_content": { "type": "string", "description": "完整的、更新后的 soul.md 内容" }
            },
            "required": ["new_soul_content"]
        })
    }
    async fn call(&self, args: serde_json::Value) -> Result<serde_json::Value> {
        let content = args["new_soul_content"].as_str().unwrap_or_default();
        if content.is_empty() {
            return Err(anyhow::anyhow!("New soul content cannot be empty"));
        }
        fs::write("soul.md", content)?;
        Ok(json!({ "status": "success", "message": "Soul evolved. Please restart or refresh to apply changes." }))
    }
}
