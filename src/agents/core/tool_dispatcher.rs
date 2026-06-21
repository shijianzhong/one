use serde_json::{json, Value};
use std::sync::Arc;

use super::ToolCall;
use crate::agents::core::orchestrator::OrchestratorEvent;
use crate::agents::permission::{classify_mcp_tool_kind, PermissionDecision};
use crate::mcp::McpClientManager;

pub struct ToolDispatcher {
    mcp_manager: Option<Arc<std::sync::Mutex<McpClientManager>>>,
}

impl ToolDispatcher {
    pub fn new(mcp_manager: Option<Arc<std::sync::Mutex<McpClientManager>>>) -> Self {
        Self { mcp_manager }
    }

    pub async fn dispatch<F>(&self, call: &ToolCall, on_event: &mut F) -> String
    where
        F: FnMut(OrchestratorEvent) + Send,
    {
        let args: Value = serde_json::from_str(&call.arguments).unwrap_or(Value::Null);
        let name = &call.name;

        if name == "run_in_terminal" {
            return self.dispatch_run_in_terminal(args, on_event);
        }

        match name.as_str() {
            "detect_coding_clis" => return self.dispatch_detect_coding_clis(),
            "install_coding_cli" => return self.dispatch_install_coding_cli(args),
            "start_coding_session" | "start_coding_terminal_runtime" => {
                return self.dispatch_start_coding_session(args, on_event)
            }
            "send_to_coding_session" | "send_to_coding_terminal_runtime" => {
                return self.dispatch_send_to_coding_session(args, on_event)
            }
            "read_coding_session_output" | "read_coding_terminal_output" => {
                return self.dispatch_read_coding_session_output(args, on_event)
            }
            "inspect_coding_terminal_runtime" => {
                return self.dispatch_inspect_coding_session(args, on_event)
            }
            "stop_coding_session" | "stop_coding_terminal_runtime" => {
                return self.dispatch_stop_coding_session(args, on_event)
            }
            "list_coding_sessions" | "list_coding_terminal_runtimes" => {
                on_event(OrchestratorEvent::ListCodingSessions);
                return "正在列出 coding terminal runtime。".to_string();
            }
            "get_workspace_write_status" => {
                on_event(OrchestratorEvent::GetWorkspaceWriteStatus);
                return "正在查询当前 workspace 写入状态。".to_string();
            }
            _ => {}
        }

        if name == "run_system_task" {
            let skill_id = args
                .get("skill_id")
                .and_then(|v| v.as_str())
                .map(|s| s.trim())
                .filter(|s| !s.is_empty());

            if let Some(sid) = skill_id {
                return self.execute_skill(sid, args.clone()).await;
            } else {
                let known: Vec<String> = crate::skills::skill_manifests()
                    .into_iter()
                    .map(|m| m.id)
                    .collect();
                return format!("请在 skill_id 参数中指定 Skill。当前已安装：{:?}", known);
            }
        }

        if let Some(source) = crate::agents::core::tool_registry::tool_registry()
            .lock()
            .ok()
            .and_then(|reg| reg.resolve_tool_source(name))
        {
            match source {
                crate::agents::core::agent::ToolSource::Skill(skill_id) => {
                    return self.execute_skill(&skill_id, args).await;
                }
                crate::agents::core::agent::ToolSource::Mcp { server, tool_name } => {
                    return self.execute_mcp_tool(server, tool_name, args).await;
                }
                crate::agents::core::agent::ToolSource::Builtin(tool) => {
                    match tool.call(args).await {
                        Ok(res) => return res.to_string(),
                        Err(e) => return format!("Error: {}", e),
                    }
                }
            }
        }

        format!("Error: Tool '{}' not found", name)
    }

    fn dispatch_start_coding_session<F>(&self, args: Value, on_event: &mut F) -> String
    where
        F: FnMut(OrchestratorEvent) + Send,
    {
        let prompt = args
            .get("prompt")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let agent_kind = args
            .get("agent_kind")
            .and_then(|v| v.as_str())
            .unwrap_or("claude")
            .to_string();
        let write_mode = args
            .get("write_mode")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        if prompt.trim().is_empty() {
            return "请提供 prompt。".to_string();
        }

        let Some(provider) = crate::runtime::resolve_coding_agent_provider(&agent_kind) else {
            return format!(
                "未找到 coding CLI `{}`。可用 provider：{}",
                agent_kind,
                crate::runtime::configured_coding_agent_usage()
            );
        };
        let availability = crate::runtime::detect_coding_cli(&provider);
        if !availability.installed {
            return format!(
                "{} 未安装或不在 PATH 中，无法启动终端 runtime。\n安装说明：{}",
                provider.label(),
                provider.install_instructions()
            );
        }

        on_event(OrchestratorEvent::StartCodingSession {
            agent_kind: provider.id.clone(),
            prompt,
            write_mode,
        });
        format!(
            "终端 coding runtime 启动请求已提交，将在右侧终端运行 `{}`。",
            provider.command_line()
        )
    }

