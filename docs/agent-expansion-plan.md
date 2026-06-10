# ONE Agent 架构重构与能力扩展方案

## 一、现状分析

### 当前架构

```
User Input → route_message()
    ├─ (如果 Orchestrator 在等待输入 → 发送到通道，不创建新 Orchestrator)
    │
    ├─ IntentRouter.quick_route() → 关键词池为空，永远返回 None，跳过
    │
    └─ spawn_orchestrator_run()  ← 所有消息都走这里
         ├─ 注入记忆 (L2 profile + L3 context)
         ├─ MainAgent.step_stream()
         │    ├─ System Prompt (硬编码，包含 skill 描述但不会动态更新)
         │    └─ Tools: [RunSystemTaskTool, RememberTool, RecallTool, ProposeSoulUpdateTool]
         ├─ AgentResponse::ToolCalls → Orchestrator.dispatch_tool()
         │    ├─ "run_system_task" → SkillRegistry (静态硬编码 6 个 Rust Skill)
         │    ├─ "run_claude_code" → 已废弃，返回提示信息
         │    └─ other → MainAgent.tools (4 个内置工具)
         └─ AgentResponse::Answer → 返回 / 等待用户输入 (多轮)
```

### 现有问题清单

| 问题 | 严重程度 | 描述 |
|------|---------|------|
| SkillRegistry 静态硬编码 | **P0** | 6 个 Skill 是 Rust struct 硬编码在 `skills/mod.rs`，用户上传的 `.skill` 文件不会被执行 |
| Agent 工具集静态生成 | **P0** | `MainAgent` 的 tools 和 system prompt 在构造时确定，无法动态感知新安装的 skill 或 MCP tool |
| Agent 无法动态扩展工具 | **P0** | 没有抽象 Agent 基类，未来新增 Agent 类型需要重复实现 tools/skills/soul 的绑定逻辑 |
| MCP 完全未接入 | **P1** | `.mcp.json` 存在但不被任何代码读取 |
| IntentRouter 关键词池为空 | **P2** | 意图路由退化：`system_keywords` 和 `coding_keywords` 为空数组，`quick_route()` 永远返回 `None`，所有消息直接走 `spawn_orchestrator_run()` |
| Skill Market UI 不可用 | **P0** | 上传按钮修复了 RefCell 崩溃，但上传的文件不会被 SkillRegistry 加载和执行 |
| `run_system_task` 拦截耦合 | **P2** | Orchestrator 必须硬编码知晓哪些 tool 名需要拦截，扩展时容易遗漏 |
| 启动时 `block_on(Backend::detect())` | **P2** | 阻塞 UI 启动 |
| API 流式调用超时 60s | **P2** | 复杂任务可能超时 |

### 架构设计原则

```
1. 面向接口编程 — Agent 是抽象 trait，不是具体 struct
2. 组合优于继承 — Tool/Skill/MCP 通过注册表组合到 Agent
3. 开闭原则 — 新增 Agent 类型不改动框架代码
4. 单一职责 — Agent (认知/决策) ≠ Tool/Skill 执行器 (执行)
5. 业界标准 — 对齐 MCP 协议、OpenAI tool calling 规范
```

---

## 二、目标架构

