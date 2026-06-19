//! MCP（Model Context Protocol）客户端
//!
//! ONE 内置 MCP 客户端，直接管理 MCP Server 子进程的生命周期。
//! 支持 stdio 和 HTTP 两种传输方式。
//!
//! # 架构
//!
//! ```text
//! ONE (Rust)
//!   └─ McpClientManager
//!        ├─ McpClientHandle (stdio)  → python3 claude_code_mcp_server.py
//!        ├─ McpClientHandle (stdio)  → npx @modelcontextprotocol/server-filesystem
//!        └─ McpClientHandle (http)   → https://remote-mcp-server.example.com
//! ```

pub mod config;
pub mod protocol;
pub mod transport;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use anyhow::{Context, Result};
use log::{error, info, warn};
use serde_json::Value;

use self::config::{McpConfig, McpServerConfig, TransportConfig};
use self::protocol::{
    CallToolResult, JsonRpcRequest, ListToolsResult, McpToolDefinition, RequestIdGenerator,
};
use self::transport::{HttpTransport, McpTransport, StdioTransport};

static GLOBAL_MCP_MANAGER: OnceLock<Arc<Mutex<McpClientManager>>> = OnceLock::new();

pub fn set_global_manager(manager: Arc<Mutex<McpClientManager>>) {
    let _ = GLOBAL_MCP_MANAGER.set(manager);
}

pub fn global_manager() -> Option<Arc<Mutex<McpClientManager>>> {
    GLOBAL_MCP_MANAGER.get().cloned()
}

/// MCP 客户端管理器
///
/// 管理所有 MCP Server 的生命周期，提供工具发现和调用接口。
pub struct McpClientManager {
    /// 所有已连接的 MCP 客户端
    clients: HashMap<String, McpClientHandle>,
    id_gen: RequestIdGenerator,
}

/// 单个 MCP Server 的连接句柄
struct McpClientHandle {
    config: McpServerConfig,
    transport: Box<dyn McpTransport>,
    /// 已发现的工具列表（缓存）
    tools: Vec<McpToolDefinition>,
}

impl std::fmt::Debug for McpClientHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpClientHandle")
            .field("tool_count", &self.tools.len())
            .finish()
    }
}

impl McpClientManager {
    /// 创建并连接到所有配置的 MCP Server
    pub async fn connect(config: &McpConfig) -> Self {
        let mut manager = Self {
            clients: HashMap::new(),
            id_gen: RequestIdGenerator::new(),
        };

        for (name, server_config) in &config.mcp_servers {
            match manager.connect_server(name, server_config) {
                Ok(handle) => {
                    let tool_count = handle.tools.len();
                    manager.clients.insert(name.clone(), handle);
                    info!(
                        "[MCP] Connected to server '{}' ({} tools)",
                        name, tool_count
                    );
                }
                Err(e) => {
                    warn!("[MCP] Failed to connect to server '{}': {}", name, e);
                }
            }
        }

        manager
    }

    /// 连接到单个 MCP Server
    fn connect_server(
        &mut self,
        name: &str,
        server_config: &McpServerConfig,
    ) -> Result<McpClientHandle> {
        let transport: Box<dyn McpTransport> = match &server_config.transport {
            TransportConfig::Stdio { command, args, env } => {
                let resolved_env = McpConfig::resolve_env(env);
                let mut transport = StdioTransport::spawn(
                    command,
                    args,
                    &resolved_env.into_iter().collect::<Vec<_>>(),
                )?;
                // 发送初始化通知
                transport.send_initialized()?;
                Box::new(transport)
            }
            TransportConfig::Http { url, headers } => Box::new(HttpTransport::new(
                url.clone(),
                headers.clone(),
                server_config.timeout_secs,
            )?),
        };

        let mut handle = McpClientHandle {
            config: server_config.clone(),
            transport,
            tools: Vec::new(),
        };

        // 发现工具
        match handle.discover_tools(&mut self.id_gen) {
            Ok(tools) => {
                handle.tools = tools;
            }
            Err(e) => {
                warn!("[MCP] Tool discovery failed for '{}': {}", name, e);
            }
        }

        Ok(handle)
    }

    /// 向所有已连接的 Server 发现工具
    pub fn discover_all_tools(&mut self) -> Vec<(String, McpToolDefinition)> {
        let mut all_tools = Vec::new();
        for (name, handle) in &mut self.clients {
            match handle.discover_tools(&mut self.id_gen) {
                Ok(tools) => {
                    for tool in &tools {
                        all_tools.push((name.clone(), tool.clone()));
                    }
                    handle.tools = tools;
                }
                Err(e) => {
                    warn!("[MCP] Failed to discover tools from '{}': {}", name, e);
                }
            }
        }
        all_tools
    }

