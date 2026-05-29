use std::sync::Arc;
use anyhow::Result;
use async_trait::async_trait;

use super::{Agent, AgentContext, AgentResponse, Tool, BaseAgent};
use super::tools::ShellTool;

pub struct CodingAgent {
    base: BaseAgent,
}

impl CodingAgent {
    pub fn new(model: String, api_base: String, api_key: String) -> Self {
        let system_prompt = r#"你是一个软件开发专家 Agent。你精通各种编程语言和架构设计。
你可以调用工具来执行命令、编译代码、运行测试或管理文件。
你可以利用已安装的 `claude` CLI 工具来辅助复杂的编码任务。
始终追求高质量、高性能且易于维护的代码。"#.to_string();

        Self {
            base: BaseAgent {
                id: "coding".to_string(),
                name: "Coding Agent".to_string(),
                system_prompt,
                tools: vec![
                    Arc::new(ShellTool),
                ],
                model,
                api_base,
                api_key,
            },
        }
    }
}

#[async_trait]
impl Agent for CodingAgent {
    fn id(&self) -> &str { &self.base.id }
    fn name(&self) -> &str { &self.base.name }
    fn system_prompt(&self) -> &str { &self.base.system_prompt }
    fn tools(&self) -> Vec<Arc<dyn Tool>> { self.base.tools.clone() }

    async fn step(&self, context: &mut AgentContext) -> Result<AgentResponse> {
        self.base.step_with_tools(context).await
    }
}
