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
        Self::with_workspace(model, api_base, api_key, "Default".to_string())
    }

    pub fn with_workspace(
        model: String,
        api_base: String,
        api_key: String,
        workspace: String,
    ) -> Self {
        let soul_path = dirs::config_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join(".one")
            .join("soul.md");
        let soul_content = fs::read_to_string(&soul_path).unwrap_or_else(|_| {
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
            Arc::new(RememberTool { workspace: workspace.clone() }),
            Arc::new(RecallTool { workspace: workspace.clone() }),
            Arc::new(ProposeSoulUpdateTool),
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

/// 标识工具：触发 SkillRegistry / SystemAgent 执行系统任务。由 Orchestrator 拦截。
struct RunSystemTaskTool;
#[async_trait]
impl Tool for RunSystemTaskTool {
    fn name(&self) -> &str { "run_system_task" }
    fn description(&self) -> &str {
        "执行系统级任务。优先用 `skill_id` 直接调用已注册 Skill（当前可用：system.cleaner, desktop.organizer, app.uninstaller, doc.summarizer, media.dedup）；\
         若调用 Skill，请先 `apply=false` 看 preview，再 `apply=true` 触发执行（执行时会经用户授权弹窗）。\
         若没有合适的 Skill，可只填 `task` 字段，由系统专家 Agent 走通用路径（列进程/磁盘/文件等）。"
    }
    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "skill_id": {
                    "type": "string",
                    "description": "目标 Skill 的 id，例如 \"system.cleaner\"。命中则走 Skill Registry 直调，否则回落到通用 SystemAgent。"
                },
                "apply": {
                    "type": "boolean",
                    "description": "仅当填了 skill_id 时有效：false 仅做 preview（只读、可重复），true 才执行 execute（需用户授权）。默认 false。"
                },
                "args": {
                    "type": "object",
                    "description": "传给 Skill preview/execute 的参数（结构由 Skill 自身决定）。"
                },
                "task": {
                    "type": "string",
                    "description": "未命中 Skill 时的自然语言任务描述，会交给 SystemAgent 处理。"
                }
            }
        })
    }
    async fn call(&self, _args: serde_json::Value) -> Result<serde_json::Value> {
        Ok(json!({ "status": "intercepted_by_orchestrator" }))
    }
}

/// 记忆存储工具
struct RememberTool {
    workspace: String,
}
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
        crate::memory::profile::save_fact(&self.workspace, fact)?;
        Ok(json!({ "status": "success", "message": "Fact remembered" }))
    }
}

/// 记忆检索工具
struct RecallTool {
    workspace: String,
}
#[async_trait]
impl Tool for RecallTool {
    fn name(&self) -> &str { "recall" }
    fn description(&self) -> &str { "查询已保存的关于用户的长期事实和偏好。" }
    fn parameters_schema(&self) -> serde_json::Value {
        json!({ "type": "object", "properties": {} })
    }
    async fn call(&self, _args: serde_json::Value) -> Result<serde_json::Value> {
        let facts = crate::memory::profile::get_all_facts(&self.workspace);
        Ok(json!(facts))
    }
}

/// 灵魂草案工具：把"修改 soul.md"的请求写入审核队列，等待用户在 GUI 中确认。
/// 不再允许 LLM 直接覆盖 soul.md（避免自我改写人格）。
struct ProposeSoulUpdateTool;
#[async_trait]
impl Tool for ProposeSoulUpdateTool {
    fn name(&self) -> &str { "propose_soul_update" }
    fn description(&self) -> &str { "提交一份对你自身人格设定（soul.md）的修订草案。仅写入审核队列，必须经用户在界面上确认后才会真正生效。" }
    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "rationale": { "type": "string", "description": "为什么需要更新人格设定的简要说明" },
                "new_soul_content": { "type": "string", "description": "完整的、更新后的 soul.md 内容（草案）" }
            },
            "required": ["rationale", "new_soul_content"]
        })
    }
    async fn call(&self, args: serde_json::Value) -> Result<serde_json::Value> {
        let rationale = args["rationale"].as_str().unwrap_or_default().to_string();
        let content = args["new_soul_content"].as_str().unwrap_or_default().to_string();
        if content.is_empty() {
            return Err(anyhow::anyhow!("New soul content cannot be empty"));
        }
        match crate::agents::soul::submit_proposal(rationale, content) {
            Some(id) => Ok(json!({
                "status": "queued",
                "proposal_id": id,
                "message": "草案已提交，等待用户在界面上审核确认后才会写入 soul.md"
            })),
            None => Err(anyhow::anyhow!("soul proposal queue unavailable")),
        }
    }
}
