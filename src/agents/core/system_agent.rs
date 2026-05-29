use std::sync::Arc;
use anyhow::Result;
use async_trait::async_trait;

use super::{Agent, AgentContext, AgentResponse, Tool, BaseAgent};
use super::tools::{ProcessListTool, FileListTool};

pub struct SystemAgent {
    base: BaseAgent,
}

impl SystemAgent {
    pub fn new(model: String, api_base: String, api_key: String) -> Self {
        let system_prompt = r#"你是一个系统管理专家 Agent。你能够分析操作系统状态，包括进程、文件系统、磁盘空间等。
你可以调用工具来获取实时数据，并根据数据为用户提供建议或执行操作。
始终保持谨慎，特别是在涉及文件删除或进程终止的操作时。"#.to_string();

        Self {
            base: BaseAgent {
                id: "system".to_string(),
                name: "System Agent".to_string(),
                system_prompt,
                tools: vec![
                    Arc::new(ProcessListTool),
                    Arc::new(FileListTool),
                ],
                model,
                api_base,
                api_key,
            },
        }
    }
}

#[async_trait]
impl Agent for SystemAgent {
    fn id(&self) -> &str { &self.base.id }
    fn name(&self) -> &str { &self.base.name }
    fn system_prompt(&self) -> &str { &self.base.system_prompt }
    fn tools(&self) -> Vec<Arc<dyn Tool>> { self.base.tools.clone() }

    async fn step(&self, context: &mut AgentContext) -> Result<AgentResponse> {
        self.base.step_with_tools(context).await
    }
}
