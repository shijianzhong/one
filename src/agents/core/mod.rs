use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

pub mod agent;
pub mod builtin_tools;
pub mod factory;
pub mod main_agent;
pub mod orchestrator;
pub mod tool_registry;

pub use agent::{AgentResponse, AgentRunContext, AgentTrait, ToolCall};
pub use main_agent::MainAgent;
pub use orchestrator::{Orchestrator, OrchestratorEvent};
pub use tool_registry::tool_registry;

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters_schema(&self) -> Value;
    async fn call(&self, arguments: Value) -> Result<Value>;
}
