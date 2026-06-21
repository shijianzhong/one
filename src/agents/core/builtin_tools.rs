use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

use super::Tool;

pub fn tools_for_workspace(workspace: &str) -> Vec<Arc<dyn Tool>> {
    let mut tools: Vec<Arc<dyn Tool>> = vec![
        Arc::new(RunSystemTaskTool),
        Arc::new(RunInTerminalTool),
        Arc::new(DetectCodingClisTool),
        Arc::new(InstallCodingCliTool),
        Arc::new(CodingTerminalRuntimeAliasTool::new(
            "start_coding_terminal_runtime",
            "打开右侧终端并启动真实交互式 coding CLI runtime，例如输入 claude 进入 Claude Code。适用于编码任务的首轮启动。",
            CodingTerminalRuntimeAliasKind::Start,
        )),
        Arc::new(StartCodingSessionTool),
        Arc::new(CodingTerminalRuntimeAliasTool::new(
            "send_to_coding_terminal_runtime",
            "向当前 task 绑定的右侧终端 coding CLI runtime 写入输入。适合继续、同意、选择选项、补充需求。",
            CodingTerminalRuntimeAliasKind::Send,
        )),
        Arc::new(SendToCodingSessionTool),
        Arc::new(CodingTerminalRuntimeAliasTool::new(
            "read_coding_terminal_output",
            "读取当前 task 绑定的右侧终端 coding CLI runtime 最近可见输出。",
            CodingTerminalRuntimeAliasKind::Read,
        )),
        Arc::new(ReadCodingSessionOutputTool),
        Arc::new(InspectCodingSessionTool),
        Arc::new(CodingTerminalRuntimeAliasTool::new(
            "stop_coding_terminal_runtime",
            "停止当前 task 绑定的右侧终端 coding CLI runtime，并释放 workspace 写锁。",
            CodingTerminalRuntimeAliasKind::Stop,
        )),
        Arc::new(StopCodingSessionTool),
        Arc::new(CodingTerminalRuntimeAliasTool::new(
            "list_coding_terminal_runtimes",
            "列出当前应用内托管的右侧终端 coding CLI runtime。",
            CodingTerminalRuntimeAliasKind::List,
        )),
        Arc::new(ListCodingSessionsTool),
        Arc::new(GetWorkspaceWriteStatusTool),
        Arc::new(RememberTool {
            workspace: workspace.to_string(),
        }),
        Arc::new(RecallTool {
            workspace: workspace.to_string(),
        }),
        Arc::new(ProposeSoulUpdateTool),
    ];
    if crate::workflows::has_published_capabilities() {
        tools.push(Arc::new(RunCapabilityTool));
    }
    tools
}

struct RunSystemTaskTool;

#[async_trait]
impl Tool for RunSystemTaskTool {
    fn name(&self) -> &str {
        "run_system_task"
    }

    fn description(&self) -> &str {
        "执行系统级任务。通过 skill_id 调用已注册 Skill（system.tools 等）。\
         先设置 apply=false 预览，再 apply=true 执行。\n\n\
         system.tools 用于系统信息查询（进程/CPU/内存/磁盘/文件操作）。\
         支持的 tool：list_processes, top_memory_procs, get_process_detail, disk_usage, disk_free, list_dir, file_info。\n\
         system.cleaner 用于扫描和清理系统缓存、废纸篓等。"
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
                    "description": "false 仅做 preview，true 执行 execute。默认 false。"
                },
                "args": {
                    "type": "object",
                    "description": "传给 Skill preview/execute 的参数。"
                },
                "task": {
                    "type": "string",
                    "description": "未命中 Skill 时的自然语言任务描述。"
                }
            }
        })
    }

    async fn call(&self, _args: serde_json::Value) -> Result<serde_json::Value> {
        Ok(json!({ "status": "intercepted_by_orchestrator" }))
    }
}

struct DetectCodingClisTool;

#[async_trait]
impl Tool for DetectCodingClisTool {
    fn name(&self) -> &str {
        "detect_coding_clis"
    }

    fn description(&self) -> &str {
        "检测当前机器上配置的交互式编码 CLI 是否已安装，例如 Claude Code、Codex、Gemini。编码任务启动前应先调用。"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({ "type": "object", "properties": {} })
    }

    async fn call(&self, _args: serde_json::Value) -> Result<serde_json::Value> {
        Ok(json!({ "status": "intercepted_by_dispatcher" }))
    }
}

struct InstallCodingCliTool;

