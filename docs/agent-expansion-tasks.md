# ONE Agent 架构重构 — 任务跟踪

> 基于 [agent-expansion-plan.md](./agent-expansion-plan.md) 的实施路线图

---

## 总进度

| Phase | 内容 | 状态 | 预估工时 |
|-------|------|------|---------|
| Phase 1 | Agent 抽象层 | ⏳ 未开始 | 2-3 天 |
| Phase 2 | MCP 客户端 | ⏳ 未开始 | 1-2 天 |
| Phase 3 | 动态 Skill 系统 | ⏳ 未开始 | 1-2 天 |
| Phase 4 | 系统集成 | ⏳ 未开始 | 1 天 |
| Phase 5 | Claude Code 集成 | ⏳ 未开始 | 0.5 天 |

---

## Phase 1: Agent 抽象层

### 1.1 Agent trait 定义

**文件**: `src/agents/core/agent.rs`（新增）

- [ ] 定义 `Agent` trait，包含：
  - `fn id(&self) -> &str`
  - `fn name(&self) -> &str`
  - `fn soul_prompt(&self) -> &str`
  - `fn tool_filter(&self) -> Option<Vec<String>>`（默认 `None`）
  - `fn build_system_prompt(&self, tool_descriptions: &str) -> String`（默认实现：soul + 工具列表）

```rust
#[async_trait]
pub trait Agent: Send + Sync {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn soul_prompt(&self) -> &str;
    fn tool_filter(&self) -> Option<Vec<String>> { None }
    fn build_system_prompt(&self, tool_descriptions: &str) -> String {
        format!("{}\n\n## 可用工具\n\n{}", self.soul_prompt(), tool_descriptions)
    }
}
```

### 1.2 ToolRegistry 定义

**文件**: `src/agents/core/tool_registry.rs`（新增）

- [ ] 定义 `ToolSource` 枚举
- [ ] 定义 `ToolResult` 枚举
- [ ] 定义 `ToolRegistry` 结构体
- [ ] 实现 `tool_definitions()` — 获取所有工具定义（用于 LLM tool calling）
- [ ] 实现 `execute()` — 统一路由所有工具调用
- [ ] 实现全局单例 `tool_registry()`

```rust
pub enum ToolSource {
    Builtin(Arc<dyn Tool>),
    Skill(String),  // skill_id
    Mcp { server: String, tool_name: String },
}

pub enum ToolResult {
    Builtin(Value),
    Skill(SkillExecution),
    Mcp(Value),
}

pub struct ToolRegistry {
    builtin_tools: Vec<Arc<dyn Tool>>,
    skill_tools: Vec<SkillToolRegistration>,
    mcp_tools: Vec<McpToolRegistration>,
}
```

### 1.3 AgentRegistry + AgentBuilder

**文件**: `src/agents/core/agent_factory.rs`（新增）

- [ ] 定义 `AgentBuilder` trait
- [ ] 定义 `AgentRegistry`（`HashMap<String, Box<dyn AgentBuilder>>`）
- [ ] 实现 `register()`、`create()`、`all_agents()`

### 1.4 导出新类型

**文件**: `src/agents/core/mod.rs`（修改）

- [ ] 添加 `pub mod agent;`
- [ ] 添加 `pub mod tool_registry;`
- [ ] 添加 `pub mod agent_factory;`
- [ ] 导出 `Agent`, `ToolRegistry`, `ToolSource`, `ToolResult`, `AgentRegistry`, `AgentBuilder`

### 1.5 重构 MainAgent

**文件**: `src/agents/core/main_agent.rs`（修改）

- [ ] 实现 `MainAgent` struct（实现 `Agent` trait）
- [ ] 实现 `MainAgentBuilder`（实现 `AgentBuilder` trait）
- [ ] **灵魂加载逻辑**保留：从 `~/.one/soul.md` 读取
- [ ] **system prompt 变更**：去掉硬编码的 skill 描述，Agent 不再直接持有工具列表
- [ ] 工具由 ToolRegistry 在运行时注入

### 1.6 重构 Orchestrator

**文件**: `src/agents/core/orchestrator.rs`（修改）

- [ ] 去掉 `dispatch_tool` 中的 `"run_system_task"` 硬编码拦截
- [ ] 所有工具通过 `ToolRegistry::execute()` 统一路由
- [ ] 支持 `AgentRunContext` 注入（包含 tool_sources, cancel_flag, user_input_rx）
- [ ] 接收 `Arc<ToolRegistry>` 或通过全局单例获取

### 1.7 编译验证

- [ ] `cargo build` 通过
- [ ] 确认现有功能不受影响（系统工具、记忆、灵魂更新等）

---

## Phase 2: MCP 客户端

### 2.1 JSON-RPC 2.0 协议

**文件**: `src/mcp/protocol.rs`（新增）

- [ ] `JsonRpcRequest` 结构体（jsonrpc, id, method, params）
- [ ] `JsonRpcResponse` 结构体（id, result, error）
- [ ] `JsonRpcError` 结构体（code, message, data）
- [ ] 序列化/反序列化实现

### 2.2 传输层

**文件**: `src/mcp/transport.rs`（新增）

- [ ] `McpTransport` 枚举（Stdio / Http）
- [ ] Stdio 实现：启动子进程，读写 stdin/stdout
- [ ] HTTP 实现：reqwest 客户端 + SSE 支持
- [ ] `request()` 方法：发送 JSON-RPC 请求，等待响应
- [ ] `shutdown()` 方法

### 2.3 .mcp.json 解析

**文件**: `src/mcp/config.rs`（新增）

- [ ] `McpServerConfig` 结构体
- [ ] `McpConfig` 结构体
- [ ] 从文件解析配置
- [ ] 环境变量替换（`${VAR_NAME}`）

