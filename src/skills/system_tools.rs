use async_trait::async_trait;
use serde::Deserialize;

use super::{Skill, SkillCategory, SkillExecution, SkillManifest, SkillPreview, SkillPreviewItem};

pub struct SystemToolsSkill;

/// Parameters accepted by the system tools skill.
///
/// The LLM decides which tool to invoke by setting the `tool` field,
/// then fills in the relevant parameter fields for that tool.
#[derive(Debug, Deserialize, Default)]
struct SystemToolsArgs {
    /// Which system tool to call.
    /// One of: list_processes, top_memory_procs, kill_process, get_process_detail,
    ///         disk_usage, disk_free, delete_file, list_dir, file_info, open_app
    #[serde(default)]
    tool: Option<String>,

    /// PID for kill_process / get_process_detail
    #[serde(default)]
    pid: Option<u32>,

    /// Count for top_memory_procs (default 10)
    #[serde(default)]
    count: Option<usize>,

    /// Path for disk_usage / delete_file / list_dir / file_info
    #[serde(default)]
    path: Option<String>,

    /// Bundle identifier for open_app, e.g. "com.apple.Safari"
    #[serde(default)]
    bundle_id: Option<String>,
}

impl SystemToolsSkill {
    fn resolve_tool(&self, args: &SystemToolsArgs) -> Result<system_tools::Tool, String> {
        match args.tool.as_deref() {
            Some("list_processes") | None => Ok(system_tools::Tool::ListProcesses),

            Some("top_memory_procs") => {
                Ok(system_tools::Tool::TopMemoryProcs(args.count.unwrap_or(10)))
            }

            Some("kill_process") => {
                let pid = args
                    .pid
                    .ok_or_else(|| "kill_process 需要 pid 参数".to_string())?;
                Ok(system_tools::Tool::KillProcess(pid))
            }

            Some("get_process_detail") => {
                let pid = args
                    .pid
                    .ok_or_else(|| "get_process_detail 需要 pid 参数".to_string())?;
                Ok(system_tools::Tool::GetProcessDetail(pid))
            }

            Some("disk_usage") => Ok(system_tools::Tool::DiskUsage(
                args.path.clone().unwrap_or_else(|| ".".to_string()),
            )),

            Some("disk_free") => Ok(system_tools::Tool::DiskFree),

            Some("delete_file") => {
                let path = args
                    .path
                    .clone()
                    .ok_or_else(|| "delete_file 需要 path 参数".to_string())?;
                Ok(system_tools::Tool::DeleteFile(path))
            }

            Some("list_dir") => Ok(system_tools::Tool::ListDir(
                args.path.clone().unwrap_or_else(|| ".".to_string()),
            )),

            Some("file_info") => {
                let path = args
                    .path
                    .clone()
                    .ok_or_else(|| "file_info 需要 path 参数".to_string())?;
                Ok(system_tools::Tool::FileInfo(path))
            }

            Some("open_app") => {
                let bundle_id = args
                    .bundle_id
                    .clone()
                    .ok_or_else(|| "open_app 需要 bundle_id 参数".to_string())?;
                Ok(system_tools::Tool::OpenApp(bundle_id))
            }

            Some(other) => Err(format!("未知工具: {}", other)),
        }
    }

    fn tool_execute_info(&self, tool_name: &str) -> (String, String, bool) {
        match tool_name {
            "list_processes" => (
                "list_processes".into(),
                "列出所有运行中的进程（PID、名称、CPU%、内存）".into(),
                false,
            ),
            "top_memory_procs" => (
                "top_memory_procs".into(),
                "按内存占用排序的前 N 个进程".into(),
                false,
            ),
            "kill_process" => (
                "kill_process".into(),
                "强制终止指定 PID 的进程".into(),
                true,
            ),
            "get_process_detail" => (
                "get_process_detail".into(),
                "获取指定 PID 的进程详细信息".into(),
                false,
            ),
            "disk_usage" => ("disk_usage".into(), "查看指定路径的磁盘占用".into(), false),
            "disk_free" => (
                "disk_free".into(),
                "查看所有挂载卷的剩余磁盘空间".into(),
                false,
            ),
            "delete_file" => (
                "delete_file".into(),
                "删除文件或目录（不可恢复）".into(),
                true,
            ),
            "list_dir" => ("list_dir".into(), "列出目录内容".into(), false),
            "file_info" => (
                "file_info".into(),
                "获取文件信息（大小、修改时间、类型）".into(),
                false,
            ),
            "open_app" => (
                "open_app".into(),
                "通过 bundle identifier 打开 macOS 应用".into(),
                false,
            ),
            _ => (tool_name.into(), String::new(), false),
        }
    }
}