#[async_trait]
impl Tool for InstallCodingCliTool {
    fn name(&self) -> &str {
        "install_coding_cli"
    }

    fn description(&self) -> &str {
        "安装指定的交互式编码 CLI。必须先向用户说明将执行的安装命令并获得确认；安装失败时返回安装说明。"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "agent_kind": {
                    "type": "string",
                    "description": "要安装的 coding CLI provider id，例如 claude。"
                },
                "confirmed": {
                    "type": "boolean",
                    "description": "用户是否已经明确同意执行安装命令。必须为 true 才会安装。"
                }
            },
            "required": ["agent_kind", "confirmed"]
        })
    }

    async fn call(&self, _args: serde_json::Value) -> Result<serde_json::Value> {
        Ok(json!({ "status": "intercepted_by_dispatcher" }))
    }
}

struct RememberTool {
    workspace: String,
}

#[async_trait]
impl Tool for RememberTool {
    fn name(&self) -> &str {
        "remember"
    }

    fn description(&self) -> &str {
        "记录关于用户的长期偏好、姓名或重要背景事实。默认只记录到当前 workspace；只有用户明确要求跨项目长期记住时才使用 global。"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "fact": { "type": "string", "description": "需要记住的事实内容" },
                "scope": {
                    "type": "string",
                    "enum": ["global", "workspace", "both"],
                    "description": "global：跨 workspace 且必须是永久用户事实；workspace：仅当前项目；both：同时存。默认 workspace。"
                }
            },
            "required": ["fact"]
        })
    }

    async fn call(&self, args: serde_json::Value) -> Result<serde_json::Value> {
        let fact = args["fact"].as_str().unwrap_or_default();
        let scope = args["scope"].as_str().unwrap_or("workspace");

        match scope {
            "global" => crate::memory::profile::save_global_fact(fact, None)?,
            "workspace" => crate::memory::profile::save_fact(&self.workspace, fact, None)?,
            _ => {
                crate::memory::profile::save_global_fact(fact, None)?;
                crate::memory::profile::save_fact(&self.workspace, fact, None)?;
            }
        }

        Ok(
            json!({ "status": "success", "message": format!("Fact remembered in scope: {}", scope) }),
        )
    }
}

struct RecallTool {
    workspace: String,
}

#[async_trait]
impl Tool for RecallTool {
    fn name(&self) -> &str {
        "recall"
    }

    fn description(&self) -> &str {
        "查询已保存的关于用户的长期事实和偏好。"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({ "type": "object", "properties": {} })
    }

    async fn call(&self, _args: serde_json::Value) -> Result<serde_json::Value> {
        let mut set = std::collections::HashSet::new();
        for f in crate::memory::profile::get_global_facts() {
            set.insert(f);
        }
        for f in crate::memory::profile::get_all_facts(&self.workspace) {
            set.insert(f);
        }
        Ok(json!(set.into_iter().collect::<Vec<_>>()))
    }
}

struct ProposeSoulUpdateTool;

#[async_trait]
impl Tool for ProposeSoulUpdateTool {
    fn name(&self) -> &str {
        "propose_soul_update"
    }

    fn description(&self) -> &str {
        "提交一份对自身人格设定（soul.md）的修订草案。仅写入审核队列，必须经用户确认后才会真正生效。"
    }

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
        let content = args["new_soul_content"]
            .as_str()
            .unwrap_or_default()
            .to_string();
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

struct RunInTerminalTool;

#[async_trait]
impl Tool for RunInTerminalTool {
    fn name(&self) -> &str {
        "run_in_terminal"
    }

    fn description(&self) -> &str {
        "在右侧终端的独立 Shell tab 执行普通 shell 命令并显示输出。适用于查看目录、git 状态、运行脚本等非交互式命令；不要用它向 Claude/Codex/Gemini coding runtime 发送内容。"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "command": { "type": "string", "description": "要执行的 shell 命令" },
                "work_dir": { "type": "string", "description": "工作目录，默认为当前项目目录" }
            },
            "required": ["command"]
        })
    }

    async fn call(&self, _args: serde_json::Value) -> Result<serde_json::Value> {
        Ok(json!({ "status": "intercepted_by_orchestrator" }))
    }
}

enum CodingTerminalRuntimeAliasKind {
    Start,
    Send,
    Read,
    Stop,
    List,
}

struct CodingTerminalRuntimeAliasTool {
    name: &'static str,
    description: &'static str,
    kind: CodingTerminalRuntimeAliasKind,
}

