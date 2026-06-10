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
            "{}\n\n当前日期：{}\n操作环境：{}\n\n请严格按照上述灵魂设定和准则行动。\n\n你有以下工具：\n\
             - **run_in_terminal** — 在右侧终端执行命令并实时显示输出。适用于运行代码、执行脚本、调用 CLI 等。\n\
             - **run_system_task** — 调用已注册的 Skill（system.tools 等）。\n\
             - **remember** — 记住用户信息。\n\
             - **recall** — 查询已记住的信息。\n\
             - **propose_soul_update** — 建议更新人格设定。\n\n\
             Skill 通过 run_system_task 调用：\n\
             - 查看 skill 使用说明 → run_system_task(skill_id=\"xxx\", apply=true)\n\
             - 查看进程/CPU/内存 → skill_id=\"system.tools\" args={{\"tool\": \"list_processes\"}}\n\
             - 查看磁盘空间 → skill_id=\"system.tools\" args={{\"tool\": \"disk_free\"}}\n\
             - 查看目录内容/文件信息 → skill_id=\"system.tools\" args={{\"tool\": \"list_dir\", \"path\": \"...\"}}\n\
             - 分析磁盘占用 → skill_id=\"system.tools\" args={{\"tool\": \"disk_usage\", \"path\": \"...\"}}\n
             用户问系统相关问题时，务必通过 run_system_task 获取真实数据，不要猜测。\n\n\
             需要执行任何命令时，使用 run_in_terminal。命令在右侧终端中执行，用户可以实时看到输出。",
            soul_content,
            chrono::Local::now().format("%Y-%m-%d"),
            std::env::consts::OS,
        );

        let tools: Vec<Arc<dyn Tool>> = vec![
            Arc::new(RunSystemTaskTool),
            Arc::new(RunInTerminalTool),
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

/// 标识工具：触发 SkillRegistry 执行系统任务。由 Orchestrator 拦截并转发到注册的 Skill。
struct RunSystemTaskTool;
#[async_trait]
impl Tool for RunSystemTaskTool {
    fn name(&self) -> &str { "run_system_task" }
    fn description(&self) -> &str {
        "执行系统级任务。通过 skill_id 调用已注册 Skill（当前可用：system.cleaner, system.tools, desktop.organizer, app.uninstaller, doc.summarizer, media.dedup）。\
         先设置 apply=false 预览，再 apply=true 执行。\n\n\
         **system.tools** 用于系统信息查询（进程/CPU/内存/磁盘/文件操作）。\
         支持的 tool：list_processes, top_memory_procs, get_process_detail, disk_usage, disk_free, list_dir, file_info。\n\
         **system.cleaner** 用于扫描和清理系统缓存、废纸篓等。"
    }
    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "skill_id": {
                    "type": "string",
                    "description": "目标 Skill 的 id，例如 system.tools。"
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
                "fact": { "type": "string", "description": "需要记住的事实内容" },
                "scope": { 
                    "type": "string", 
                    "enum": ["global", "workspace", "both"],
                    "description": "存储范围。global：跨 workspace 的个人信息；workspace：仅限当前项目；both：同时存。默认 both。"
                }
            },
            "required": ["fact"]
        })
    }
    async fn call(&self, args: serde_json::Value) -> Result<serde_json::Value> {
        let fact = args["fact"].as_str().unwrap_or_default();
        let scope = args["scope"].as_str().unwrap_or("both");
        
        match scope {
            "global" => crate::memory::profile::save_global_fact(fact, None)?,
            "workspace" => crate::memory::profile::save_fact(&self.workspace, fact, None)?,
            _ => {
                crate::memory::profile::save_global_fact(fact, None)?;
                crate::memory::profile::save_fact(&self.workspace, fact, None)?;
            }
        }
        
        Ok(json!({ "status": "success", "message": format!("Fact remembered in scope: {}", scope) }))
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
        let mut set = std::collections::HashSet::new();
        
        // 合并全局和工作区事实并去重
        for f in crate::memory::profile::get_global_facts() {
            set.insert(f);
        }
        for f in crate::memory::profile::get_all_facts(&self.workspace) {
            set.insert(f);
        }
        
        let facts: Vec<String> = set.into_iter().collect();
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

// ── 终端命令执行工具 ────────────────────────────────────────────

/// 在右侧终端中执行命令。由 Orchestrator 拦截并发送 RunInTerminal 事件。
struct RunInTerminalTool;
#[async_trait]
impl Tool for RunInTerminalTool {
    fn name(&self) -> &str { "run_in_terminal" }
    fn description(&self) -> &str {
        "在右侧终端执行 shell 命令并实时显示输出。适用于运行代码、执行脚本、调用 CLI 工具（如 claude）等场景。"
    }
    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "要执行的 shell 命令"
                },
                "work_dir": {
                    "type": "string",
                    "description": "工作目录，默认为当前项目目录"
                }
            },
            "required": ["command"]
        })
    }
    async fn call(&self, _args: serde_json::Value) -> Result<serde_json::Value> {
        Ok(json!({ "status": "intercepted_by_orchestrator" }))
    }
}

// ── MainAgentBuilder ────────────────────────────────────────────────────────────

/// MainAgent 的构建器，用于 AgentRegistry 注册。
pub struct MainAgentBuilder;

#[async_trait]
impl super::AgentBuilder for MainAgentBuilder {
    fn agent_id(&self) -> &str {
        "main"
    }

    fn agent_name(&self) -> &str {
        "Main Agent"
    }

    fn build(&self, config: &crate::services::Config, workspace: &str) -> Box<dyn super::AgentTrait> {
        // 创建 MainAgent 实例
        let agent = MainAgent::with_workspace(
            config.model_name.clone(),
            config.model_base_url.clone(),
            config.model_api_key.clone(),
            workspace.to_string(),
        );
        Box::new(MainAgentWrapper { inner: agent })
    }
}

/// 包装 MainAgent 以适配 AgentTrait
struct MainAgentWrapper {
    inner: MainAgent,
}

#[async_trait]
impl super::AgentTrait for MainAgentWrapper {
    fn id(&self) -> &str {
        "main"
    }

    fn name(&self) -> &str {
        "Main Agent"
    }

    fn soul_prompt(&self) -> &str {
        &self.inner.base.system_prompt
    }
}