#[async_trait]
impl Skill for SystemToolsSkill {
    fn manifest(&self) -> SkillManifest {
        SkillManifest {
            id: "system.tools".to_string(),
            name: "系统工具".to_string(),
            description: concat!(
                "系统工具：查看进程、CPU 占用、内存使用、磁盘空间、文件/目录操作等。\n",
                "可用工具：\n",
                "- list_processes：列出所有进程（PID/名称/CPU%/内存）\n",
                "- top_memory_procs：按内存排序的前 N 个进程（参数 count）\n",
                "- kill_process：强制终止进程（参数 pid）\n",
                "- get_process_detail：获取进程详情（参数 pid）\n",
                "- disk_usage：查看目录磁盘占用（参数 path）\n",
                "- disk_free：查看磁盘剩余空间\n",
                "- delete_file：删除文件/目录（参数 path，不可恢复）\n",
                "- list_dir：列出目录内容（参数 path）\n",
                "- file_info：获取文件信息（参数 path）\n",
                "- open_app：打开 macOS 应用（参数 bundle_id）\n\n",
                "使用方式：设置 tool 参数指定工具名，再填写对应的参数。\n",
                "例如：{\"tool\": \"list_processes\"} 或 {\"tool\": \"disk_free\"}。"
            )
            .to_string(),
            category: SkillCategory::System,
            danger_level: crate::agents::permission::DangerLevel::Dangerous,
        }
    }

    async fn preview(&self, args: serde_json::Value) -> anyhow::Result<SkillPreview> {
        let parsed: SystemToolsArgs = serde_json::from_value(args.clone()).unwrap_or_default();
        let tool_name = parsed.tool.as_deref().unwrap_or("list_processes");

        let (name, desc, dangerous) = self.tool_execute_info(tool_name);

        let mut warnings = Vec::new();
        if dangerous {
            warnings.push("⚠️ 该操作有风险（杀进程/删文件），执行前需要确认。".to_string());
        }

        Ok(SkillPreview {
            summary: format!("准备执行系统工具：{}", name),
            items: vec![SkillPreviewItem {
                label: name,
                detail: desc,
                bytes: 0,
            }],
            estimated_bytes: 0,
            warnings,
        })
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        _source: Option<&str>,
    ) -> anyhow::Result<SkillExecution> {
        let parsed: SystemToolsArgs = serde_json::from_value(args).unwrap_or_default();

        let tool = match self.resolve_tool(&parsed) {
            Ok(t) => t,
            Err(e) => {
                return Ok(SkillExecution {
                    summary: e,
                    ..Default::default()
                });
            }
        };

        let tool_name = tool.name().to_string();
        let is_dangerous = tool.is_dangerous();

        // 危险操作先返回 preview 提示，不直接执行
        if is_dangerous {
            return Ok(SkillExecution {
                summary: format!(
                    "⚠️ {} 是危险操作，请在参数中加 \"confirm\": true 以确认执行。\
                     \n\n如需确认，请重新调用时设置 \"tool\": \"{}\" 并带上 \"confirm\": true。",
                    tool_name, tool_name
                ),
                denied: true,
                ..Default::default()
            });
        }

        // 执行
        match tool.execute() {
            Ok(result) => {
                // 截断过长输出（比如 ps aux 可能很长）
                let truncated = if result.len() > 3000 {
                    format!(
                        "{}...\n\n（结果过长，仅显示前 3000 字符，共 {} 字符）",
                        &result[..3000],
                        result.len()
                    )
                } else {
                    result
                };
                Ok(SkillExecution {
                    summary: format!("[{}] 执行成功\n\n{}", tool_name, truncated),
                    success_items: vec![tool_name],
                    ..Default::default()
                })
            }
            Err(e) => Ok(SkillExecution {
                summary: format!("[{}] 执行失败: {}", tool_name, e),
                failed_items: vec![(tool_name, e)],
                ..Default::default()
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn manifest_returns_expected_id() {
        let m = SystemToolsSkill.manifest();
        assert_eq!(m.id, "system.tools");
        assert!(matches!(m.category, SkillCategory::System));
    }

    #[tokio::test]
    async fn preview_list_processes() {
        let preview = SystemToolsSkill
            .preview(serde_json::json!({"tool": "list_processes"}))
            .await
            .expect("preview should succeed");
        assert!(preview.summary.contains("list_processes"));
    }

    #[tokio::test]
    async fn execute_list_processes() {
        let result = SystemToolsSkill
            .execute(serde_json::json!({"tool": "list_processes"}), None)
            .await
            .expect("execute should succeed");
        assert!(!result.success_items.is_empty());
    }

    #[tokio::test]
    async fn execute_unknown_tool_returns_error() {
        let result = SystemToolsSkill
            .execute(serde_json::json!({"tool": "nonexistent"}), None)
            .await
            .expect("execute should not panic");
        assert!(result.summary.contains("未知工具"));
    }

    #[tokio::test]
    async fn dangerous_tool_requires_confirm() {
        let result = SystemToolsSkill
            .execute(
                serde_json::json!({"tool": "kill_process", "pid": 99999}),
                None,
            )
            .await
            .expect("execute should not panic");
        assert!(result.denied);
        assert!(result.summary.contains("confirm"));
    }
}
