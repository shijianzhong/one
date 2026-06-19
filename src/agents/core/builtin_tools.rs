use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

use super::Tool;

pub fn tools_for_workspace(workspace: &str) -> Vec<Arc<dyn Tool>> {
    vec![
        Arc::new(RunSystemTaskTool),
        Arc::new(RunInTerminalTool),
        Arc::new(StartCodingWorkflowTool),
        Arc::new(RememberTool {
            workspace: workspace.to_string(),
        }),
        Arc::new(RecallTool {
            workspace: workspace.to_string(),
        }),
        Arc::new(ProposeSoulUpdateTool),
    ]
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

struct RememberTool {
    workspace: String,
}

#[async_trait]
impl Tool for RememberTool {
    fn name(&self) -> &str {
        "remember"
    }

    fn description(&self) -> &str {
        "记录关于用户的长期偏好、姓名或重要背景事实。"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "fact": { "type": "string", "description": "需要记住的事实内容" },
                "scope": {
                    "type": "string",
                    "enum": ["global", "workspace", "both"],
                    "description": "global：跨 workspace；workspace：仅当前项目；both：同时存。默认 both。"
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
        "在右侧终端执行 shell 命令并实时显示输出。适用于运行代码、执行脚本、调用 CLI 工具等场景。"
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

struct StartCodingWorkflowTool;

#[async_trait]
impl Tool for StartCodingWorkflowTool {
    fn name(&self) -> &str {
        "start_coding_workflow"
    }

    fn description(&self) -> &str {
        "启动两阶段 Claude Code 编码工作流。适用于开发应用、实现功能、创建页面、修改代码、修复 bug、重构项目等编码任务。第一阶段只做方案梳理，用户确认后第二阶段才编码。"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "user_request": { "type": "string", "description": "用户的原始编码需求" },
                "main_agent_summary": { "type": "string", "description": "对需求的简要梳理，包括目标、范围和关键约束" },
                "known_constraints": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "已经明确的约束条件"
                },
                "suggested_direction": { "type": "string", "description": "可选的建议技术方向或实现倾向" },
                "clarification_focus": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "希望 Claude Code 在第一阶段重点澄清的问题"
                }
            },
            "required": ["user_request", "main_agent_summary"]
        })
    }

    async fn call(&self, _args: serde_json::Value) -> Result<serde_json::Value> {
        Ok(json!({ "status": "intercepted_by_orchestrator" }))
    }
}