    fn dispatch_detect_coding_clis(&self) -> String {
        let clis = crate::runtime::detect_configured_coding_clis()
            .into_iter()
            .map(|item| {
                json!({
                    "id": item.provider.id,
                    "label": item.provider.label,
                    "command": item.provider.command,
                    "args": item.provider.args,
                    "command_line": item.provider.command_line(),
                    "installed": item.installed,
                    "resolved_path": item.resolved_path,
                    "has_install_command": item.provider.install_command.is_some(),
                    "install_instructions": item.provider.install_instructions(),
                })
            })
            .collect::<Vec<_>>();
        json!({
            "coding_clis": clis,
            "guidance": "编码任务前先检查 installed=true 的 CLI；如果没有可用 CLI，询问用户是否安装 Claude Code。用户确认后可调用 install_coding_cli。"
        })
        .to_string()
    }

    fn dispatch_install_coding_cli(&self, args: Value) -> String {
        let agent_kind = args
            .get("agent_kind")
            .and_then(|v| v.as_str())
            .unwrap_or("claude");
        let confirmed = args
            .get("confirmed")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let Some(provider) = crate::runtime::resolve_coding_agent_provider(agent_kind) else {
            return format!(
                "未找到 coding CLI `{}`。可用 provider：{}",
                agent_kind,
                crate::runtime::configured_coding_agent_usage()
            );
        };
        if crate::runtime::detect_coding_cli(&provider).installed {
            return format!(
                "{} 已安装，可直接启动 `{}`。",
                provider.label(),
                provider.command_line()
            );
        }
        let Some(command) = provider.install_command.clone() else {
            return format!(
                "{} 没有配置自动安装命令。\n安装说明：{}",
                provider.label(),
                provider.install_instructions()
            );
        };
        if !confirmed {
            return format!(
                "安装 {} 需要执行命令：\n{}\n请先征得用户确认；确认后用 confirmed=true 再调用 install_coding_cli。",
                provider.label(),
                command
            );
        }

        let output = std::process::Command::new("sh")
            .arg("-lc")
            .arg(&command)
            .output();
        match output {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                if crate::runtime::detect_coding_cli(&provider).installed {
                    return format!(
                        "{} 安装完成，已检测到 `{}`。\nstdout:\n{}\nstderr:\n{}",
                        provider.label(),
                        provider.command,
                        stdout,
                        stderr
                    );
                }
                format!(
                    "{} 安装命令已结束，但仍未检测到 `{}`。\nstatus={}\nstdout:\n{}\nstderr:\n{}\n安装说明：{}",
                    provider.label(),
                    provider.command,
                    output.status,
                    stdout,
                    stderr,
                    provider.install_instructions()
                )
            }
            Err(error) => format!(
                "{} 安装命令执行失败：{}\n安装说明：{}",
                provider.label(),
                error,
                provider.install_instructions()
            ),
        }
    }

    fn dispatch_send_to_coding_session<F>(&self, args: Value, on_event: &mut F) -> String
    where
        F: FnMut(OrchestratorEvent) + Send,
    {
        let text = args
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        if text.trim().is_empty() {
            return "请提供 text。".to_string();
        }
        let session_id = optional_string_arg(&args, "session_id");
        on_event(OrchestratorEvent::SendToCodingSession { session_id, text });
        "输入已发送到 coding terminal runtime。".to_string()
    }

    fn dispatch_read_coding_session_output<F>(&self, args: Value, on_event: &mut F) -> String
    where
        F: FnMut(OrchestratorEvent) + Send,
    {
        let session_id = optional_string_arg(&args, "session_id");
        let limit = args
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(40)
            .clamp(1, 200) as usize;
        on_event(OrchestratorEvent::ReadCodingSessionOutput { session_id, limit });
        "正在读取 coding terminal runtime 最近输出。".to_string()
    }

    fn dispatch_inspect_coding_session<F>(&self, args: Value, on_event: &mut F) -> String
    where
        F: FnMut(OrchestratorEvent) + Send,
    {
        let session_id = optional_string_arg(&args, "session_id");
        let limit = args
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(80)
            .clamp(1, 200) as usize;
        on_event(OrchestratorEvent::InspectCodingSession { session_id, limit });
        "正在分析右侧终端 coding runtime 状态。".to_string()
    }

    fn dispatch_stop_coding_session<F>(&self, args: Value, on_event: &mut F) -> String
    where
        F: FnMut(OrchestratorEvent) + Send,
    {
        let session_id = optional_string_arg(&args, "session_id");
        on_event(OrchestratorEvent::StopCodingSession { session_id });
        "停止 coding terminal runtime 的请求已提交。".to_string()
    }

    fn dispatch_run_in_terminal<F>(&self, args: Value, on_event: &mut F) -> String
    where
        F: FnMut(OrchestratorEvent) + Send,
    {
        let command = args
            .get("command")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        if !command.is_empty() {
            let work_dir = args
                .get("work_dir")
                .and_then(|v| v.as_str())
                .unwrap_or(".")
                .to_string();
            on_event(OrchestratorEvent::RunInTerminal { command, work_dir });
            return "命令已发送到右侧终端的 Shell tab 执行。".to_string();
        }
        "请提供要执行的命令。".to_string()
    }

    async fn execute_mcp_tool(&self, server: String, tool_name: String, args: Value) -> String {
        let detail = format!("MCP {}/{} {}", server, tool_name, args);
        let kind = classify_mcp_tool_kind(&tool_name);
        match crate::agents::permission::global()
            .request_async(kind, detail, None)
            .await
        {
            PermissionDecision::Allow => {}
            PermissionDecision::Deny(reason) => {
                return format!("MCP 调用已拒绝：{}", reason);
            }
            PermissionDecision::Ask => {
                return "MCP 调用未获得授权。".to_string();
            }
        }
        if let Some(mcp) = &self.mcp_manager {
            match crate::mcp::call_tool_async(mcp.clone(), server.clone(), tool_name.clone(), args)
                .await
            {
                Ok(result) => return result,
                Err(e) => return format!("MCP Error: {}", e),
            }
        }
        format!("MCP server '{}' not connected", server)
    }

    async fn execute_skill(&self, skill_id: &str, args: Value) -> String {
        let sid = skill_id.strip_prefix("skill:").unwrap_or(skill_id);
        let apply = args.get("apply").and_then(|v| v.as_bool()).unwrap_or(false);
        let skill_args = args
            .get("args")
            .cloned()
            .unwrap_or(Value::Object(Default::default()));

        if let Some(skill) = crate::skills::find_skill(sid) {
            if apply {
                match skill.execute(skill_args, None).await {
                    Ok(exec) => {
                        if sid == "system.tools" {
                            exec.summary
                        } else {
                            serde_json::json!({
                                "stage": "execute",
                                "skill_id": sid,
                                "denied": exec.denied,
                                "summary": exec.summary,
                                "freed_bytes": exec.freed_bytes,
                                "success": exec.success_items,
                                "failed": exec.failed_items.iter()
                                    .map(|(k, v)| serde_json::json!({"item": k, "error": v}))
                                    .collect::<Vec<_>>(),
                            })
                            .to_string()
                        }
                    }
                    Err(e) => format!("Error: skill execute failed: {}", e),
                }
            } else {
                match skill.preview(skill_args).await {
                    Ok(preview) => serde_json::json!({
                        "stage": "preview",
                        "skill_id": sid,
                        "summary": preview.summary,
                        "estimated_bytes": preview.estimated_bytes,
                        "items": preview.items.iter().map(|it| serde_json::json!({
                            "label": it.label,
                            "detail": it.detail,
                            "bytes": it.bytes,
                        })).collect::<Vec<_>>(),
                        "warnings": preview.warnings,
                        "hint": "若用户同意继续，再次调用时把 apply 设为 true。",
                    })
                    .to_string(),
                    Err(e) => format!("Error: skill preview failed: {}", e),
                }
            }
        } else {
            let known: Vec<String> = crate::skills::skill_manifests()
                .into_iter()
                .map(|m| m.id)
                .collect();
            format!(
                "Error: skill_id '{}' not found. Available: {:?}",
                sid, known
            )
        }
    }
}