```
ONE (Rust/GPUI) 直接管理 MCP Server 子进程

┌────────────────────────────────────────────────────────────────┐
│                         ONE (Rust/GPUI)                         │
│                                                                  │
│  ┌─ McpClientManager ─────────────────────────────────────────┐ │
│  │  ┌──────────────┐  ┌──────────────┐  ┌──────────────────┐  │ │
│  │  │ claude-code  │  │ filesystem   │  │ 其他 MCP Server  │  │ │
│  │  │ MCP Client   │  │ MCP Client   │  │ MCP Client       │  │ │
│  │  │ (stdio)      │  │ (stdio)      │  │ (stdio/HTTP)     │  │ │
│  │  └──────┬───────┘  └──────┬───────┘  └────────┬─────────┘  │ │
│  └─────────┼─────────────────┼────────────────────┼────────────┘ │
│            │                 │                    │               │
│            ▼                 ▼                    ▼               │
│     python3 claude-   npx @modelcontext-   HTTP MCP Server      │
│     code_mcp_server   protocol/server-     (远程 API)            │
│     .py               filesystem                                 │
│                                                                  │
│  User Input → route_message()                                    │
│       │                                                          │
│       ▼                                                          │
│  Orchestrator (编排循环, 最多 15 步)                                │
│       │                                                          │
│       ├─ 注入记忆 (L2 profile + L3 context)                       │
│       │                                                          │
│       ├─ Agent.step_stream()                                     │
│       │    ├─ Dynamic System Prompt (包含所有可用工具描述)          │
│       │    └─ ToolList (来自 ToolRegistry 统一注册表):              │
│       │         ├─ BuiltinTools  (remember/recall)                │
│       │         ├─ SkillTools    (SkillRegistry → Tool)           │
│       │         └─ McpTools      (McpClientManager → Tool)        │
│       │                                                          │
│       └─ dispatch_tool() → ToolRegistry.execute()                 │
│            ├─ BuiltinTool → 直接执行                               │
│            ├─ SkillTool   → SkillRegistry.find() + execute()      │
│            └─ McpTool     → McpClientManager.call_tool()          │
│                                                                  │
└──────────────────────────────────────────────────────────────────┘

---

## 三、详细设计

### 3.1 Agent 抽象层（核心重构）

#### 现状问题

当前 `Agent` trait 定义在 `src/agents/core/mod.rs`，但有几个设计缺陷：

1. `Agent` trait 的 `tools()` 返回 `Vec<Arc<dyn Tool>>`，但工具列表在构造时固定，无法动态增减
2. `BaseAgent` 同时包含 `call_llm` 和 `step_with_tools`，职责不清晰
3. `Orchestrator` 与 `MainAgent` 强耦合，`dispatch_tool` 直接检查 `call.name.as_str() == "run_system_task"`

#### 改造设计

```rust
// ===== src/agents/core/agent.rs (Agent trait 重构) =====

/// 工具来源：一个 Tool 可以来自不同的后端
#[derive(Clone)]
pub enum ToolSource {
    /// 内置 Rust 工具（remember/recall 等）
    Builtin(Arc<dyn Tool>),
    /// 注册的 Skill（来自 SkillRegistry）
    Skill(String),  // skill_id
    /// 通过 MCP Gateway 注册的外部工具
    Mcp { server: String, tool_name: String },
}

/// Agent 运行时的完整上下文
pub struct AgentRunContext {
    pub session_id: String,
    pub history: Vec<ChatMessage>,
    pub metadata: HashMap<String, String>,
    /// 可用的工具来源（由注册表动态生成，每个 Agent 可以不同）
    pub tool_sources: Vec<ToolSource>,
    /// 取消标志
    pub cancel_flag: Option<Arc<AtomicBool>>,
    /// 用户输入通道（支持多轮交互）
    pub user_input_rx: Option<mpsc::UnboundedReceiver<String>>,
}

/// 统一的工具路由结果
pub enum ToolResult {
    /// 内置工具直接返回
    Builtin(Value),
    /// Skill 执行结果
    Skill(SkillExecution),
    /// MCP 调用结果
    Mcp(Value),
}

/// Agent trait（抽象基类）
#[async_trait]
pub trait Agent: Send + Sync {
    /// Agent 唯一标识
    fn id(&self) -> &str;
    /// 显示名称
    fn name(&self) -> &str;
    /// Agent 的灵魂/人格设定
    fn soul_prompt(&self) -> &str;
    /// 获取该 Agent 专属的工具来源过滤条件
    /// 例如：CodingAgent 可能排除某些系统工具
    fn tool_filter(&self) -> Option<Vec<String>> { None }

    /// 生成最终的 system prompt（框架自动拼接 soul + 可用工具描述 + 记忆上下文）
    fn build_system_prompt(&self, tool_descriptions: &str) -> String {
        format!("{}\n\n## 可用工具\n\n{}", self.soul_prompt(), tool_descriptions)
    }
}

// ===== src/agents/core/agent_factory.rs (工厂模式重构) =====

/// Agent 注册表：管理所有 Agent 类型的注册和实例化
pub struct AgentRegistry {
    builders: HashMap<String, Box<dyn AgentBuilder>>,
}

#[async_trait]
pub trait AgentBuilder: Send + Sync {
    fn agent_id(&self) -> &str;
    fn agent_name(&self) -> &str;
    fn build(&self, config: &Config, workspace: &str) -> Box<dyn Agent>;
}

