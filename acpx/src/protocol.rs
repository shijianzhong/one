//! ACP Protocol Message Types
//!
//! JSON-RPC 2.0 message definitions for the Agent Client Protocol.

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

/// JSON-RPC version constant
pub const JSONRPC_VERSION: &str = "2.0";

/// Request/Response ID - can be string, number, or null
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Id {
    String(String),
    Number(i64),
    Null,
}

impl Default for Id {
    fn default() -> Self {
        Id::Null
    }
}

impl From<i64> for Id {
    fn from(n: i64) -> Self {
        Id::Number(n)
    }
}

impl From<String> for Id {
    fn from(s: String) -> Self {
        Id::String(s)
    }
}

impl From<&str> for Id {
    fn from(s: &str) -> Self {
        Id::String(s.to_string())
    }
}

/// JSON-RPC Message envelope
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Message {
    Request(Request),
    Notification(Notification),
    Response(Response),
    ErrorResponse(ErrorResponse),
}

impl Message {
    pub fn method(&self) -> Option<&str> {
        match self {
            Message::Request(r) => Some(&r.method),
            Message::Notification(n) => Some(&n.method),
            _ => None,
        }
    }

    pub fn id(&self) -> Option<&Id> {
        match self {
            Message::Request(r) => Some(&r.id),
            Message::Response(r) => Some(&r.id),
            Message::ErrorResponse(e) => e.id.as_ref(),
            Message::Notification(_) => None,
        }
    }
}

/// JSON-RPC Request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    #[serde(rename = "jsonrpc")]
    pub jsonrpc: String,

    pub method: String,

    #[serde(rename = "params")]
    pub params: Option<JsonValue>,

    pub id: Id,
}

/// JSON-RPC Notification (no response expected)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    #[serde(rename = "jsonrpc")]
    pub jsonrpc: String,

    pub method: String,

    #[serde(rename = "params")]
    pub params: Option<JsonValue>,
}

/// JSON-RPC Response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    #[serde(rename = "jsonrpc")]
    pub jsonrpc: String,

    #[serde(rename = "result")]
    pub result: JsonValue,

    pub id: Id,
}

/// JSON-RPC Error Response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorResponse {
    #[serde(rename = "jsonrpc")]
    pub jsonrpc: String,

    pub error: Error,

    pub id: Option<Id>,
}

/// JSON-RPC Error
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Error {
    pub code: i32,
    pub message: String,
    pub data: Option<JsonValue>,
}

// ============================================================================
// Initialize
// ============================================================================

/// Initialize request parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeParams {
    #[serde(rename = "protocolVersion")]
    pub protocol_version: u32,

    #[serde(rename = "clientInfo")]
    pub client_info: Option<ClientInfo>,

    pub capabilities: ClientCapabilities,
}

/// Client information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientInfo {
    pub name: String,
    pub version: Option<String>,
}

/// Initialize result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeResult {
    #[serde(rename = "protocolVersion")]
    pub protocol_version: u32,

    #[serde(rename = "agentInfo")]
    pub agent_info: AgentInfo,

    pub capabilities: AgentCapabilities,
}

/// Agent information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInfo {
    pub name: String,
    pub version: Option<String>,
}

/// Client capabilities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientCapabilities {
    pub fs: Option<FsCapabilities>,
    pub terminal: Option<TerminalCapabilities>,
}

/// Filesystem capabilities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FsCapabilities {
    #[serde(rename = "readTextFile")]
    pub read_text_file: bool,

    #[serde(rename = "writeTextFile")]
    pub write_text_file: bool,
}

/// Terminal capabilities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalCapabilities {
    pub create: bool,
    pub output: bool,
    pub release: bool,
    #[serde(rename = "waitForExit")]
    pub wait_for_exit: bool,
    pub kill: bool,
}

/// Tool definition following JSON Schema / AI tool standards
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: JsonValue,
    #[serde(default)]
    pub strict: bool,
    #[serde(rename = "isDangerous")]
    #[serde(default)]
    pub is_dangerous: bool,
}

/// Agent capabilities extended for tool support
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCapabilities {
    #[serde(rename = "loadSession")]
    pub load_session: bool,

    #[serde(rename = "promptCapabilities")]
    pub prompt_capabilities: PromptCapabilities,

    #[serde(rename = "mcpCapabilities")]
    pub mcp_capabilities: McpCapabilities,

    /// List of tools/skills exposed by this agent
    #[serde(default)]
    pub tools: Vec<ToolDefinition>,
}

/// Prompt capabilities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptCapabilities {
    pub image: bool,
    pub audio: bool,
    #[serde(rename = "embeddedContext")]
    pub embedded_context: bool,
}

/// MCP capabilities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpCapabilities {
    pub http: bool,
    pub sse: bool,
}

// ============================================================================
// Session Operations
// ============================================================================

