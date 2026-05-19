# ACP (Agent Client Protocol) 协议设计方案

> 本文档基于官方 ACP (Agent Client Protocol) 规范设计，参考了 https://agentclientprotocol.com 和 https://github.com/openclaw/acpx

## 1. 项目背景

### 1.1 目标

在 Rust 项目中实现一个 **ACP (Agent Client Protocol)** 协议框架，将 Claude Code CLI 封装为标准 Coding Agent，支持：

- 智能体发现与能力注册
- 标准化的任务委托/响应格式
- 会话管理（session/new, session/prompt, session/cancel）
- 状态管理（working/completed/cancelled）
- 跨进程 stdio 通信
- 能力协商（capability negotiation）

### 1.2 参考实现

- [agentclientprotocol.com](https://agentclientprotocol.com): ACP 官方协议规范
- [openclaw/acpx](https://github.com/openclaw/acpx): TypeScript 实现的 ACP 客户端
- [agentclientprotocol/claude-agent-acp](https://github.com/agentclientprotocol/claude-agent-acp): Claude Code ACP 适配器

---

## 2. 协议设计

### 2.1 设计原则

- 基于 **JSON-RPC 2.0** 消息格式
- 采用 **stdio** 作为传输层（本地代理）
- 支持 **HTTP/WebSocket** 传输（远程代理）
- 复用 MCP 的 JSON 表示格式
- 协议版本为**单一整数**，仅破坏性变更时递增

### 2.2 消息格式

```json
{
  "jsonrpc": "2.0",
  "method": "<method_name>",
  "params": { ... },
  "id": "<unique_id>"
}
```

响应格式：

```json
{
  "jsonrpc": "2.0",
  "result": { ... },
  "id": "<unique_id>"
}
```

错误格式：

```json
{
  "jsonrpc": "2.0",
  "error": {
    "code": <error_code>,
    "message": "<error_message>",
    "data": { ... }
  },
  "id": "<unique_id>"
}
```

### 2.3 stdio 传输规范

- 客户端以子进程方式启动 ACP Agent
- Agent 从标准输入 (`stdin`) 读取 JSON-RPC 消息
- Agent 向标准输出 (`stdout`) 发送 JSON-RPC 消息
- 消息用**换行符 (`\n`)** 分隔
- 消息**不得包含嵌入式换行符**
- Agent 可向 `stderr` 写入日志（调试信息、错误等），客户端可捕获或忽略

### 2.4 核心消息类型

#### 2.4.1 初始化 (initialize)

**请求：**

```json
{
  "jsonrpc": "2.0",
  "method": "initialize",
  "params": {
    "protocolVersion": 1,
    "clientInfo": {
      "name": "acpx",
      "version": "0.1.0"
    },
    "capabilities": {
      "fs": {
        "readTextFile": true,
        "writeTextFile": true
      },
      "terminal": {
        "create": true,
        "output": true,
        "release": true,
        "waitForExit": true,
        "kill": true
      }
    }
  },
  "id": 1
}
```

**响应：**

```json
{
  "jsonrpc": "2.0",
  "result": {
    "protocolVersion": 1,
    "agentInfo": {
      "name": "claude-code-agent",
      "version": "1.0.0"
    },
    "capabilities": {
      "loadSession": true,
      "promptCapabilities": {
        "image": true,
        "audio": false,
        "embeddedContext": true
      },
      "mcpCapabilities": {
        "http": true,
        "sse": true
      }
    }
  },
  "id": 1
}
```

#### 2.4.2 认证 (authenticate)

**请求：**

```json
{
  "jsonrpc": "2.0",
  "method": "authenticate",
  "params": {
    "method": "api_key",
    "credentials": {
      "api_key": "sk-..."
    }
  },
  "id": 2
}
```

**响应：**

```json
{
  "jsonrpc": "2.0",
  "result": {
    "success": true,
    "agentSessionId": "agent-session-uuid"
  },
  "id": 2
}
```

#### 2.4.3 创建会话 (session/new)

**请求：**

```json
{
  "jsonrpc": "2.0",
  "method": "session/new",
  "params": {
    "cwd": "/path/to/workdir",
    "mcpServers": []
  },
  "id": 3
}
```

**响应：**

```json
{
  "jsonrpc": "2.0",
  "result": {
    "sessionId": "uuid-v4",
    "agentSessionId": "claude-internal-session-id"
  },
  "id": 3
}
```

#### 2.4.4 发送提示 (session/prompt)

**请求：**

```json
{
  "jsonrpc": "2.0",
  "method": "session/prompt",
  "params": {
    "sessionId": "uuid-v4",
    "content": [
      {
        "Text": "帮我写一个 Hello World 程序"
      }
    ],
    "systemPrompt": "optional system prompt",
    "mode": "auto"
  },
  "id": 4
}
```

**响应：**

```json
{
  "jsonrpc": "2.0",
  "result": {
    "sessionId": "uuid-v4",
    "stopReason": "end_turn"
  },
  "id": 4
}
```

#### 2.4.5 会话更新通知 (session/update)

Agent 在执行过程中发送通知：

```json
{
  "jsonrpc": "2.0",
  "method": "session/update",
  "params": {
    "sessionId": "uuid-v4",
    "content": [
      {
        "Text": "正在分析..."
      }
    ],
    "role": "assistant"
  }
}
```

#### 2.4.6 取消会话 (session/cancel)

**请求：**

```json
{
  "jsonrpc": "2.0",
  "method": "session/cancel",
  "params": {
    "sessionId": "uuid-v4"
  },
  "id": 5
}
```

**响应：**

```json
{
  "jsonrpc": "2.0",
  "result": {
    "sessionId": "uuid-v4",
    "stopReason": "cancelled"
  },
  "id": 5
}
```

#### 2.4.7 加载会话 (session/load)

**请求：**

```json
{
  "jsonrpc": "2.0",
  "method": "session/load",
  "params": {
    "sessionId": "uuid-v4",
    "cwd": "/path/to/workdir",
    "mcpServers": []
  },
  "id": 6
}
```

**响应：**

```json
{
  "jsonrpc": "2.0",
  "result": {
    "sessionId": "uuid-v4",
    "agentSessionId": "claude-internal-session-id"
  },
  "id": 6
}
```

加载后 Agent 会通过 `session/update` 通知重放整个对话历史。

#### 2.4.8 恢复会话 (session/resume)

**请求：**

```json
{
  "jsonrpc": "2.0",
  "method": "session/resume",
  "params": {
    "sessionId": "uuid-v4",
    "cwd": "/path/to/workdir",
    "mcpServers": []
  },
  "id": 7
}
```

与 `session/load` 的区别：不重放历史，直接恢复上下文。

#### 2.4.9 关闭会话 (session/close)

**请求：**

```json
{
  "jsonrpc": "2.0",
  "method": "session/close",
  "params": {
    "sessionId": "uuid-v4"
  },
  "id": 8
}
```

**响应：**

```json
{
  "jsonrpc": "2.0",
  "result": {
    "sessionId": "uuid-v4"
  },
  "id": 8
}
```

#### 2.4.10 设置模式 (session/set_mode)

**请求：**

```json
{
  "jsonrpc": "2.0",
  "method": "session/set_mode",
  "params": {
    "sessionId": "uuid-v4",
    "mode": "plan"
  },
  "id": 9
}
```

**响应：**

```json
{
  "jsonrpc": "2.0",
  "result": {
    "sessionId": "uuid-v4",
    "mode": "plan"
  },
  "id": 9
}
```

#### 2.4.11 列出会话 (session/list)

**请求：**

```json
{
  "jsonrpc": "2.0",
  "method": "session/list",
  "params": {},
  "id": 10
}
```

**响应：**

```json
{
  "jsonrpc": "2.0",
  "result": {
    "sessions": [
      {
        "sessionId": "uuid-v4",
        "cwd": "/path/to/workdir",
        "createdAt": "2025-11-25T10:30:00Z",
        "lastUsedAt": "2025-11-25T12:00:00Z"
      }
    ]
  },
  "id": 10
}
```

#### 2.4.12 请求权限 (session/request_permission)

**请求：**

```json
{
  "jsonrpc": "2.0",
  "method": "session/request_permission",
  "params": {
    "sessionId": "uuid-v4",
    "tool": "Edit",
    "toolInput": {
      "filePath": "/path/to/file.ts",
      "oldString": "old code",
      "newString": "new code"
    }
  },
  "id": 11
}
```

**响应：**

```json
{
  "jsonrpc": "2.0",
  "result": {
    "approved": true,
    "response": "approved_once"
  },
  "id": 11
}
```

#### 2.4.13 心跳 (ping)

**请求：**

```json
{
  "jsonrpc": "2.0",
  "method": "ping",
  "params": {},
  "id": 12
}
```

**响应：**

```json
{
  "jsonrpc": "2.0",
  "result": {},
  "id": 12
}
```

### 2.5 错误码定义

| 错误码 | 含义 | 说明 |
|--------|------|------|
| -32700 | Parse error | 无效的 JSON |
| -32600 | Invalid Request | 请求格式错误 |
| -32601 | Method not found | 方法不存在 |
| -32602 | Invalid params | 参数无效 |
| -32603 | Internal error | 内部错误 |
| -32500 | Execution timeout | 执行超时 |
| -32501 | Session not found | 会话不存在 |
| -32502 | Task cancelled | 任务被取消 |
| -32503 | Agent not available | Agent 不可用 |
| -32504 | Permission denied | 权限被拒绝 |

---

## 3. 能力协商 (Capability Negotiation)

### 3.1 客户端能力 (Client Capabilities)

| 能力 | 说明 |
|------|------|
| `fs.readTextFile` | 支持读取文本文件 |
| `fs.writeTextFile` | 支持写入文本文件 |
| `terminal.create` | 支持创建终端 |
| `terminal.output` | 支持终端输出 |
| `terminal.release` | 支持释放终端 |
| `terminal.waitForExit` | 支持等待终端退出 |
| `terminal.kill` | 支持终止终端 |

### 3.2 Agent 能力 (Agent Capabilities)

| 能力 | 说明 |
|------|------|
| `loadSession` | 支持加载会话功能 |
| `promptCapabilities.image` | 支持图像内容 |
| `promptCapabilities.audio` | 支持音频内容 |
| `promptCapabilities.embeddedContext` | 支持嵌入上下文 |
| `mcpCapabilities.http` | 支持 MCP over HTTP |
| `mcpCapabilities.sse` | 支持 MCP over SSE |

---

## 4. 会话生命周期

```
session/new
    │
    ▼
┌─────────────┐
│   active    │◄──── session/prompt
└──────┬──────┘
       │
       ├── session/cancel ──► cancelled
       │
       ├── session/close ──► closed
       │
       └── end_turn ──► idle
                           │
                           ├── session/prompt ──► active
                           │
                           └── session/close ──► closed
```

**状态说明**：
- `active`: 会话正在处理提示
- `idle`: 等待下一个提示
- `cancelled`: 会话被取消
- `closed`: 会话已关闭

---

## 5. 内容块类型 (Content Blocks)

ACP 复用 MCP 的内容块格式：

| 类型 | 说明 | 示例 |
|------|------|------|
| `Text` | 纯文本消息 | `{"Text": "Hello world"}` |
| `Image` | Base64 编码的图像 | `{"Image": {"data": "base64...", "mimeType": "image/png"}}` |
| `Audio` | Base64 编码的音频 | `{"Audio": {"data": "base64...", "mimeType": "audio/wav"}}` |
| `ResourceLink` | 资源 URI 引用 | `{"ResourceLink": {"uri": "file:///path"}}` |
| `EmbeddedResource` | 嵌入的资源内容 | `{"EmbeddedResource": {"resource": {...}}}` |

---

## 6. acpx crate 架构设计

### 6.1 目录结构

```
acpx/
├── Cargo.toml
└── src/
    ├── lib.rs              # 公共接口导出

    # 协议核心
    ├── protocol.rs         # JSON-RPC 消息定义、序列化/反序列化
    ├── codec.rs            # 消息编解码器（newline 分隔）

    # 传输层
    ├── transport.rs        # StdioTransport（stdio 读写抽象）

    # 能力系统
    ├── capabilities.rs     # 能力定义与协商

    # 会话管理
    ├── session.rs          # 会话状态机、会话存储

    # Agent 接口
    ├── agent.rs            # Agent trait 接口定义
    ├── registry.rs         # Agent 注册与管理中心

    # 具体 Agent 实现
    └── coding_agent/
        ├── mod.rs          # ClaudeCodeAgent 主模块
        ├── process.rs       # 子进程管理（spawn、wait、kill）
        ├── cli.rs           # Claude CLI 调用封装

    # 错误处理
    └── error.rs            # AcpError 错误类型
```

### 6.2 核心模块设计

#### 6.2.1 protocol.rs - 协议消息

```rust
// JSON-RPC 消息类型
pub enum Message {
    Request(Request),
    Notification(Notification),
    Response(Response),
    ErrorResponse(ErrorResponse),
}

// JSON-RPC 请求
pub struct Request {
    pub jsonrpc: String,       // "2.0"
    pub method: String,
    pub params: serde_json::Value,
    pub id: Id,
}

// 请求 ID
pub type Id = serde_json::Value;  // string | number | null

// 初始化参数
pub struct InitializeParams {
    pub protocol_version: u32,
    pub client_info: Option<ClientInfo>,
    pub capabilities: ClientCapabilities,
}

// 客户端能力
pub struct ClientCapabilities {
    pub fs: Option<FsCapabilities>,
    pub terminal: Option<TerminalCapabilities>,
}

pub struct FsCapabilities {
    pub read_text_file: bool,
    pub write_text_file: bool,
}

pub struct TerminalCapabilities {
    pub create: bool,
    pub output: bool,
    pub release: bool,
    pub wait_for_exit: bool,
    pub kill: bool,
}

// Agent 能力
pub struct AgentCapabilities {
    pub load_session: bool,
    pub prompt_capabilities: PromptCapabilities,
    pub mcp_capabilities: McpCapabilities,
}

pub struct PromptCapabilities {
    pub image: bool,
    pub audio: bool,
    pub embedded_context: bool,
}

pub struct McpCapabilities {
    pub http: bool,
    pub sse: bool,
}

// 会话参数
pub struct SessionNewParams {
    pub cwd: String,
    pub mcp_servers: Vec<McpServer>,
}

pub struct SessionPromptParams {
    pub session_id: String,
    pub content: Vec<ContentBlock>,
    pub system_prompt: Option<String>,
    pub mode: Option<String>,
}

pub struct SessionCancelParams {
    pub session_id: String,
}

pub struct SessionLoadParams {
    pub session_id: String,
    pub cwd: String,
    pub mcp_servers: Vec<McpServer>,
}

pub struct SessionResumeParams {
    pub session_id: String,
    pub cwd: String,
    pub mcp_servers: Vec<McpServer>,
}

pub struct SessionCloseParams {
    pub session_id: String,
}

pub struct SessionSetModeParams {
    pub session_id: String,
    pub mode: String,
}

// 内容块
pub enum ContentBlock {
    Text { text: String },
    Image { data: String, mime_type: String },
    Audio { data: String, mime_type: String },
    ResourceLink { uri: String },
}

// 会话状态
pub enum SessionState {
    Active,
    Idle,
    Cancelled,
    Closed,
}

// 停止原因
pub enum StopReason {
    EndTurn,
    Cancelled,
    Completion,
    Error,
}
```

#### 6.2.2 transport.rs - 传输层

```rust
pub trait Transport: Send + Sync {
    async fn send(&mut self, msg: &Message) -> Result<()>;
    async fn recv(&mut self) -> Result<Message>;
}

pub struct StdioTransport {
    stdin: Mutex<ChildStdin>,
    stdout: Mutex<BufReader<ChildStdout>>,
    stderr: Mutex<ChildStderr>,
}

impl StdioTransport {
    pub fn new() -> Self { ... }
}

impl Transport for StdioTransport { ... }
```

#### 6.2.3 session.rs - 会话管理

```rust
pub struct Session {
    pub id: String,
    pub agent_session_id: Option<String>,
    pub cwd: String,
    pub state: SessionState,
    pub created_at: DateTime<Utc>,
    pub last_used_at: DateTime<Utc>,
}

pub struct SessionManager {
    sessions: RwLock<HashMap<String, Arc<Session>>>,
    cwd_sessions: RwLock<HashMap<String, String>>,  // cwd -> session_id
}

impl SessionManager {
    pub fn create_session(&self, cwd: &str) -> Result<Arc<Session>>;
    pub fn get_session(&self, id: &str) -> Option<Arc<Session>>;
    pub fn find_session_by_cwd(&self, cwd: &str) -> Option<Arc<Session>>;
    pub fn list_sessions(&self) -> Vec<Arc<Session>>;
    pub fn close_session(&self, id: &str) -> Result<()>;
}
```

#### 6.2.4 agent.rs - Agent 接口

```rust
#[async_trait]
pub trait Agent: Send + Sync {
    fn agent_type(&self) -> &str;
    fn agent_name(&self) -> &str;
    fn get_capabilities(&self) -> AgentCapabilities;

    // 会话操作
    async fn session_new(&self, params: SessionNewParams) -> Result<SessionNewResult>;
    async fn session_prompt(&self, params: SessionPromptParams) -> Result<SessionPromptResult>;
    async fn session_cancel(&self, params: SessionCancelParams) -> Result<SessionCancelResult>;
    async fn session_load(&self, params: SessionLoadParams) -> Result<SessionLoadResult>;
    async fn session_resume(&self, params: SessionResumeParams) -> Result<SessionResumeResult>;
    async fn session_close(&self, params: SessionCloseParams) -> Result<SessionCloseResult>;
    async fn session_set_mode(&self, params: SessionSetModeParams) -> Result<SessionSetModeResult>;
    async fn session_list(&self) -> Result<SessionListResult>;

    // 权限请求（由 Client 实现）
    async fn request_permission(&self, params: RequestPermissionParams) -> Result<RequestPermissionResult>;
}
```

#### 6.2.5 registry.rs - Agent 注册中心

```rust
pub struct Registry {
    agents: HashMap<String, Arc<dyn Agent>>,
    session_manager: Arc<SessionManager>,
}

impl Registry {
    pub fn new() -> Self { ... }

    // 注册 Agent
    pub fn register(&mut self, name: String, agent: Arc<dyn Agent>) { ... }

    // 获取 Agent
    pub fn get(&self, name: &str) -> Option<Arc<dyn Agent>> { ... }

    // 列出所有 Agent
    pub fn list_agents(&self) -> Vec<(String, String)> { ... }

    // 会话操作代理
    pub async fn session_new(&self, agent_name: &str, params: SessionNewParams) -> Result<SessionNewResult>;
    pub async fn session_prompt(&self, agent_name: &str, params: SessionPromptParams) -> Result<SessionPromptResult>;
    pub async fn session_cancel(&self, agent_name: &str, params: SessionCancelParams) -> Result<SessionCancelResult>;
}
```

#### 6.2.6 coding_agent/ - Claude Code Agent 实现

```rust
pub struct ClaudeCodeAgent {
    cli_path: PathBuf,
    session_manager: Arc<SessionManager>,
}

impl ClaudeCodeAgent {
    pub fn new() -> Self { ... }

    // 检查 Claude CLI 是否可用
    pub fn check_installation() -> Option<PathBuf> { ... }

    // 执行单条指令
    fn execute_instruction(&self, instruction: &str, session_id: Option<&str>, work_dir: &Path) -> Result<String> { ... }
}

#[async_trait]
impl Agent for ClaudeCodeAgent {
    fn agent_type(&self) -> &str { "claude_code" }
    fn agent_name(&self) -> &str { "Claude Code" }
    fn get_capabilities(&self) -> AgentCapabilities { ... }

    async fn session_new(&self, params: SessionNewParams) -> Result<SessionNewResult> { ... }
    async fn session_prompt(&self, params: SessionPromptParams) -> Result<SessionPromptResult> { ... }
    async fn session_cancel(&self, params: SessionCancelParams) -> Result<SessionCancelResult> { ... }
    async fn session_load(&self, params: SessionLoadParams) -> Result<SessionLoadResult> { ... }
    async fn session_resume(&self, params: SessionResumeParams) -> Result<SessionResumeResult> { ... }
    async fn session_close(&self, params: SessionCloseParams) -> Result<SessionCloseResult> { ... }
    async fn session_set_mode(&self, params: SessionSetModeParams) -> Result<SessionSetModeResult> { ... }
    async fn session_list(&self) -> Result<SessionListResult> { ... }
}
```

### 6.3 ClaudeCodeAgent 参数映射

| ACP 参数 | Claude CLI 参数 | 说明 |
|----------|----------------|------|
| `cwd` | `current_dir` | 工作目录 |
| `sessionId` | `--resume <session>` | 恢复会话 |
| `mode` | `--permission-mode` | 权限模式 (auto/plan) |
| `content[].Text` | `-p <prompt>` | 提示内容 |

---

## 7. 错误处理设计

### 7.1 AcpError 类型

```rust
pub enum AcpError {
    // 协议层错误
    ParseError(String),           // -32700
    InvalidRequest(String),       // -32600
    MethodNotFound(String),       // -32601
    InvalidParams(String),        // -32602
    InternalError(String),        // -32603

    // 会话错误
    ExecutionTimeout { session_id: String, timeout_ms: u64 },  // -32500
    SessionNotFound(String),      // -32501
    TaskCancelled(String),        // -32502
    AgentNotAvailable(String),    // -32503
    PermissionDenied(String),    // -32504

    // 传输层错误
    TransportError(String),
    IoError(std::io::Error),
}
```

### 7.2 错误响应生成

```rust
impl From<AcpError> for ErrorResponse {
    fn from(err: AcpError) -> Self {
        let (code, message, data) = match err {
            AcpError::ParseError(msg) => (-32700, msg, None),
            AcpError::InvalidRequest(msg) => (-32600, msg, None),
            // ...
        };
        ErrorResponse { jsonrpc: "2.0", error: Error { code, message, data }, id: None }
    }
}
```

---

## 8. 使用示例

### 8.1 启动 Claude Code Agent（stdio 模式）

```bash
# 启动 Claude Code Agent 作为子进程
cargo run --bin acpx -- --protocol acp
```

### 8.2 通过 Registry 调用

```rust
use acpx::{Registry, ClaudeCodeAgent, SessionNewParams, SessionPromptParams};

#[tokio::main]
async fn main() -> Result<()> {
    let mut registry = Registry::new();
    registry.register("claude", Arc::new(ClaudeCodeAgent::new()));

    // 创建会话
    let new_params = SessionNewParams {
        cwd: "/tmp".into(),
        mcp_servers: vec![],
    };
    let new_result = registry.session_new("claude", new_params).await?;
    println!("Session created: {}", new_result.session_id);

    // 发送提示
    let prompt_params = SessionPromptParams {
        session_id: new_result.session_id.clone(),
        content: vec![ContentBlock::Text { text: "帮我写一个 Hello World".into() }],
        system_prompt: None,
        mode: Some("auto".into()),
    };
    let prompt_result = registry.session_prompt("claude", prompt_params).await?;
    println!("Stop reason: {:?}", prompt_result.stop_reason);

    Ok(())
}
```

---

## 9. 协议版本与兼容性

### 9.1 版本协商

客户端在 `initialize` 请求中指定支持的协议版本：

```json
{
  "protocolVersion": 1
}
```

Agent 响应实际使用的版本。如果客户端不支持 Agent 返回的版本，应关闭连接。

### 9.2 版本历史

| 版本 | 日期 | 说明 |
|------|------|------|
| 1 | 2025-xx-xx | 初始版本 |

---

## 10. 安全考虑

### 10.1 信任模型

1. **用户授权**：工具调用必须获得用户明确授权
2. **数据隐私**：不得在未经用户同意情况下传输用户数据
3. **路径验证**：文件路径必须为绝对路径

### 10.2 实现建议

1. 客户端应验证所有工具输入
2. 敏感操作应显示确认对话框
3. 工具输出应进行验证后再传递给 LLM
4. 实现工具调用超时机制
5. 记录工具使用日志用于审计

---

## 11. 参考资料

- [ACP Protocol Specification](https://agentclientprotocol.com) - 官方 ACP 协议规范
- [openclaw/acpx](https://github.com/openclaw/acpx) - ACP 客户端参考实现
- [agentclientprotocol/claude-agent-acp](https://github.com/agentclientprotocol/claude-agent-acp) - Claude Code ACP 适配器

---

## 12. 修订记录

| 日期 | 版本 | 修订内容 | 负责人 |
|------|------|----------|--------|
| 2026-05-18 | v0.1 | 初始版本，基于 MCP 规范的初稿设计 | - |
| 2026-05-18 | v0.2 | 对齐官方 ACP 规范，重写方法体系、能力协商、消息格式 | - |