impl CodingTerminalRuntimeAliasTool {
    fn new(
        name: &'static str,
        description: &'static str,
        kind: CodingTerminalRuntimeAliasKind,
    ) -> Self {
        Self {
            name,
            description,
            kind,
        }
    }
}

#[async_trait]
impl Tool for CodingTerminalRuntimeAliasTool {
    fn name(&self) -> &str {
        self.name
    }

    fn description(&self) -> &str {
        self.description
    }

    fn parameters_schema(&self) -> serde_json::Value {
        match self.kind {
            CodingTerminalRuntimeAliasKind::Start => json!({
                "type": "object",
                "properties": {
                    "agent_kind": {
                        "type": "string",
                        "description": "要启动的 coding CLI provider id，来自配置 coding_agents。默认优先 claude。"
                    },
                    "prompt": { "type": "string", "description": "启动后写入终端 CLI runtime 的任务说明。必须是 MainAgent 理解、补全上下文并拆解后的可执行工程任务，不要原样透传用户消息。" },
                    "write_mode": { "type": "boolean", "description": "是否需要写 workspace。编码任务通常为 true，查看状态或只读 review 可为 false。默认 true。" }
                },
                "required": ["prompt"]
            }),
            CodingTerminalRuntimeAliasKind::Send => json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string", "description": "可选；不填则使用当前 task 绑定的 runtime" },
                    "text": { "type": "string", "description": "要发送给 coding CLI runtime 的内容。必须是 MainAgent 理解后的操作指令、补充约束、澄清选择或明确交互输入；不要原样透传用户消息。" }
                },
                "required": ["text"]
            }),
            CodingTerminalRuntimeAliasKind::Read => json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string", "description": "可选；不填则使用当前 task 绑定的 runtime" },
                    "limit": { "type": "integer", "description": "最近输出行数，默认 40" }
                }
            }),
            CodingTerminalRuntimeAliasKind::Stop => json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string", "description": "可选；不填则使用当前 task 绑定的 runtime" }
                }
            }),
            CodingTerminalRuntimeAliasKind::List => {
                json!({ "type": "object", "properties": {} })
            }
        }
    }

    async fn call(&self, _args: serde_json::Value) -> Result<serde_json::Value> {
        Ok(json!({ "status": "intercepted_by_orchestrator" }))
    }
}

struct StartCodingSessionTool;

#[async_trait]
impl Tool for StartCodingSessionTool {
    fn name(&self) -> &str {
        "start_coding_session"
    }

    fn description(&self) -> &str {
        "在右侧终端启动一个真实的交互式 coding CLI runtime，例如运行 claude 进入 Claude Code。适用于开发应用、实现功能、创建页面、修改代码、修复 bug、重构项目等编码任务。runtime 在当前 workspace root 中运行，并绑定当前 task。"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "agent_kind": {
                    "type": "string",
                    "description": "要启动的 coding agent provider id，来自配置 coding_agents。默认优先 claude。"
                },
                "prompt": { "type": "string", "description": "启动后写入终端 CLI runtime 的任务说明。必须是 MainAgent 理解、补全上下文并拆解后的可执行工程任务，不要原样透传用户消息。" },
                "write_mode": { "type": "boolean", "description": "是否需要写 workspace。编码任务通常为 true，查看状态或只读 review 可为 false。默认 true。" }
            },
            "required": ["prompt"]
        })
    }

    async fn call(&self, _args: serde_json::Value) -> Result<serde_json::Value> {
        Ok(json!({ "status": "intercepted_by_orchestrator" }))
    }
}

struct SendToCodingSessionTool;

#[async_trait]
impl Tool for SendToCodingSessionTool {
    fn name(&self) -> &str {
        "send_to_coding_session"
    }

    fn description(&self) -> &str {
        "向当前 task 绑定的右侧终端 coding CLI runtime 写入输入。适合用户说继续、同意、选择某个选项、补充要求时使用。MainAgent 必须先理解用户意图，再发送整理后的操作指令或明确选择，不要把用户原话直接透传。"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "session_id": { "type": "string", "description": "可选；不填则使用当前 task 绑定的 session" },
                "text": { "type": "string", "description": "要发送给 coding agent 的内容。必须是 MainAgent 理解后的操作指令、补充约束、澄清选择或明确交互输入；不要原样透传用户消息。" }
            },
            "required": ["text"]
        })
    }

    async fn call(&self, _args: serde_json::Value) -> Result<serde_json::Value> {
        Ok(json!({ "status": "intercepted_by_orchestrator" }))
    }
}

struct ReadCodingSessionOutputTool;