impl AgentRegistry {
    pub fn register(builder: Box<dyn AgentBuilder>) { ... }
    pub fn create(&self, id: &str, config: &Config, workspace: &str) -> Option<Box<dyn Agent>> { ... }
    pub fn all_agents(&self) -> Vec<AgentDescriptor> { ... }
}

// ===== 具体 Agent 实现示例 =====

// MainAgent（默认通用助手）
pub struct MainAgent {
    soul: String,
}

impl AgentBuilder for MainAgentBuilder {
    fn build(&self, _config: &Config, _workspace: &str) -> Box<dyn Agent> {
        Box::new(MainAgent {
            soul: load_soul_or_default(),
        })
    }
}

impl Agent for MainAgent {
    fn id(&self) -> &str { "main" }
    fn name(&self) -> &str { "Main Agent" }
    fn soul_prompt(&self) -> &str { &self.soul }
}

// CodingAgent（后续扩展）
pub struct CodingAgent { ... }
impl Agent for CodingAgent { ... }
```

#### 工具注册表（全局、动态）

```rust
// ===== src/agents/core/tool_registry.rs (新增) =====

/// 全局工具注册表。所有工具（Builtin + Skill + MCP）统一注册到此。
/// Orchestrator 在构造 AgentRunContext 时从此拉取 tool_sources。
pub struct ToolRegistry {
    builtin_tools: Vec<Arc<dyn Tool>>,
    skill_tools: Vec<SkillToolRegistration>,  // 动态刷新
    mcp_tools: Vec<McpToolRegistration>,      // 动态刷新
}

impl ToolRegistry {
    /// 获取 Agent 可用的所有工具描述（用于 LLM tool calling）
    pub fn tool_definitions(&self, agent_filter: Option<&[String]>) -> Vec<ToolDefinition> { ... }

    /// 执行工具调用（统一路由）
    pub async fn execute(&self, call: &ToolCall) -> Result<ToolResult> {
        match self.resolve(call) {
            ToolSource::Builtin(t) => ...
            ToolSource::Skill(id) => ...
            ToolSource::Mcp { server, tool } => ...
        }
    }
}

/// 全局单例
static TOOL_REGISTRY: OnceLock<ToolRegistry> = OnceLock::new();
pub fn tool_registry() -> &'static ToolRegistry { ... }
```

---

### 3.2 Orchestrator 重构

#### 现状

当前 `Orchestrator` 持有 `Arc<dyn Agent>`，硬编码了 `dispatch_tool` 中的 `"run_system_task"` 拦截逻辑。

#### 改造后

```rust
pub struct Orchestrator {
    agent: Box<dyn Agent>,
}

impl Orchestrator {
    pub async fn run_task(
        &self,
        task: &str,
        context: &mut AgentRunContext,  // 注入，包含 tool_sources、cancel_flag 等
        on_event: impl FnMut(OrchestratorEvent),
    ) -> Result<String> {
        // 1. 构建 system prompt（Agent soul + 工具描述 + 记忆上下文）
        let system_prompt = self.agent.build_system_prompt(&tool_descriptions);

        // 2. 进入工具调用循环（最多 15 步）
        loop {
            // 调用 LLM → 得 ToolCalls 或 Answer
            let response = self.call_llm_with_tools(&system_prompt, context).await?;

            match response {
                AgentResponse::Answer(text) => {
                    // 返回 / 等待用户输入
                }
                AgentResponse::ToolCalls(calls, _thinking) => {
                    // 统一路由 → tool_registry().execute()
                    for call in calls {
                        let result = tool_registry().execute(&call).await;
                        context.add_tool_result(call.id, result);
                    }
                }
            }
        }
    }
}
```

关键变更：
- `Orchestrator` **不再硬编码工具名**，所有工具通过 `ToolRegistry` 统一路由
- `ToolRegistry` 由 `tool_sources` 注入，`Orchestrator` 不需要知道工具来自哪里
- `Agent` 只关心自己的灵魂/人格，不关心工具的具体实现

---

### 3.3 MCP 客户端（ONE 内置）

#### 目标架构

```
ONE (Rust)
  └─ src/mcp/
       ├─ mod.rs          → McpClientManager（管理所有 MCP Server 的生命周期）
       ├─ protocol.rs     → JSON-RPC 2.0 协议编解码
       ├─ transport.rs    → stdio / HTTP 两种传输实现
       └─ config.rs       → .mcp.json 解析

         │ 启动子进程 / HTTP 连接
         ▼
  ┌──────────────┐  ┌──────────────────┐  ┌──────────────────┐
  │ claude-code  │  │ filesystem       │  │ 其他 MCP Server  │
  │ MCP Server   │  │ MCP Server       │  │                  │
  │ (Python)     │  │ (Node.js)        │  │ (任何语言)        │
  └──────────────┘  └──────────────────┘  └──────────────────┘