/// Create new session parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionNewParams {
    pub cwd: String,

    #[serde(rename = "additionalDirectories")]
    #[serde(default)]
    pub additional_directories: Vec<String>,

    #[serde(rename = "mcpServers")]
    #[serde(default)]
    pub mcp_servers: Vec<McpServer>,
}

/// MCP Server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServer {
    pub name: String,
    pub command: String,
    pub args: Option<Vec<String>>,
    pub env: Option<std::collections::HashMap<String, String>>,
}

/// Session new result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionNewResult {
    #[serde(rename = "sessionId")]
    pub session_id: String,

    #[serde(rename = "agentSessionId")]
    pub agent_session_id: Option<String>,
}

/// Session prompt parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionPromptParams {
    #[serde(rename = "sessionId")]
    pub session_id: String,

    pub content: Vec<ContentBlock>,

    #[serde(rename = "systemPrompt")]
    #[serde(default)]
    pub system_prompt: Option<String>,

    #[serde(default)]
    pub mode: Option<String>,
}

/// Session prompt result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionPromptResult {
    #[serde(rename = "sessionId")]
    pub session_id: String,

    #[serde(rename = "stopReason")]
    pub stop_reason: StopReason,
}

/// Stop reason
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    EndTurn,
    Cancelled,
    Completion,
    Error,
}

/// Session cancel parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionCancelParams {
    #[serde(rename = "sessionId")]
    pub session_id: String,
}

/// Session cancel result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionCancelResult {
    #[serde(rename = "sessionId")]
    pub session_id: String,

    #[serde(rename = "stopReason")]
    pub stop_reason: StopReason,
}

/// Session load parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionLoadParams {
    #[serde(rename = "sessionId")]
    pub session_id: String,

    pub cwd: String,

    #[serde(rename = "additionalDirectories")]
    #[serde(default)]
    pub additional_directories: Vec<String>,

    #[serde(rename = "mcpServers")]
    #[serde(default)]
    pub mcp_servers: Vec<McpServer>,
}

/// Session resume parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionResumeParams {
    #[serde(rename = "sessionId")]
    pub session_id: String,

    pub cwd: String,

    #[serde(rename = "additionalDirectories")]
    #[serde(default)]
    pub additional_directories: Vec<String>,

    #[serde(rename = "mcpServers")]
    #[serde(default)]
    pub mcp_servers: Vec<McpServer>,
}

/// Session close parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionCloseParams {
    #[serde(rename = "sessionId")]
    pub session_id: String,
}

/// Session close result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionCloseResult {
    #[serde(rename = "sessionId")]
    pub session_id: String,
}

/// Session set mode parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSetModeParams {
    #[serde(rename = "sessionId")]
    pub session_id: String,

    pub mode: String,
}

/// Session set mode result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSetModeResult {
    #[serde(rename = "sessionId")]
    pub session_id: String,

    pub mode: String,
}

/// Session list result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionListResult {
    pub sessions: Vec<SessionInfo>,
}

/// Session information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    #[serde(rename = "sessionId")]
    pub session_id: String,

    pub cwd: String,

    #[serde(rename = "createdAt")]
    pub created_at: String,

    #[serde(rename = "lastUsedAt")]
    pub last_used_at: String,
}

/// Request permission parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestPermissionParams {
    #[serde(rename = "sessionId")]
    pub session_id: String,

    pub tool: String,

    #[serde(rename = "toolInput")]
    pub tool_input: JsonValue,
}

/// Request permission result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestPermissionResult {
    pub approved: bool,

    #[serde(rename = "response")]
    pub response: PermissionResponse,
}

/// Permission response type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionResponse {
    ApprovedOnce,
    ApprovedAlways,
    RejectOnce,
    RejectAlways,
    Cancel,
}

// ============================================================================
// Content Blocks
// ============================================================================

/// Content block types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    Image {
        data: String,
        #[serde(rename = "mimeType")]
        mime_type: Option<String>,
    },
    Audio {
        data: String,
        #[serde(rename = "mimeType")]
        mime_type: Option<String>,
    },
    ResourceLink {
        uri: String,
        name: Option<String>,
        description: Option<String>,
    },
}

impl ContentBlock {
    pub fn text(&self) -> Option<&str> {
        match self {
            ContentBlock::Text { text } => Some(text),
            _ => None,
        }
    }
}

// ============================================================================
// Session Update Notification
// ============================================================================

/// Session update notification parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionUpdateParams {
    #[serde(rename = "sessionId")]
    pub session_id: String,

    pub content: Vec<ContentBlock>,

    #[serde(default)]
    pub role: Option<String>,
}

/// Ping request/response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PingResult {}

/// Authenticate parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthenticateParams {
    pub method: String,

    #[serde(default)]
    pub credentials: Option<JsonValue>,
}

/// Authenticate result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthenticateResult {
    pub success: bool,

    #[serde(rename = "agentSessionId")]
    pub agent_session_id: Option<String>,
}