    /// 调用指定 Server 的工具
    pub fn call_tool(&mut self, server: &str, tool: &str, args: Value) -> Result<String> {
        let handle = self
            .clients
            .get_mut(server)
            .context(format!("MCP server '{}' not found", server))?;

        let request = JsonRpcRequest::call_tool(self.id_gen.next(), tool, Some(args));
        let response = handle.transport.request(&request)?;
        let result = response.into_result()?;

        // 解析为 CallToolResult
        let call_result: CallToolResult = serde_json::from_value(result)?;
        Ok(call_result.text_content())
    }

    pub fn server_timeout_secs(&self, server: &str) -> u64 {
        self.clients
            .get(server)
            .map(|handle| handle.config.timeout_secs.max(1))
            .unwrap_or(30)
    }

    /// 断开所有连接
    pub fn shutdown_all(&mut self) {
        for (name, mut handle) in self.clients.drain() {
            if let Err(e) = handle.transport.shutdown() {
                error!("[MCP] Error shutting down '{}': {}", name, e);
            } else {
                info!("[MCP] Disconnected from '{}'", name);
            }
        }
    }

    /// 获取所有已发现的工具列表
    pub fn all_tools(&self) -> Vec<McpToolInfo> {
        let mut tools = Vec::new();
        for (server_name, handle) in &self.clients {
            for tool_def in &handle.tools {
                tools.push(McpToolInfo {
                    server_name: server_name.clone(),
                    tool_name: tool_def.name.clone(),
                    description: tool_def.description.clone(),
                    input_schema: tool_def.input_schema.clone(),
                });
            }
        }
        tools
    }

    /// 获取连接的 Server 数量
    pub fn server_count(&self) -> usize {
        self.clients.len()
    }

    /// 获取工具总数
    pub fn tool_count(&self) -> usize {
        self.clients.values().map(|h| h.tools.len()).sum()
    }

    /// 检查指定 Server 是否已连接
    pub fn has_server(&self, name: &str) -> bool {
        self.clients.contains_key(name)
    }
}

pub async fn call_tool_async(
    manager: Arc<Mutex<McpClientManager>>,
    server: String,
    tool: String,
    args: Value,
) -> Result<String> {
    let timeout_secs = manager
        .try_lock()
        .map(|manager| manager.server_timeout_secs(&server))
        .unwrap_or(30);
    let server_for_error = server.clone();
    let tool_for_error = tool.clone();
    let task = tokio::task::spawn_blocking(move || {
        let mut manager = manager
            .lock()
            .map_err(|_| anyhow::anyhow!("MCP manager lock poisoned"))?;
        manager.call_tool(&server, &tool, args)
    });

    match tokio::time::timeout(Duration::from_secs(timeout_secs), task).await {
        Ok(joined) => joined.context("MCP tool call worker failed")?,
        Err(_) => anyhow::bail!(
            "MCP tool call {}/{} timed out after {}s",
            server_for_error,
            tool_for_error,
            timeout_secs
        ),
    }
}

impl McpClientHandle {
    /// 发现该 Server 支持的所有工具
    fn discover_tools(
        &mut self,
        id_gen: &mut RequestIdGenerator,
    ) -> Result<Vec<McpToolDefinition>> {
        let request = JsonRpcRequest::list_tools(id_gen.next());
        let response = self.transport.request(&request)?;
        let result = response.into_result()?;
        let list_result: ListToolsResult = serde_json::from_value(result)?;
        Ok(list_result.tools)
    }
}

/// MCP 工具信息（用于注册到 ToolRegistry）
#[derive(Debug, Clone)]
pub struct McpToolInfo {
    pub server_name: String,
    pub tool_name: String,
    pub description: String,
    pub input_schema: Value,
}

impl McpToolInfo {
    /// 获取完整的工具名称（用于 LLM tool calling）
    pub fn full_name(&self) -> String {
        format!("mcp:{}:{}", self.server_name, self.tool_name)
    }
}

impl Drop for McpClientManager {
    fn drop(&mut self) {
        self.shutdown_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_manager() {
        let config = McpConfig {
            mcp_servers: HashMap::new(),
        };
        let rt = tokio::runtime::Runtime::new().unwrap();
        let manager = rt.block_on(McpClientManager::connect(&config));
        assert_eq!(manager.server_count(), 0);
        assert_eq!(manager.tool_count(), 0);
    }
}