```

#### MCP 传输层

```rust
// ===== src/mcp/transport.rs =====

/// MCP 传输方式
pub enum McpTransport {
    /// stdio 子进程通信
    Stdio {
        child: Child,
        stdin: BufWriter<ChildStdin>,
        stdout: BufReader<ChildStdout>,
    },
    /// HTTP/SSE 通信
    Http {
        client: reqwest::Client,
        base_url: String,
        headers: HashMap<String, String>,
    },
}

impl McpTransport {
    /// 发送 JSON-RPC 请求，等待响应
    pub async fn request(&mut self, req: JsonRpcRequest) -> Result<JsonRpcResponse>;
    /// 关闭连接
    pub fn shutdown(&mut self);
}
```

#### MCP Server 配置

从 `.mcp.json` 读取，支持 `stdio` 和 `http` 两种传输：

```json
{
  "mcpServers": {
    "claude-code": {
      "transport": "stdio",
      "command": "python3",
      "args": ["./scripts/claude_code_mcp_server.py"],
      "env": {
        "ANTHROPIC_API_KEY": "${ANTHROPIC_API_KEY}"
      }
    },
    "filesystem": {
      "transport": "stdio",
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
    },
    "remote-api": {
      "transport": "http",
      "url": "https://api.example.com/mcp",
      "headers": { "Authorization": "Bearer xxx" }
    }
  }
}
```

#### ONE 侧接口

```rust
// ===== src/mcp/mod.rs =====

/// MCP 客户端管理器（ONE 内置）
pub struct McpClientManager {
    /// 所有已连接的 MCP 客户端
    clients: HashMap<String, McpClientHandle>,
}

/// 单个 MCP Server 的连接句柄
struct McpClientHandle {
    config: McpServerConfig,
    transport: McpTransport,
    /// 已发现的工具列表（缓存）
    tools: Vec<McpToolDefinition>,
}

#[derive(Clone)]
pub struct McpToolDefinition {
    pub server_name: String,     // 来源 server，如 "claude-code"
    pub tool_name: String,       // 工具名，如 "run_task"
    pub description: String,
    pub input_schema: Value,
}

impl McpClientManager {
    /// 从 .mcp.json 加载配置，连接到所有 MCP Server
    pub async fn load_and_connect(config_path: &Path) -> Result<Self>;

    /// 向所有已连接的 Server 发现工具
    pub async fn discover_all_tools(&mut self) -> Result<Vec<McpToolDefinition>>;

    /// 调用指定 Server 的工具
    pub async fn call_tool(&self, server: &str, tool: &str, args: Value) -> Result<Value>;

    /// 关闭所有连接
    pub async fn shutdown_all(&mut self);
}
```

#### MCP 协议层

```rust
// ===== src/mcp/protocol.rs =====

/// JSON-RPC 2.0 请求
#[derive(Serialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,  // "2.0"
    pub id: u64,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

/// JSON-RPC 2.0 响应
#[derive(Deserialize)]
pub struct JsonRpcResponse {
    pub id: u64,
    pub result: Option<Value>,
    pub error: Option<JsonRpcError>,
}

// MCP 标准方法
// tools/list — 发现工具
// tools/call — 调用工具
// notifications/initialized — 初始化完成通知
```

---

### 3.4 Skill 系统重构

#### 现状

```rust
// skills/mod.rs — 6 个 hardcoded Rust struct
static REGISTRY: OnceLock<SkillRegistry> = OnceLock::new();
```

#### 改造设计

```rust
// ===== src/skills/mod.rs (重构) =====

