use serde_json::Value;
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

        if name == "start_coding_workflow" {
            return self.dispatch_start_coding_workflow(args, on_event);
        }

        if name == "run_in_terminal" {
            return self.dispatch_run_in_terminal(args, on_event);
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

    fn dispatch_start_coding_workflow<F>(&self, args: Value, on_event: &mut F) -> String
    where
        F: FnMut(OrchestratorEvent) + Send,
    {
        let user_request = args
            .get("user_request")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let main_agent_summary = args
            .get("main_agent_summary")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let known_constraints = string_array_arg(&args, "known_constraints");
        let suggested_direction = args
            .get("suggested_direction")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let clarification_focus = string_array_arg(&args, "clarification_focus");

        if user_request.trim().is_empty() {
            return "请提供 user_request。".to_string();
        }

        on_event(OrchestratorEvent::CodingWorkflowRequested {
            user_request,
            main_agent_summary,
            known_constraints,
            suggested_direction,
            clarification_focus,
        });
        "编码工作流已启动。Claude Code 会先做方案梳理，完成后等待用户确认再执行编码。".to_string()
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
            return "命令已发送到终端执行。用户可以在终端中查看实时输出。".to_string();
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

fn string_array_arg(args: &Value, key: &str) -> Vec<String> {
    args.get(key)
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
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

        assert!(result.contains("命令已发送到终端执行"));
        assert!(matches!(
            events.as_slice(),
            [OrchestratorEvent::RunInTerminal { command, work_dir }]
                if command == "cargo test" && work_dir == "/tmp/project"
        ));
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