### 2.4 McpClientManager

**文件**: `src/mcp/mod.rs`（新增）

- [ ] `McpClientHandle` 结构体（config + transport + tools 缓存）
- [ ] `McpToolDefinition` 结构体（server_name, tool_name, description, input_schema）
- [ ] `load_and_connect()` — 加载配置，启动所有 MCP Server
- [ ] `discover_all_tools()` — 调用 `tools/list`，缓存结果
- [ ] `call_tool()` — 调用 `tools/call`，转发到对应 Server
- [ ] `shutdown_all()` — 关闭所有连接

### 2.5 MCP 集成到 AppState

**文件**: `src/app_state.rs`（修改）

- [ ] 新增 `mcp_manager: Option<McpClientManager>` 字段
- [ ] AppState::new() 中初始化（或异步启动后注入）
- [ ] 启动时调用 discover_all_tools

### 2.6 main.rs 初始化

**文件**: `src/main.rs`（修改）

- [ ] 启动后异步初始化 MCP 连接
- [ ] 失败时不影响主流程（MCP 不可用不阻塞启动）

### 2.7 注册到 ToolRegistry

- [ ] 将 `McpToolDefinition` 转换为 `ToolSource::Mcp`
- [ ] 注册到全局 `ToolRegistry`

---

## Phase 3: 动态 Skill 系统

### 3.1 DynamicSkill 加载器

**文件**: `src/skills/dynamic_skill.rs`（新增）

- [ ] `SkillExecutor` 枚举（McpTool / Command / Local）
- [ ] `DynamicSkill` 结构体
- [ ] `load_from_dir()` — 从目录加载 SKILL.md
- [ ] 解析 YAML frontmatter + Markdown body
- [ ] 实现 `Skill` trait（preview, execute）

### 3.2 SkillRegistry 扩展

**文件**: `src/skills/mod.rs`（修改）

- [ ] 新增 `dynamic: Vec<DynamicSkill>` 字段
- [ ] `refresh_dynamic()` — 扫描 `~/.one/skills/` 目录
- [ ] `all_skills()` — 合并 builtin + dynamic
- [ ] `find()` — 同时在 builtin 和 dynamic 中查找
- [ ] 启动时自动扫描

### 3.3 Skill → Tool 适配器

**文件**: `src/skills/mod.rs`（修改）

- [ ] `SkillAsTool` 包装器：实现 `Tool` trait，包装 `Skill` trait
- [ ] 注册到 `ToolRegistry`，命名空间 `skill:<id>`
- [ ] `call()` 方法：根据参数中 `apply` 字段决定 preview/execute

### 3.4 上传后刷新

**文件**: `src/skills_market.rs`（修改）

- [ ] `install_from_file` 成功后调用 `SkillRegistry::refresh_dynamic()`
- [ ] 通知 UI 更新已安装列表

---

## Phase 4: 系统集成

### 4.1 路由层适配

**文件**: `src/routing.rs`（修改）

- [ ] 使用新的 `AgentFactory`（通过 `AgentRegistry`）
- [ ] 传递 MCP 和 Skill 工具上下文给 Orchestrator

### 4.2 JobManager 适配

**文件**: `src/runtime/job_manager.rs`（修改）

- [ ] `spawn_orchestrator_run()` 适配新的 Orchestrator 接口
- [ ] `AgentRunContext` 的创建和注入

### 4.3 IntentRouter 实装

**文件**: `src/agents/intent_router.rs`（修改）

- [ ] 添加 system_keywords：进程、内存、磁盘、CPU、文件操作等
- [ ] 添加 coding_keywords：写代码、修复 bug、重构、审查等

### 4.4 端到端验证

- [ ] 测试流程：安装 skill → Agent 感知 → LLM 调用 → 执行 → 返回结果
- [ ] MCP 工具调用流程验证
- [ ] 新旧 skill 兼容性验证

---

## Phase 5: Claude Code 集成

### 5.1 Claude Code MCP Server

**文件**: `scripts/claude_code_mcp_server.py`（新增）

- [ ] 实现 MCP 协议（JSON-RPC 2.0 over stdio）
- [ ] 暴露 `run_task` tool
- [ ] 内部调用 `claude -p "<task>"` 命令
- [ ] 流式输出支持
- [ ] 参数：task, work_dir, max_turns, allowed_tools

### 5.2 .mcp.json 配置

- [ ] 添加 `claude-code` server 配置

### 5.3 claude-code SKILL.md

**文件**: `~/.one/skills/claude-code/SKILL.md`（新增）

- [ ] 符合 ONE 的 SKILL.md 规范
- [ ] 包含参数说明和使用说明

---

## 依赖关系

```
Phase 1 (Agent 抽象层) ──→ Phase 4 (系统集成)
       │                           │
       ▼                           ▼
Phase 2 (MCP 客户端) ────────→ Phase 4
       │
       ▼
Phase 3 (动态 Skill) ─────────→ Phase 4
                                     │
                                     ▼
                               Phase 5 (Claude Code)
```

- Phase 1 是基础，必须先完成
- Phase 2 和 Phase 3 可并行
- Phase 4 依赖 Phase 1/2/3
- Phase 5 依赖 Phase 2/3/4

---

## 注意事项

1. **向后兼容**：每个 Phase 完成后 `cargo build` 验证，确保现有功能不受影响
2. **逐步提交**：每个子任务完成后 git commit，方便回滚
3. **Skill 现有 6 个不改动**：只加适配器不修改原有实现
4. **权限系统不变**：MCP 工具调用也走 `PermissionPolicy`
5. **UI 不改动**：聊天渲染、流式显示、think blocks 保持原样