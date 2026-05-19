//! ACP Error Types
//!
//! Error definitions following JSON-RPC 2.0 and ACP protocol error codes.

use std::fmt;

/// ACP Error type
#[derive(Debug, Clone)]
pub enum AcpError {
    // JSON-RPC 2.0 standard errors (-32700 to -32603)
    ParseError(String),
    InvalidRequest(String),
    MethodNotFound(String),
    InvalidParams(String),
    InternalError(String),

    // ACP protocol errors (-32500 to -32599)
    ExecutionTimeout { session_id: String, timeout_ms: u64 },
    SessionNotFound(String),
    TaskCancelled(String),
    AgentNotAvailable(String),
    PermissionDenied(String),
    SessionAlreadyExists(String),
    SessionLoadFailed(String),
    InvalidCredentials,

    // Transport errors
    TransportError(String),
    IoError(String),
    ChannelError(String),

    // Codec errors
    CodecError(String),
    UnexpectedMessage(String),
}

impl fmt::Display for AcpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AcpError::ParseError(msg) => write!(f, "Parse error: {}", msg),
            AcpError::InvalidRequest(msg) => write!(f, "Invalid request: {}", msg),
            AcpError::MethodNotFound(msg) => write!(f, "Method not found: {}", msg),
            AcpError::InvalidParams(msg) => write!(f, "Invalid params: {}", msg),
            AcpError::InternalError(msg) => write!(f, "Internal error: {}", msg),
            AcpError::ExecutionTimeout { session_id, timeout_ms } => {
                write!(f, "Execution timeout for session {} after {}ms", session_id, timeout_ms)
            }
            AcpError::SessionNotFound(id) => write!(f, "Session not found: {}", id),
            AcpError::TaskCancelled(id) => write!(f, "Task cancelled: {}", id),
            AcpError::AgentNotAvailable(name) => write!(f, "Agent not available: {}", name),
            AcpError::PermissionDenied(msg) => write!(f, "Permission denied: {}", msg),
            AcpError::SessionAlreadyExists(id) => write!(f, "Session already exists: {}", id),
            AcpError::SessionLoadFailed(id) => write!(f, "Failed to load session: {}", id),
            AcpError::InvalidCredentials => write!(f, "Invalid credentials"),
            AcpError::TransportError(msg) => write!(f, "Transport error: {}", msg),
            AcpError::IoError(msg) => write!(f, "IO error: {}", msg),
            AcpError::ChannelError(msg) => write!(f, "Channel error: {}", msg),
            AcpError::CodecError(msg) => write!(f, "Codec error: {}", msg),
            AcpError::UnexpectedMessage(msg) => write!(f, "Unexpected message: {}", msg),
        }
    }
}

impl std::error::Error for AcpError {}

impl From<std::io::Error> for AcpError {
    fn from(err: std::io::Error) -> Self {
        AcpError::IoError(err.to_string())
    }
}

impl From<serde_json::Error> for AcpError {
    fn from(err: serde_json::Error) -> Self {
        AcpError::ParseError(err.to_string())
    }
}

impl From<tokio::sync::mpsc::error::SendError<String>> for AcpError {
    fn from(err: tokio::sync::mpsc::error::SendError<String>) -> Self {
        AcpError::ChannelError(err.to_string())
    }
}

impl From<anyhow::Error> for AcpError {
    fn from(err: anyhow::Error) -> Self {
        AcpError::InternalError(err.to_string())
    }
}

/// JSON-RPC 2.0 error codes
pub const PARSE_ERROR: i32 = -32700;
pub const INVALID_REQUEST: i32 = -32600;
pub const METHOD_NOT_FOUND: i32 = -32601;
pub const INVALID_PARAMS: i32 = -32602;
pub const INTERNAL_ERROR: i32 = -32603;

/// ACP protocol error codes
pub const EXECUTION_TIMEOUT: i32 = -32500;
pub const SESSION_NOT_FOUND: i32 = -32501;
pub const TASK_CANCELLED: i32 = -32502;
pub const AGENT_NOT_AVAILABLE: i32 = -32503;
pub const PERMISSION_DENIED: i32 = -32504;
pub const SESSION_ALREADY_EXISTS: i32 = -32505;
pub const SESSION_LOAD_FAILED: i32 = -32506;
pub const INVALID_CREDENTIALS: i32 = -32507;

impl AcpError {
    /// Get the JSON-RPC error code
    pub fn code(&self) -> i32 {
        match self {
            AcpError::ParseError(_) => PARSE_ERROR,
            AcpError::InvalidRequest(_) => INVALID_REQUEST,
            AcpError::MethodNotFound(_) => METHOD_NOT_FOUND,
            AcpError::InvalidParams(_) => INVALID_PARAMS,
            AcpError::InternalError(_) => INTERNAL_ERROR,
            AcpError::ExecutionTimeout { .. } => EXECUTION_TIMEOUT,
            AcpError::SessionNotFound(_) => SESSION_NOT_FOUND,
            AcpError::TaskCancelled(_) => TASK_CANCELLED,
            AcpError::AgentNotAvailable(_) => AGENT_NOT_AVAILABLE,
            AcpError::PermissionDenied(_) => PERMISSION_DENIED,
            AcpError::SessionAlreadyExists(_) => SESSION_ALREADY_EXISTS,
            AcpError::SessionLoadFailed(_) => SESSION_LOAD_FAILED,
            AcpError::InvalidCredentials => INVALID_CREDENTIALS,
            AcpError::TransportError(_) => INTERNAL_ERROR,
            AcpError::IoError(_) => INTERNAL_ERROR,
            AcpError::ChannelError(_) => INTERNAL_ERROR,
            AcpError::CodecError(_) => INTERNAL_ERROR,
            AcpError::UnexpectedMessage(_) => INTERNAL_ERROR,
        }
    }

    /// Convert to JSON-RPC error response
    pub fn to_error_response(&self, id: Option<crate::protocol::Id>) -> crate::protocol::ErrorResponse {
        crate::protocol::ErrorResponse {
            jsonrpc: crate::protocol::JSONRPC_VERSION.to_string(),
            error: crate::protocol::Error {
                code: self.code(),
                message: self.to_string(),
                data: None,
            },
            id,
        }
    }
}
