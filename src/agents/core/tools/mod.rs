use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use super::{Tool};

pub struct ProcessListTool;

#[async_trait]
impl Tool for ProcessListTool {
    fn name(&self) -> &str { "list_processes" }
    fn description(&self) -> &str { "列出当前系统中正在运行的进程及其资源占用情况" }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    }

    async fn call(&self, _arguments: Value) -> Result<Value> {
        match crate::agents::permission::global()
            .request_async(
                crate::agents::permission::ToolKind::Process,
                "list_processes",
                None,
            )
            .await
        {
            crate::agents::permission::PermissionDecision::Allow => {}
            crate::agents::permission::PermissionDecision::Deny(reason) => {
                return Err(anyhow::anyhow!(
                    "Process list denied by permission policy: {}",
                    reason
                ));
            }
            crate::agents::permission::PermissionDecision::Ask => {
                return Err(anyhow::anyhow!(
                    "Process list requires explicit user approval but no UI handler resolved it"
                ));
            }
        }

        let procs = system_tools::tools::process::list_processes()
            .map_err(|e| anyhow::anyhow!(e))?;
        Ok(json!(procs))
    }
}

pub struct FileListTool;

#[async_trait]
impl Tool for FileListTool {
    fn name(&self) -> &str { "list_files" }
    fn description(&self) -> &str { "列出指定目录下的文件和文件夹" }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "要查询的目录路径"
                }
            },
            "required": ["path"]
        })
    }

    async fn call(&self, arguments: Value) -> Result<Value> {
        let path = arguments["path"].as_str().ok_or_else(|| anyhow::anyhow!("Missing path"))?;
        match crate::agents::permission::global()
            .request_async(crate::agents::permission::ToolKind::File, path, None)
            .await
        {
            crate::agents::permission::PermissionDecision::Allow => {}
            crate::agents::permission::PermissionDecision::Deny(reason) => {
                return Err(anyhow::anyhow!(
                    "File listing denied by permission policy: {}",
                    reason
                ));
            }
            crate::agents::permission::PermissionDecision::Ask => {
                return Err(anyhow::anyhow!(
                    "File listing requires explicit user approval but no UI handler resolved it"
                ));
            }
        }
        let files = system_tools::tools::file::list_dir(path)
            .map_err(|e| anyhow::anyhow!(e))?;
        Ok(json!(files))
    }
}

pub struct ShellTool;

#[async_trait]
impl Tool for ShellTool {
    fn name(&self) -> &str { "execute_command" }
    fn description(&self) -> &str { "在终端中执行 shell 命令。可以用于编译、运行测试或启动应用。" }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "要执行的 shell 命令"
                },
                "cwd": {
                    "type": "string",
                    "description": "执行命令的工作目录（可选）"
                },
                "timeout_secs": {
                    "type": "number",
                    "description": "命令超时时间（秒），默认 30 秒（可选）"
                }
            },
            "required": ["command"]
        })
    }

    async fn call(&self, arguments: Value) -> Result<Value> {
        let command = arguments["command"].as_str().ok_or_else(|| anyhow::anyhow!("Missing command"))?;
        let cwd = arguments["cwd"].as_str();

        match crate::agents::permission::global()
            .request_async(crate::agents::permission::ToolKind::Shell, command, None)
            .await
        {
            crate::agents::permission::PermissionDecision::Allow => {}
            crate::agents::permission::PermissionDecision::Deny(reason) => {
                return Err(anyhow::anyhow!(
                    "Shell execution denied by permission policy: {}",
                    reason
                ));
            }
            crate::agents::permission::PermissionDecision::Ask => {
                return Err(anyhow::anyhow!(
                    "Shell execution requires explicit user approval but no UI handler resolved it"
                ));
            }
        }

        // Security: limit output to prevent memory issues
        const MAX_OUTPUT_BYTES: usize = 100_000;

        // Timeout: default 30s, max 120s
        let timeout_secs = arguments["timeout_secs"]
            .as_u64()
            .map(|t| t.min(120))
            .unwrap_or(30);

        use tokio::process::Command as TokioCommand;
        use tokio::time::timeout;

        let mut cmd = TokioCommand::new("sh");
        cmd.arg("-c").arg(command);

        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }

        let output = timeout(
            std::time::Duration::from_secs(timeout_secs),
            cmd.output(),
        )
        .await
        .map_err(|_| anyhow::anyhow!("Command timed out after {} seconds: '{}'", timeout_secs, command))?;

        let output = output.map_err(|e| anyhow::anyhow!("Command failed: {}", e))?;

        Ok(json!({
            "stdout": truncate_utf8(&output.stdout, MAX_OUTPUT_BYTES),
            "stderr": truncate_utf8(&output.stderr, MAX_OUTPUT_BYTES),
            "status": output.status.code().unwrap_or(-1),
            "truncated": output.stdout.len() > MAX_OUTPUT_BYTES
                || output.stderr.len() > MAX_OUTPUT_BYTES,
        }))
    }
}

/// Truncate bytes at the nearest UTF-8 boundary, returning a String.
fn truncate_utf8(data: &[u8], max_bytes: usize) -> String {
    if data.len() <= max_bytes {
        String::from_utf8_lossy(data).to_string()
    } else {
        // Use String::from_utf8_lossy which handles boundary issues gracefully
        let truncated = String::from_utf8_lossy(&data[..max_bytes]).to_string();
        truncated + "… [truncated]"
    }
}

pub struct MemoryTool {
    pub workspace: String,
}

#[async_trait]
impl Tool for MemoryTool {
    fn name(&self) -> &str { "manage_memory" }
    fn description(&self) -> &str { "记录或查询关于用户的永久事实、偏好或重要信息。用于实现长期记忆。" }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["save_fact", "get_all_facts"],
                    "description": "要执行的操作"
                },
                "fact": {
                    "type": "string",
                    "description": "要记录的事实内容（仅在 action 为 save_fact 时需要）"
                }
            },
            "required": ["action"]
        })
    }

    async fn call(&self, arguments: Value) -> Result<Value> {
        let action = arguments["action"].as_str().ok_or_else(|| anyhow::anyhow!("Missing action"))?;

        match action {
            "save_fact" => {
                let fact = arguments["fact"].as_str().ok_or_else(|| anyhow::anyhow!("Missing fact"))?;
                crate::memory::profile::save_fact(&self.workspace, fact)?;
                Ok(json!({ "status": "success", "message": "Fact saved to user profile" }))
            }
            "get_all_facts" => {
                let facts = crate::memory::profile::get_all_facts(&self.workspace);
                Ok(json!(facts))
            }
            _ => Err(anyhow::anyhow!("Unknown action")),
        }
    }
}