#[async_trait]
impl Tool for ReadCodingSessionOutputTool {
    fn name(&self) -> &str {
        "read_coding_session_output"
    }

    fn description(&self) -> &str {
        "读取当前 task 绑定的右侧终端 coding CLI runtime 原始最近输出。仅在用户明确要求原始日志、最近输出或完整输出时使用；普通进度/卡住/等待确认请用 inspect_coding_terminal_runtime。"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "session_id": { "type": "string", "description": "可选；不填则使用当前 task 绑定的 session" },
                "limit": { "type": "integer", "description": "最近输出行数，默认 40" }
            }
        })
    }

    async fn call(&self, _args: serde_json::Value) -> Result<serde_json::Value> {
        Ok(json!({ "status": "intercepted_by_orchestrator" }))
    }
}

struct StopCodingSessionTool;

struct InspectCodingSessionTool;

#[async_trait]
impl Tool for InspectCodingSessionTool {
    fn name(&self) -> &str {
        "inspect_coding_terminal_runtime"
    }

    fn description(&self) -> &str {
        "读取并分析当前 task 绑定的右侧终端 coding CLI runtime 状态。会返回 ready、auth_required、trust_required、permission_required、choice_required、busy、command_missing 等结构化状态，适合 MainAgent 判断 Claude Code 当前是否需要用户登录、确认、授权或选择；不要把 raw terminal log 复述给用户。"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "session_id": { "type": "string", "description": "可选；不填则使用当前 task 绑定的 runtime" },
                "limit": { "type": "integer", "description": "用于状态判断的最近输出行数，默认 80" }
            }
        })
    }

    async fn call(&self, _args: serde_json::Value) -> Result<serde_json::Value> {
        Ok(json!({ "status": "intercepted_by_orchestrator" }))
    }
}

#[async_trait]
impl Tool for StopCodingSessionTool {
    fn name(&self) -> &str {
        "stop_coding_session"
    }

    fn description(&self) -> &str {
        "停止当前 task 绑定的右侧终端 coding CLI runtime，并释放 workspace 写锁。"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "session_id": { "type": "string", "description": "可选；不填则使用当前 task 绑定的 session" }
            }
        })
    }

    async fn call(&self, _args: serde_json::Value) -> Result<serde_json::Value> {
        Ok(json!({ "status": "intercepted_by_orchestrator" }))
    }
}

struct ListCodingSessionsTool;

#[async_trait]
impl Tool for ListCodingSessionsTool {
    fn name(&self) -> &str {
        "list_coding_sessions"
    }

    fn description(&self) -> &str {
        "列出当前应用内托管的右侧终端 coding CLI runtime。"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({ "type": "object", "properties": {} })
    }

    async fn call(&self, _args: serde_json::Value) -> Result<serde_json::Value> {
        Ok(json!({ "status": "intercepted_by_orchestrator" }))
    }
}

struct GetWorkspaceWriteStatusTool;

#[async_trait]
impl Tool for GetWorkspaceWriteStatusTool {
    fn name(&self) -> &str {
        "get_workspace_write_status"
    }

    fn description(&self) -> &str {
        "查询当前 workspace 是否已有 write-active coding CLI 会话。"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({ "type": "object", "properties": {} })
    }

    async fn call(&self, _args: serde_json::Value) -> Result<serde_json::Value> {
        Ok(json!({ "status": "intercepted_by_orchestrator" }))
    }
}

struct RunCapabilityTool;

#[async_trait]
impl Tool for RunCapabilityTool {
    fn name(&self) -> &str {
        "run_capability"
    }

    fn description(&self) -> &str {
        "调用已发布能力。能力是由工作流发布后的可复用执行单元，适合处理明确匹配已发布能力的任务。"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        let capability_ids: Vec<String> = crate::workflows::capability_manifests()
            .into_iter()
            .map(|capability| capability.id)
            .collect();
        json!({
            "type": "object",
            "properties": {
                "capability_id": {
                    "type": "string",
                    "enum": capability_ids,
                    "description": "要调用的已发布能力 id。"
                },
                "input": {
                    "type": "object",
                    "description": "传给能力工作流的输入对象，结构由该能力的 input_schema 决定。"
                }
            },
            "required": ["capability_id", "input"]
        })
    }

    async fn call(&self, args: serde_json::Value) -> Result<serde_json::Value> {
        let capability_id = args
            .get("capability_id")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow::anyhow!("capability_id is required"))?;
        let input = args.get("input").cloned().unwrap_or_else(|| json!({}));
        crate::workflows::run_capability(capability_id, input).await
    }
}
