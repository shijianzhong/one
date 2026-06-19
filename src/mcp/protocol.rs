//! MCP JSON-RPC 2.0 协议编解码
//!
//! MCP（Model Context Protocol）基于 JSON-RPC 2.0 标准协议。
//! 一个请求 = jsonrpc + id + method + params
//! 一个响应 = jsonrpc + id + (result | error)

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ── JSON-RPC 2.0 核心类型 ──────────────────────────────────────────────────────

/// JSON-RPC 2.0 请求
#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: u64,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl JsonRpcRequest {
    pub fn new(id: u64, method: impl Into<String>, params: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            method: method.into(),
            params,
        }
    }

    /// 创建 tools/list 请求
    pub fn list_tools(id: u64) -> Self {
        Self::new(id, "tools/list", None)
    }

    /// 创建 tools/call 请求
    pub fn call_tool(id: u64, name: impl Into<String>, args: Option<Value>) -> Self {
        Self::new(
            id,
            "tools/call",
            Some(serde_json::json!({
                "name": name.into(),
                "arguments": args.unwrap_or(Value::Null),
            })),
        )
    }
}

/// JSON-RPC 2.0 响应
#[derive(Debug, Clone, Deserialize)]
pub struct JsonRpcResponse {
    pub id: u64,
    #[serde(default)]
    pub result: Option<Value>,
    #[serde(default)]
    pub error: Option<JsonRpcError>,
}

impl JsonRpcResponse {
    /// 提取结果，如果有错误则返回 Err
    pub fn into_result(self) -> Result<Value, JsonRpcError> {
        match self.error {
            Some(err) => Err(err),
            None => Ok(self.result.unwrap_or(Value::Null)),
        }
    }
}

/// JSON-RPC 2.0 错误
#[derive(Debug, Clone, Deserialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(default)]
    pub data: Option<Value>,
}

impl std::fmt::Display for JsonRpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "JSON-RPC error {}: {}", self.code, self.message)
    }
}

impl std::error::Error for JsonRpcError {}

// ── MCP 协议标准方法 ───────────────────────────────────────────────────────────

/// MCP 标准方法名
pub mod methods {
    /// 列出服务器支持的所有工具
    pub const TOOLS_LIST: &str = "tools/list";
    /// 调用指定的工具
    pub const TOOLS_CALL: &str = "tools/call";
    /// 初始化通知（客户端已准备好接收消息）
    pub const NOTIFICATIONS_INITIALIZED: &str = "notifications/initialized";
}

// ── MCP 工具定义 ───────────────────────────────────────────────────────────────

/// MCP 工具定义（来自 tools/list 响应）
#[derive(Debug, Clone, Deserialize)]
pub struct McpToolDefinition {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub input_schema: Value,
}

/// tools/list 的响应
#[derive(Debug, Clone, Deserialize)]
pub struct ListToolsResult {
    pub tools: Vec<McpToolDefinition>,
}

/// tools/call 的响应内容块
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum McpContent {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "resource")]
    Resource { resource: McpResourceContents },
}

/// MCP 资源内容
#[derive(Debug, Clone, Deserialize)]
pub struct McpResourceContents {
    pub uri: String,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub mime_type: Option<String>,
}

/// tools/call 的响应
#[derive(Debug, Clone, Deserialize)]
pub struct CallToolResult {
    #[serde(default)]
    pub content: Vec<McpContent>,
    #[serde(default)]
    pub is_error: bool,
}

impl CallToolResult {
    /// 提取所有文本内容
    pub fn text_content(&self) -> String {
        self.content
            .iter()
            .filter_map(|c| match c {
                McpContent::Text { text } => Some(text.clone()),
                McpContent::Resource { resource } => resource.text.clone(),
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

// ── JSON-RPC 行协议解析 ────────────────────────────────────────────────────────

/// JSON-RPC 2.0 over stdio 的行协议解析器
///
/// MCP 的 stdio 传输使用换行分隔的 JSON 消息：
/// 每条消息是一行完整的 JSON，以 \n 结尾。
pub struct LineProtocolParser {
    buffer: String,
}

impl LineProtocolParser {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
        }
    }

    /// 喂入数据，返回解析出的完整消息行
    pub fn feed<'a>(&'a mut self, data: &str) -> Vec<String> {
        self.buffer.push_str(data);
        let mut messages = Vec::new();
        loop {
            match self.buffer.find('\n') {
                Some(pos) => {
                    let line = self.buffer[..pos].to_string();
                    self.buffer.drain(..=pos);
                    let trimmed = line.trim();
                    if !trimmed.is_empty() {
                        messages.push(trimmed.to_string());
                    }
                }
                None => break,
            }
        }
        messages
    }
}

impl Default for LineProtocolParser {
    fn default() -> Self {
        Self::new()
    }
}

// ── JSON RPC ID 生成器 ─────────────────────────────────────────────────────────

/// 线程安全的 JSON-RPC 请求 ID 生成器
pub struct RequestIdGenerator {
    next_id: std::sync::atomic::AtomicU64,
}

impl RequestIdGenerator {
    pub fn new() -> Self {
        Self {
            next_id: std::sync::atomic::AtomicU64::new(1),
        }
    }

    /// 生成下一个唯一 ID
    pub fn next(&self) -> u64 {
        self.next_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }
}

impl Default for RequestIdGenerator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_serialization() {
        let req = JsonRpcRequest::new(1, "tools/list", None);
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"jsonrpc\":\"2.0\""));
        assert!(json.contains("\"method\":\"tools/list\""));
        assert!(json.contains("\"id\":1"));
    }

    #[test]
    fn test_line_protocol_parser() {
        let mut parser = LineProtocolParser::new();
        let messages = parser.feed("{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}\n");
        assert_eq!(messages.len(), 1);
        assert!(messages[0].contains("\"jsonrpc\":\"2.0\""));
    }

    #[test]
    fn test_response_deserialization() {
        let json = r#"{"jsonrpc":"2.0","id":1,"result":{"tools":[{"name":"test","description":"a test tool","input_schema":{}}]}}"#;
        let resp: JsonRpcResponse = serde_json::from_str(json).unwrap();
        assert!(resp.error.is_none());
        let result: ListToolsResult = serde_json::from_value(resp.result.unwrap()).unwrap();
        assert_eq!(result.tools.len(), 1);
        assert_eq!(result.tools[0].name, "test");
    }

    #[test]
    fn test_call_tool_result() {
        let json = r#"{"content":[{"type":"text","text":"hello world"}],"is_error":false}"#;
        let result: CallToolResult = serde_json::from_str(json).unwrap();
        assert_eq!(result.text_content(), "hello world");
    }

    #[test]
    fn test_id_generator() {
        let gen = RequestIdGenerator::new();
        let a = gen.next();
        let b = gen.next();
        assert_eq!(b, a + 1);
    }
}