// Skill trait 保持不变（向后兼容现有 6 个 Rust Skill）
#[async_trait]
pub trait Skill: Send + Sync {
    fn manifest(&self) -> SkillManifest;
    async fn preview(&self, args: Value) -> Result<SkillPreview>;
    async fn execute(&self, args: Value, source: Option<&str>) -> Result<SkillExecution>;
}

// 新增：动态 Skill 类型
pub enum AnySkill {
    /// Rust 内置 Skill
    Builtin(Box<dyn Skill>),
    /// 从 .skill 文件加载的动态 Skill
    Dynamic(DynamicSkill),
}

// 新增：动态 Skill 加载器
pub struct DynamicSkill {
    manifest: SkillManifest,
    /// SKILL.md 中的执行器定义
    executor: SkillExecutor,
}

pub enum SkillExecutor {
    /// 通过 MCP 调用
    McpTool { server: String, tool: String },
    /// 直接终端执行命令
    Command { command: String, args: Vec<String> },
    /// 本地 Rust 函数（预留）
    Local { fn_name: String },
}

impl DynamicSkill {
    /// 从 ~/.one/skills/<name>/ 目录加载
    pub fn load_from_dir(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path.join("SKILL.md"))?;
        let (frontmatter, body) = parse_frontmatter(&content)?;
        // frontmatter 中的 manifest
        // body 中的执行逻辑描述（给 LLM 看）
        Ok(Self { manifest, executor })
    }
}

// SkillRegistry 增加动态扫描
impl SkillRegistry {
    /// 刷新动态 Skill（扫描 ~/.one/skills/）
    pub fn refresh_dynamic(&mut self) -> Result<()> {
        self.dynamic.clear();
        let dir = Self::skills_root_dir();
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            if path.is_dir() && path.join("SKILL.md").exists() {
                self.dynamic.push(DynamicSkill::load_from_dir(&path)?);
            }
        }
        Ok(())
    }

    /// 获取所有工具定义（用于注入 ToolRegistry）
    pub fn skill_tool_definitions(&self) -> Vec<ToolDefinition> {
        // 每个 Skill 转换为一个 ToolDefinition
        // 通过 "skill:<id>" 命名空间标识
    }
}
```

#### .skill / .zip 文件规范

```
~/.one/skills/<skill-name>/
├── SKILL.md              # 主描述文件
├── references/           # 参考文档（可选）
├── templates/            # 模板（可选）
└── assets/              # 资源文件（可选）

.skill 文件格式 = .zip 包，解压后结构同上
```

**SKILL.md 规范：**

```markdown
---
name: claude-code
description: "调用 Claude Code 执行编码任务"
version: 1.0.0
author: ONE
platforms: [macos, linux]
danger_level: Normal
category: Development
executor:
  type: mcp_tool
  server: claude-code
  tool: run_task
---

# Claude Code Skill

此 Skill 通过 MCP 协议调用 Claude Code CLI 执行编码任务。

## 参数说明

- `task` (string, required): 要执行的编码任务描述
- `work_dir` (string, optional): 工作目录
- `max_turns` (integer, optional): 最大执行步数，默认 15

## 使用说明