fn optional_string_arg(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::core::tool_registry::{tool_registry, McpToolRegistration};
    use crate::agents::core::Tool;
    use anyhow::Result;
    use async_trait::async_trait;

    struct EchoTool;

    #[async_trait]
    impl Tool for EchoTool {
        fn name(&self) -> &str {
            "dispatcher_test_echo"
        }

        fn description(&self) -> &str {
            "Echoes test input"
        }

        fn parameters_schema(&self) -> Value {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "value": { "type": "string" }
                }
            })
        }

        async fn call(&self, arguments: Value) -> Result<Value> {
            Ok(serde_json::json!({
                "echo": arguments.get("value").and_then(|v| v.as_str()).unwrap_or_default()
            }))
        }
    }

    #[tokio::test]
    async fn unknown_tool_returns_error() {
        let dispatcher = ToolDispatcher::new(None);
        let call = ToolCall {
            id: "call_1".to_string(),
            name: "missing_tool".to_string(),
            arguments: "{}".to_string(),
        };
        let mut events = Vec::new();

        let result = dispatcher
            .dispatch(&call, &mut |event| events.push(event))
            .await;

        assert_eq!(result, "Error: Tool 'missing_tool' not found");
        assert!(events.is_empty());
    }

    #[tokio::test]
    async fn run_in_terminal_emits_runtime_event() {
        let dispatcher = ToolDispatcher::new(None);
        let call = ToolCall {
            id: "call_1".to_string(),
            name: "run_in_terminal".to_string(),
            arguments: serde_json::json!({
                "command": "cargo test",
                "work_dir": "/tmp/project"
            })
            .to_string(),
        };
        let mut events = Vec::new();

        let result = dispatcher
            .dispatch(&call, &mut |event| events.push(event))
            .await;

        assert!(result.contains("命令已发送到右侧终端的 Shell tab 执行"));
        assert!(matches!(
            events.as_slice(),
            [OrchestratorEvent::RunInTerminal { command, work_dir }]
                if command == "cargo test" && work_dir == "/tmp/project"
        ));
    }

    #[tokio::test]
    async fn start_coding_session_emits_runtime_event() {
        let dispatcher = ToolDispatcher::new(None);
        let call = ToolCall {
            id: "call_1".to_string(),
            name: "start_coding_session".to_string(),
            arguments: serde_json::json!({
                "agent_kind": "codex",
                "prompt": "review this project",
                "write_mode": false
            })
            .to_string(),
        };
        let mut events = Vec::new();

        let result = dispatcher
            .dispatch(&call, &mut |event| events.push(event))
            .await;

        if events.is_empty() {
            assert!(result.contains("未安装") || result.contains("无法启动"));
        } else {
            assert!(result.contains("终端 coding runtime"));
            assert!(matches!(
                events.as_slice(),
                [OrchestratorEvent::StartCodingSession {
                    agent_kind,
                    prompt,
                    write_mode
                }] if agent_kind == "codex" && prompt == "review this project" && !write_mode
            ));
        }
    }

    #[tokio::test]
    async fn coding_session_control_tools_emit_runtime_events() {
        let dispatcher = ToolDispatcher::new(None);
        let mut events = Vec::new();

        let send = ToolCall {
            id: "call_1".to_string(),
            name: "send_to_coding_session".to_string(),
            arguments: serde_json::json!({"text": "continue"}).to_string(),
        };
        let _ = dispatcher
            .dispatch(&send, &mut |event| events.push(event))
            .await;

        let read = ToolCall {
            id: "call_2".to_string(),
            name: "read_coding_session_output".to_string(),
            arguments: serde_json::json!({"limit": 12}).to_string(),
        };
        let _ = dispatcher
            .dispatch(&read, &mut |event| events.push(event))
            .await;

        let inspect = ToolCall {
            id: "call_3".to_string(),
            name: "inspect_coding_terminal_runtime".to_string(),
            arguments: serde_json::json!({"limit": 33}).to_string(),
        };
        let _ = dispatcher
            .dispatch(&inspect, &mut |event| events.push(event))
            .await;

        let stop = ToolCall {
            id: "call_4".to_string(),
            name: "stop_coding_session".to_string(),
            arguments: "{}".to_string(),
        };
        let _ = dispatcher
            .dispatch(&stop, &mut |event| events.push(event))
            .await;

        assert!(matches!(
            &events[0],
            OrchestratorEvent::SendToCodingSession { session_id: None, text }
                if text == "continue"
        ));
        assert!(matches!(
            &events[1],
            OrchestratorEvent::ReadCodingSessionOutput { session_id: None, limit }
                if *limit == 12
        ));
        assert!(matches!(
            &events[2],
            OrchestratorEvent::InspectCodingSession { session_id: None, limit }
                if *limit == 33
        ));
        assert!(matches!(
            &events[3],
            OrchestratorEvent::StopCodingSession { session_id: None }
        ));
    }

    #[tokio::test]
    async fn coding_terminal_runtime_alias_tools_emit_runtime_events() {
        let dispatcher = ToolDispatcher::new(None);
        let mut events = Vec::new();

        let send = ToolCall {
            id: "call_1".to_string(),
            name: "send_to_coding_terminal_runtime".to_string(),
            arguments: serde_json::json!({"text": "continue"}).to_string(),
        };
        let _ = dispatcher
            .dispatch(&send, &mut |event| events.push(event))
            .await;

        let read = ToolCall {
            id: "call_2".to_string(),
            name: "read_coding_terminal_output".to_string(),
            arguments: serde_json::json!({"limit": 9}).to_string(),
        };
        let _ = dispatcher
            .dispatch(&read, &mut |event| events.push(event))
            .await;

        let stop = ToolCall {
            id: "call_3".to_string(),
            name: "stop_coding_terminal_runtime".to_string(),
            arguments: "{}".to_string(),
        };
        let _ = dispatcher
            .dispatch(&stop, &mut |event| events.push(event))
            .await;

        assert!(matches!(
            &events[0],
            OrchestratorEvent::SendToCodingSession { session_id: None, text }
                if text == "continue"
        ));
        assert!(matches!(
            &events[1],
            OrchestratorEvent::ReadCodingSessionOutput { session_id: None, limit }
                if *limit == 9
        ));
        assert!(matches!(
            &events[2],
            OrchestratorEvent::StopCodingSession { session_id: None }
        ));
    }

    #[tokio::test]
    async fn detect_coding_clis_returns_configured_status() {
        let dispatcher = ToolDispatcher::new(None);
        let call = ToolCall {
            id: "call_1".to_string(),
            name: "detect_coding_clis".to_string(),
            arguments: "{}".to_string(),
        };
        let mut events = Vec::new();

        let result = dispatcher
            .dispatch(&call, &mut |event| events.push(event))
            .await;

        assert!(events.is_empty());
        assert!(result.contains("coding_clis"));
        assert!(result.contains("installed"));
    }

    #[tokio::test]
    async fn install_coding_cli_requires_confirmation() {
        let dispatcher = ToolDispatcher::new(None);
        let call = ToolCall {
            id: "call_1".to_string(),
            name: "install_coding_cli".to_string(),
            arguments: serde_json::json!({
                "agent_kind": "claude",
                "confirmed": false
            })
            .to_string(),
        };
        let mut events = Vec::new();

        let result = dispatcher
            .dispatch(&call, &mut |event| events.push(event))
            .await;

        assert!(events.is_empty());
        assert!(
            result.contains("需要执行命令")
                || result.contains("已安装")
                || result.contains("没有配置自动安装命令")
        );
    }

    #[tokio::test]
    async fn builtin_tool_routes_through_registry() {
        {
            let mut registry = tool_registry().lock().unwrap();
            registry.register_builtin(Arc::new(EchoTool));
        }

        let dispatcher = ToolDispatcher::new(None);
        let call = ToolCall {
            id: "call_1".to_string(),
            name: "dispatcher_test_echo".to_string(),
            arguments: serde_json::json!({"value": "hello"}).to_string(),
        };
        let mut events = Vec::new();

        let result = dispatcher
            .dispatch(&call, &mut |event| events.push(event))
            .await;

        assert_eq!(result, r#"{"echo":"hello"}"#);
        assert!(events.is_empty());
    }

    #[tokio::test]
    async fn run_system_task_reports_missing_skill_id() {
        let dispatcher = ToolDispatcher::new(None);
        let call = ToolCall {
            id: "call_1".to_string(),
            name: "run_system_task".to_string(),
            arguments: "{}".to_string(),
        };
        let mut events = Vec::new();

        let result = dispatcher
            .dispatch(&call, &mut |event| events.push(event))
            .await;

        assert!(result.contains("请在 skill_id 参数中指定 Skill"));
        assert!(events.is_empty());
    }

    #[tokio::test]
    async fn mcp_tool_routes_through_registry() {
        {
            let mut registry = tool_registry().lock().unwrap();
            registry.register_mcp(McpToolRegistration {
                server_name: "dispatcher_test_server".to_string(),
                tool_name: "list_items".to_string(),
                description: "List items".to_string(),
                input_schema: serde_json::json!({"type": "object"}),
            });
        }

        let dispatcher = ToolDispatcher::new(None);
        let call = ToolCall {
            id: "call_1".to_string(),
            name: "mcp__dispatcher_test_server__list_items".to_string(),
            arguments: "{}".to_string(),
        };
        let mut events = Vec::new();

        let result = dispatcher
            .dispatch(&call, &mut |event| events.push(event))
            .await;

        assert_eq!(result, "MCP server 'dispatcher_test_server' not connected");
        assert!(events.is_empty());
    }
}