当用户需要编写代码、修复 bug、代码审查时，调用此 Skill。
```

---

### 3.5 配置文件规范

#### `.mcp.json`（新增字段）

```json
{
  "mcpServers": {
    "claude-code": {
      "transport": "stdio",
      "command": "python3",
      "args": ["./scripts/claude_code_mcp_server.py"],
      "env": {
        "ANTHROPIC_API_KEY": "${ANTHROPIC_API_KEY}"
      }
    },
    "filesystem": {
      "transport": "stdio",
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
    }
  }
}
```

#### Skill 市场数据源（可选，用于动态加载 Market 卡片）

```
~/.one/skills/index.json  // 本地已安装的 skill 索引
```

---

### 3.6 Agent/Skill/Tool 工具定义到 LLM 的标准化

当 Agent 调用 LLM 时，需要将所有可用工具（Builtin + Skill + MCP）格式化为 OpenAI 兼容的 tool definitions：

```rust
fn build_tool_definitions(agent: &dyn Agent, registry: &ToolRegistry) -> Vec<Value> {
    let mut defs = Vec::new();

    // 1. 内置工具（所有 Agent 共享）
    for tool in &registry.builtin_tools {
        defs.push(json!({
            "type": "function",
            "function": {
                "name": tool.name(),
                "description": tool.description(),
                "parameters": tool.parameters_schema()
            }
        }));
    }

    // 2. Skill 工具（动态注册）
    for skill in registry.all_skills() {
        if let Some(filter) = agent.tool_filter() {
            if !filter.contains(&skill.manifest().id) { continue; }
        }
        defs.push(json!({
            "type": "function",
            "function": {
                "name": format!("skill:{}", skill.manifest().id),
                "description": skill.manifest().description,
                "parameters": { ... }
            }
        }));
    }

    // 3. MCP 工具（通过网关注册）
    for mcp in registry.all_mcp_tools() {
        defs.push(json!({
            "type": "function",
            "function": {
                "name": format!("mcp:{}:{}", mcp.server_name, mcp.tool_name),
                "description": mcp.description,
                "parameters": mcp.input_schema
            }
        }));
    }

    defs
}
```

命名空间约定（避免工具名冲突）：

| 前缀 | 示例 | 来源 |
|------|------|------|
| (无前缀) | `remember` | Builtin tool |
| `skill:` | `skill:system.tools` | SkillRegistry |
| `mcp:` | `mcp:claude-code:run_task` | McpClientManager |

---

## 四、实施路线图

### Phase 1：Agent 抽象层（2-3 天）

| 步骤 | 文件 | 内容 |
|------|------|------|
| 1.1 | `src/agents/core/agent.rs` | 定义 `Agent` trait（soul_prompt, tool_filter） |
| 1.2 | `src/agents/core/tool_registry.rs` | 定义 `ToolRegistry`、`ToolSource`、`ToolResult` |
| 1.3 | `src/agents/core/agent_factory.rs` | `AgentRegistry` + `AgentBuilder` trait |
| 1.4 | `src/agents/core/mod.rs` | 导出新类型，保留现有 `ToolCall` 等 |
| 1.5 | `src/agents/core/main_agent.rs` | 重构为 `MainAgentBuilder` + `MainAgent` |
| 1.6 | `src/agents/core/orchestrator.rs` | 重构：去掉硬编码拦截，统一走 ToolRegistry |
| 1.7 | 编译验证 | `cargo build` |

### Phase 2：MCP 客户端（1-2 天）

| 步骤 | 文件 | 内容 |
| 2.1 | `src/mcp/mod.rs` | McpClientManager（连接/发现/调用/关闭） |
| 2.2 | `src/mcp/protocol.rs` | JSON-RPC 2.0 协议编解码 |
| 2.3 | `src/mcp/transport.rs` | stdio 和 HTTP 两种传输实现 |
| 2.4 | `src/mcp/config.rs` | .mcp.json 解析 |
| 2.5 | `src/app_state.rs` | McpClientManager 集成 |
| 2.6 | `src/main.rs` | 启动时初始化 MCP 连接 |
| 2.7 | 注册到 ToolRegistry | tools/list → 注册到全局注册表 |

### Phase 3：动态 Skill 系统（1-2 天）

| 步骤 | 文件 | 内容 |
|------|------|------|
| 3.1 | `src/skills/dynamic_skill.rs` | DynamicSkill 加载器（SKILL.md 解析） |
| 3.2 | `src/skills/mod.rs` | SkillRegistry 扩展（动态扫描 + 刷新） |
| 3.3 | `src/skills/mod.rs` | Skill → Tool 适配器（注册到 ToolRegistry） |
| 3.4 | `src/skills_market.rs` | 上传后触发 registry refresh |

### Phase 4：系统集成（1 天）

| 步骤 | 文件 | 内容 |
|------|------|------|
| 4.1 | `src/routing.rs` | 使用新 AgentFactory + ToolRegistry |
| 4.2 | `src/job_manager.rs` | 适配新的 Orchestrator 接口 |
| 4.3 | `src/agents/intent_router.rs` | IntentRouter 实装关键词 |
| 4.4 | 端到端测试 | 安装 skill → 调用 → 执行 → 返回 |

### Phase 5：Claude Code MCP Server + Skill（0.5 天）

| 步骤 | 文件 | 内容 |
|------|------|------|
| 5.1 | `scripts/claude_code_mcp_server.py` | Claude Code MCP Server（Python 脚本，ONE 直接启动作为子进程） |
| 5.2 | `.mcp.json` | 配置 claude-code server |
| 5.3 | `~/.one/skills/claude-code/SKILL.md` | Claude Code skill 文件 |

---

## 五、文件变更清单

### 新增文件

| 文件 | 说明 |
|------|------|
| `src/agents/core/agent.rs` | Agent trait 定义 |
| `src/agents/core/tool_registry.rs` | 全局工具注册表 |
| `src/agents/core/agent_factory.rs` | AgentBuilder 工厂 |
| `src/mcp/mod.rs` | MCP 客户端管理器（ONE 内置） |
| `src/mcp/protocol.rs` | JSON-RPC 2.0 协议编解码 |
| `src/mcp/transport.rs` | stdio / HTTP 两种传输 |
| `src/mcp/config.rs` | .mcp.json 解析 |
| `src/skills/dynamic_skill.rs` | 动态 Skill 加载器 |
| `scripts/claude_code_mcp_server.py` | Claude Code MCP Server |

### 修改文件

| 文件 | 修改内容 |
|------|---------|
| `src/agents/core/mod.rs` | 导出 Agent trait, ToolRegistry, AgentFactory |
| `src/agents/core/main_agent.rs` | 重构为实现 Agent trait |
| `src/agents/core/orchestrator.rs` | 去掉硬编码路由，走 ToolRegistry |
| `src/agents/core/factory.rs` | 重构为 AgentRegistry + AgentBuilder |
| `src/skills/mod.rs` | SkillRegistry 增加动态扫描和 Tool 适配 |
| `src/skills_market.rs` | 上传后触发 registry refresh |
| `src/app_state.rs` | 增加 mcp_manager 字段 |
| `src/runtime/job_manager.rs` | 适配新 Orchestrator 接口 |
| `src/routing.rs` | 使用新 AgentFactory |
| `src/main.rs` | 启动时初始化 MCP 连接 |
| `Cargo.toml` | 新增依赖（如有必要） |

---

## 六、关键设计决策记录

| 决策 | 选择 | 理由 |
|------|------|------|
| MCP 接入方式 | **ONE 内置 MCP 客户端** | 社区 MCP Server 生态是 Python/Node，但 ONE 直接启动 subprocess 做 MCP Client，不需要中间网关 |
| 网关语言 | 不适用 | ONE 的 Rust 代码直接管理 MCP Server 子进程，不需要独立网关 |
| MCP Server 实现语言 | **Python/Node/Go 均可** | 由社区生态决定，ONE 的 MCP Client 与语言无关 |
| Agent 与工具关系 | **Agent 不持有工具，通过 ToolRegistry 注入** | 解耦，支持动态扩缩 |
| Skill 与 Tool 关系 | **Skill = Tool（统一命名空间）** | LLM 只理解 tool calling，不需要区分来源 |
| 工具命名空间 | `skill:` / `mcp:` 前缀 | 避免冲突，LLM 可通过名称区分来源 |
| .skill 文件格式 | **YAML frontmatter + Markdown body** | 兼容 Hermes Agent 生态，人类可读 |
| Skill 执行方式 | **MCP 为主，直接命令为辅** | MCP 是标准协议，直接命令做 fallback |
| Agent 注册 | **AgentRegistry 全局单例** | 简单可控，启动时注册所有 Agent 类型 |
| IntentRouter | **关键词匹配 + 兜底 LLM** | 关键词快速路由常见请求，复杂请求走 LLM 判断 |

---

## 七、与现有代码兼容性

1. **现有 6 个 Rust Skill 不改动** — 它们继续实现 `Skill` trait，通过 `Builtin(Box<dyn Skill>)` 包装后注册到新的 `ToolRegistry`
2. **AppState 字段不改动** — 只新增 `mcp_manager`，不影响现有状态管理
3. **路由不改动** — `route_message()` → `spawn_orchestrator_run()` 流程不变
4. **Chat UI 不改动** — 消息渲染、流式显示、think blocks 等均保持原样
5. **权限系统不改动** — `PermissionPolicy` 继续保持，MCP 工具调用也走权限检查
6. **DB/记忆系统不改动** — task_db、memory 保持原样