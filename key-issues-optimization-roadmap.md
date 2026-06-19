# ONE 关键问题与优先优化技术路线

## 背景

ONE 当前已经具备一个清晰的产品方向：以本地 Workspace / Task 为组织单元，把 MainAgent、Skill、MCP、终端、Claude Code 编码工作流、记忆系统和远程触发整合成一个本地 AI 工作台。

这个方向是合理的，但当前实现仍处于 MVP 到产品化之间：主流程可跑，概念完整，但关键扩展点、权限边界、状态持久化、产物管理和测试治理还需要进一步硬化。

本文只整理关键问题、技术路线和实施方案，不直接修改业务代码。待 review 后再进入实施阶段。

## 调研依据

### 外部技术参照

1. GPUI 官方说明  
   GPUI 是 Rust 的 GPU 加速 UI 框架，采用 hybrid immediate / retained mode，适合构建高性能原生桌面应用。但官方也明确 GPUI 仍处于 active development / pre-1.0 状态，会有 breaking changes。  
   参考：https://github.com/zed-industries/zed/tree/main/crates/gpui

2. Claude Code CLI  
   Claude Code 已支持 Terminal、VS Code、Desktop、Web、JetBrains 等入口。CLI 支持 `--permission-mode`、`--output-format stream-json`、`--mcp-config`、`--tools`、`--disallowedTools`、`--settings`、`--worktree` 等关键参数。  
   参考：https://docs.anthropic.com/en/docs/claude-code/cli-reference

3. Claude Code Hooks  
   Hooks 可以在 Claude Code 生命周期中做确定性控制，例如 `PreToolUse` 阻断危险命令、`PostToolUse` 格式化、`Notification` 通知、`SessionStart` 注入上下文。官方说明中明确：`PreToolUse` hooks 会在 permission-mode 检查前执行，即使 `bypassPermissions` 也能被 hook 拦截。  
   参考：https://docs.anthropic.com/en/docs/claude-code/hooks-guide

4. Claude Code Memory  
   Claude Code 推荐用 `CLAUDE.md`、`.claude/rules/` 和 auto memory 组织项目规则和长期知识。规则越具体、越短、越有范围，遵循度越高。  
   参考：https://docs.anthropic.com/en/docs/claude-code/memory

5. MCP Tools 规范  
   MCP tools 是 model-controlled，支持 `tools/list` 和 `tools/call`，工具定义包含 `name`、`description`、`inputSchema`、可选 `outputSchema`。规范建议客户端对敏感操作做用户确认、展示输入、校验输出、设置超时、记录审计日志。  
   参考：https://modelcontextprotocol.io/specification/2025-06-18/server/tools

### 本地代码参照

关键文件：

- `src/app_state.rs`
- `src/runtime/job_manager.rs`
- `src/runtime/coding_workflow.rs`
- `src/agents/core/main_agent.rs`
- `src/agents/core/orchestrator.rs`
- `src/agents/intent_router.rs`
- `src/mcp/mod.rs`
- `src/skills/dynamic_skill.rs`
- `src/agents/permission.rs`
- `src/memory/snapshot.rs`
- `src/ui/mod.rs`
- `src/ui/terminal.rs`

当前规模信号：

- `src` 下 Rust 代码约 19,599 行。
- `TODO / unwrap / expect / eprintln / allow(dead_code)` 等工程风险信号约 203 处。
- 测试约 97 个，但关键 agent workflow 场景测试还不够。

## 总体判断

### 项目合理性

项目方向合理，并且比普通 AI Chat 壳更有价值：

- Workspace / Task 组织比单会话聊天更适合长期工作。
- 本地原生 UI + 内嵌终端 + task 目录产物，比 WebView 套壳更适合做本地工作台。
- MainAgent 统一意图理解，再派发 Skill / MCP / Claude Code，是正确的产品抽象。
- Claude Code 不重造，而是作为编码执行引擎接入，方向务实。
- Skill / MCP / remote trigger 是未来扩展生态的合理基础。

### 当前短板

核心问题不是“方向错”，而是“闭环还不够硬”：

- MCP client 有雏形，但未真正注入 Orchestrator。
- 动态 Skill 解析了 executor，但执行时只是返回指南文本。
- Claude Code auto-accept 可用，但缺少 task 级强边界和 hook 级硬约束。
- Task 产物没有统一索引和状态模型。
- CodingWorkflow 状态只在内存里，不可恢复。
- `AppState` / `JobManager` 过重，状态边界不清。
- 测试缺少端到端场景和 agent workflow 回归样例。

## 关键问题

## 1. MCP 未真正闭环

### 当前现状

`src/mcp/mod.rs` 已经实现：

- MCP server 连接。
- `tools/list` 工具发现。
- `tools/call` 工具调用。
- stdio / HTTP transport 抽象。

但 `src/runtime/job_manager.rs` 中创建 Orchestrator 时，当前逻辑只是发现 `mcp_manager` 可用并打印日志，没有真正注入。

这导致：

- MainAgent 看不到 MCP tools。
- MCP tools 无法进入 ToolRegistry。
- Skill 无法稳定复用 MCP。
- 项目“开放生态”的先进性无法落地。

### 目标

让 MCP 成为一等工具来源：

```text
McpConfig
  -> McpClientManager::connect()
  -> discover_all_tools()
  -> ToolRegistry 注册 mcp:<server>:<tool>
  -> MainAgent tool definitions 可见
  -> Orchestrator dispatch_tool 调用 MCP
  -> 结果进入日志、权限、UI
```

### 实施方案

1. 在 AppState 初始化或配置加载后建立 `McpClientManager`。
2. 将 `Arc<Mutex<McpClientManager>>` 注入 `AgentFactory::create_orchestrator`。
3. Orchestrator 构造时调用 `discover_all_tools()`。
4. 将 MCP 工具注册到 `ToolRegistry`：

```text
tool name: mcp:<server>:<tool>
description: server/tool description
input_schema: MCP inputSchema
source: ToolSource::Mcp { server, tool_name }
```

5. MainAgent system prompt 中加入“可用 MCP 工具概览”，但要控制 token：

```text
只列 server / tool / 简短描述，不注入长 schema。
schema 由 tool definitions 提供。
```

6. 调用 MCP 前进入权限判断：

- 只读类工具默认允许。
- 写入、外部 API、文件操作类工具需要 Ask。
- remote source 进入 Strict。

7. 为 MCP 调用增加超时、错误归一化和审计日志。

### 验收标准

- 配置一个 mock MCP server 后，MainAgent 能看到工具并调用。
- `tools/list` 失败不会影响应用启动。
- `tools/call` 失败能在聊天区和 run log 中清晰显示。
- 敏感 MCP 工具调用会进入权限确认。

## 2. 动态 Skill 只是指南，不是真执行器

### 当前现状

`src/skills/dynamic_skill.rs` 支持解析：

- frontmatter
- name / description / version / platforms
- danger_level
- executor: `mcp_tool` 或 `command`

但 `DynamicSkill::execute()` 当前只是返回 SKILL.md body，让 Agent 自行阅读后决定下一步。

这会导致：

- executor 声明没有实际意义。
- Skill 执行结果不可控。
- 权限无法统一。
- 动态 Skill 很难稳定复用和测试。

### 目标

把动态 Skill 改为真正可执行：

```text
DynamicSkill::preview(args)
  -> 展示 executor、参数、风险、将调用的 MCP/command

DynamicSkill::execute(args, source)
  -> PermissionPolicy
  -> McpClientManager 或 SandboxBackend
  -> SkillExecution
```

### 实施方案

#### mcp_tool executor

frontmatter：

```yaml
executor:
  type: mcp_tool
  server: github
  tool: search_repositories
  args_template: '{"query":"{{query}}"}'
```

执行流程：

```text
args + args_template
  -> 渲染成 JSON
  -> permission check
  -> mcp.call_tool(server, tool, rendered_args)
  -> SkillExecution
```

#### command executor

frontmatter：

```yaml
executor:
  type: command
  command: "python3"
  args: ["script.py", "{{input}}"]
```

执行流程：

```text
args template render
  -> permission check ToolKind::Shell
  -> 在 skill source_dir 或 task_dir 中执行
  -> stdout/stderr capture
  -> SkillExecution
```

#### 模板策略

不要先做复杂模板引擎，第一阶段只支持：

- `{{key}}` 字符串替换。
- JSON 模板必须 parse 成 `serde_json::Value`。
- 缺 key 直接返回 preview warning 或 execute error。

### 验收标准

- 一个 command dynamic skill 可以真实执行并返回结果。
- 一个 MCP dynamic skill 可以真实调用 MCP tool。
- Dangerous / Extreme skill 会触发审批或远程暗号流程。
- 执行失败能返回结构化 `failed_items`。

## 3. Claude Code auto-accept 缺少硬边界

### 当前现状

`src/runtime/coding_workflow.rs` 已实现两阶段：

1. Planning：`claude -p <planning prompt>`
2. Implementation：`claude -p <implementation prompt> --permission-mode bypassPermissions`

该方案 UX 顺滑，但 `bypassPermissions` 是高权限模式。Claude Code 官方 CLI 也明确 `--dangerously-skip-permissions` 等价于 `--permission-mode bypassPermissions`。

### 风险

- Claude Code 可能写出 task 目录外的文件。
- 可能执行危险 shell 命令。
- 用户以为“只作用于当前 task”，但实际权限可能超过这个范围。
- 当前只依赖 prompt 约束，不是硬边界。

### 技术路线

使用“三层防护”：

```text
第一层：工作目录约束
第二层：Claude Code settings / hooks 约束
第三层：ONE 自己的产物扫描和审计
```

### 实施方案

#### 1. 为每个 task 生成临时 Claude settings

在 task 目录创建：

```text
.claude/
  settings.local.json
  hooks/
    one-pretool-guard.sh
```

`settings.local.json` 用于：

- 注册 `PreToolUse` hook。
- 限制或记录 Bash / Edit / Write。
- 可选设置 allowed / disallowed tools。

#### 2. PreToolUse hook 做硬约束

Hook 逻辑：

- 读取 Claude Code 传入的 tool JSON。
- 对 Bash 命令做 denylist：
  - `rm -rf /`
  - `sudo`
  - `chmod -R 777`
  - 写入 `$HOME`、`/etc`、项目根外路径
- 对 Edit / Write 路径做校验：
  - 允许 task_dir 内。
  - 禁止 `.one`、`.git`、系统目录。
  - 可选允许 workspace 只读。
- 违规时 exit code 2 阻断。

Claude Code hooks 官方说明中，`PreToolUse` hooks 在 permission-mode 检查前执行，即使 `bypassPermissions` 也能阻断，因此这是当前最适合的硬约束点。

#### 3. 命令参数调整

第二阶段建议：

```text
claude -p <implementation prompt>
  --permission-mode bypassPermissions
  --settings <task_dir>/.claude/settings.local.json
  --output-format stream-json
  --verbose
```

`stream-json` 用于后续更准确区分：

- assistant message
- tool use
- hook event
- error
- final result

第一阶段可考虑：

```text
claude -p <planning prompt>
  --permission-mode plan
  --output-format stream-json
```

但需要验证 `-p` 模式和 plan mode 的实际行为。如果不可用，继续使用纯 prompt 禁止写文件，同时加 PreToolUse hook。

#### 4. 执行后扫描 task 目录

Implementation 完成后：

```text
walk task_dir
  -> 生成 artifact diff
  -> 记录新增/修改文件
  -> 更新 task_artifacts 表
```

### 验收标准

- Claude Code implementation 阶段仍然 auto-accept。
- 尝试写 task_dir 外文件会被 hook 阻断。
- 终端能显示 hook 阻断原因。
- 聊天区最终摘要包含新增/修改文件清单。

## 4. Task Artifact 模型缺失

### 当前现状

Task 目录已有概念，CodingWorkflow 会写：

- `CLAUDE_PLAN.md`
- `claude-code.log`
- 应用文件

但 DB 中没有统一产物模型。

问题：

- UI 不知道哪些文件是产物。
- 预览、运行、日志、计划文件之间没有结构化关系。
- 用户切换 task 后只能从文件系统猜测状态。
- 以后无法做 artifact version / diff / revert / publish。

### 目标

增加一张轻量 artifact 表：

```sql
CREATE TABLE task_artifacts (
  id INTEGER PRIMARY KEY,
  task_id INTEGER NOT NULL,
  kind TEXT NOT NULL,
  path TEXT NOT NULL,
  title TEXT,
  status TEXT NOT NULL DEFAULT 'ready',
  metadata_json TEXT,
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
  updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
```

kind 建议：

- `plan`
- `log`
- `source`
- `entry`
- `preview`
- `report`
- `asset`

### 实施方案

1. CodingWorkflow start 时登记：
   - `CLAUDE_PLAN.md` as `plan`
   - `claude-code.log` as `log`
2. Implementation 完成后扫描 task 目录：
   - `index.html` / `package.json` / `README.md` 等登记为 artifact。
3. `AppState::try_prepare_preview()` 优先读 artifact entry。
4. Sidebar 展示 artifact 列表：
   - Plan
   - Log
   - App Entry
   - Source Files
5. 后续可扩展 version：
   - 初期只记录当前状态。
   - 第二阶段再引入 hash / mtime / diff。

### 验收标准

- Task 完成编码后，sidebar 能列出产物。
- 用户能打开 plan/log/entry。
- preview 不再只靠文件名猜测。

## 5. CodingWorkflow 状态没有持久化

### 当前现状

`CodingWorkflowState` 只存在 `JobManager` 内存中。

如果应用退出或切换状态复杂：

- Planning 完成但未确认的状态丢失。
- 用户回来后不知道是否可继续 implementation。
- 日志和计划文件在磁盘，但 UI 状态不可恢复。

### 目标

将 coding workflow 状态持久化到 DB。

建议新增：

```sql
CREATE TABLE coding_workflows (
  id INTEGER PRIMARY KEY,
  task_id INTEGER NOT NULL,
  stage TEXT NOT NULL,
  user_request TEXT NOT NULL,
  main_agent_summary TEXT,
  known_constraints_json TEXT,
  suggested_direction TEXT,
  clarification_focus_json TEXT,
  plan_path TEXT,
  log_path TEXT,
  approval_notes_json TEXT,
  last_error TEXT,
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
  updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
```

### 实施方案

1. `start_coding_workflow()` 创建或更新 workflow row。
2. 每次 stage 变化写 DB：
   - PlanningRunning
   - AwaitingApproval
   - Implementing
   - Done
   - Failed
   - Cancelled
3. `route_message()` 中处理确认前，从 DB 恢复当前 task 的 pending workflow。
4. App 启动加载 active task 时，如果存在 `AwaitingApproval`，聊天区提示：

```text
Claude Code 已完成方案，等待确认。
```

5. 对 Done / Failed 保留历史，但不拦截后续普通消息。

### 验收标准

- Planning 完成后重启应用，仍能继续确认执行。
- Failed 状态能显示错误和日志入口。
- Done 状态不影响新对话。

## 6. AppState / JobManager 职责过重

### 当前现状

`AppState` 包含：

- workspace/task 状态
- UI 可见性
- terminal 状态
- preview process
- model config
- approval dialog
- telegram binding
- MCP manager
- terminal emulator
- coding workflow

`JobManager` 包含：

- general AI run
- summarize job
- orchestrator channel
- coding workflow
- terminal command output
- cancel flag

短期还能维护，长期会导致：

- 切 task bug。
- 状态清理不完整。
- GUI 交互和 runtime 逻辑耦合。
- 新功能容易互相影响。

### 目标

逐步拆 store，不做一次性大重构。

建议边界：

```text
TaskRuntimeStore
  - active runs
  - request state
  - cancel state

CodingWorkflowStore
  - current workflow
  - db sync
  - start/confirm/cancel

TerminalStore
  - terminal emulator
  - terminal output
  - terminal cwd
  - resize state

ApprovalStore
  - local approval queue
  - pending approval dialog

ArtifactStore
  - task artifact indexing
  - preview entry
```

### 实施方案

不建议马上大拆。先按新功能引入边界：

1. P0 只新增 `coding_workflow` DB API 和 artifact DB API。
2. P1 把 `src/runtime/coding_workflow.rs` 中的 AppState 方法迁移成独立 service：

```text
CodingWorkflowService
  start()
  handle_user_input()
  finish_stage()
  persist()
```

3. P2 再拆 TerminalStore。

### 验收标准

- 新增功能不再继续扩大 `job_manager.rs`。
- CodingWorkflow 单元测试不依赖完整 GUI。

## 7. 记忆系统需要作用域治理

### 当前现状

当前记忆系统包含：

- global facts
- workspace facts
- task snapshot
- TF-IDF 跨任务检索

优点：

- 简单。
- 无需 embedding 服务。
- 可本地运行。

问题：

- 自动把 key_facts 写入 global + workspace，可能污染长期记忆。
- preferences 写 workspace，也可能把临时偏好永久化。
- TF-IDF 对中文、短句和代码上下文召回质量有限。
- 没有 memory review / delete / confidence。

### 技术路线

先治理作用域，再考虑 embedding。

#### P0

新增 memory item metadata：

```text
scope: global | workspace | task
source_task_id
confidence
kind: fact | preference | rule | decision | open_loop
created_at
last_used_at
```

#### P1

增加记忆审核 UI：

- 本次会话准备写入哪些记忆。
- 用户可接受/拒绝/改写。
- 支持删除过期记忆。

#### P2

可选 embedding：

- 本地 sqlite-vec / tantivy / fastembed。
- 不替换 TF-IDF，先作为增强召回。

### 验收标准

- 临时偏好不会自动进入 global。
- 用户可查看和删除记忆。
- 召回内容在聊天区可解释来源。

## 优先优化路线

## P0：先把核心闭环补齐

目标：让 ONE 从“概念完整”变成“核心路径可信”。

### P0.1 MCP 注入 Orchestrator

范围：

- `AgentFactory`
- `Orchestrator`
- `ToolRegistry`
- `McpClientManager`

交付：

- MCP tools 可被 MainAgent 调用。
- MCP 调用进入 run log。
- MCP 调用有超时和错误处理。

### P0.2 Dynamic Skill executor 真执行

范围：

- `src/skills/dynamic_skill.rs`
- `src/skills/mod.rs`
- 权限策略
- MCP manager / sandbox backend

交付：

- `mcp_tool` executor 可执行。
- `command` executor 可执行。
- preview 展示将执行内容。
- execute 走权限。

### P0.3 Claude Code task sandbox guard

范围：

- `src/runtime/coding_workflow.rs`
- task `.claude/settings.local.json`
- hook script generation

交付：

- auto-accept 仍保留。
- PreToolUse hook 阻断 task 外写入和危险命令。
- `stream-json` 输出预研并可选接入。

### P0.4 Artifact DB

范围：

- `src/task_db.rs`
- `src/runtime/coding_workflow.rs`
- `src/ui/sidebar.rs`

交付：

- Plan / log / entry 文件结构化登记。
- Sidebar 能展示 artifacts。

## P1：状态持久化和工程治理

### P1.1 CodingWorkflow DB 持久化

交付：

- AwaitingApproval 可重启恢复。
- Failed / Cancelled 有错误详情。
- Done 不阻塞后续对话。

### P1.2 结构化日志替换 eprintln

交付：

- `log` / `env_logger` 统一。
- LLM request 不再默认打印完整消息和敏感内容。
- Debug mode 才输出详细信息。

### P1.3 场景测试

优先测试场景：

- 编码任务触发 `start_coding_workflow`。
- Planning -> AwaitingApproval。
- 用户补充意见。
- 用户确认 -> Implementing。
- Cancel。
- 切 task 不串消息。
- MCP tool 调用成功/失败。
- Dynamic Skill command executor 权限拒绝。

## P2：体验和长期能力

### P2.1 记忆治理 UI

交付：

- 查看记忆。
- 审核新记忆。
- 删除/禁用记忆。

### P2.2 Terminal / Preview / Artifact 一体化

交付：

- 一个 task 可以清晰看到：
  - Plan
  - Terminal log
  - Generated files
  - Preview
  - Run command

### P2.3 可选 worktree / sandbox 模式

参考 Claude Code CLI 的 `--worktree` 思路，为高风险编码任务提供隔离执行：

```text
task_dir as default
git worktree / temp workspace as optional strict mode
```

## 推荐实施顺序

### Phase 1：MCP 和 Dynamic Skill 闭环

1. 注入 MCP manager 到 Orchestrator。
2. 注册 MCP tools 到 ToolRegistry。
3. DynamicSkill 支持 `mcp_tool` executor。
4. DynamicSkill 支持 `command` executor。
5. 增加权限和测试。

原因：

- 这是项目生态扩展能力的基础。
- 能马上提升“Skill Market”可信度。
- 不直接影响 Claude Code 工作流。

### Phase 2：Claude Code 安全边界

1. 生成 task `.claude/settings.local.json`。
2. 生成 PreToolUse hook。
3. implementation 阶段加 `--settings`。
4. 验证 `bypassPermissions` 下 hook 仍能阻断。
5. 预研 `--output-format stream-json` 替换纯文本解析。

原因：

- 当前 auto-accept 是用户体验优势，但必须有硬边界。
- 这是编码工作流从 demo 到可长期使用的关键。

### Phase 3：Artifact 和 Workflow 持久化

1. 新增 `task_artifacts`。
2. 新增 `coding_workflows`。
3. CodingWorkflow 每个阶段落库。
4. Sidebar 展示 artifact。
5. 重启恢复 AwaitingApproval。

原因：

- 解决 task 工作台的长期价值。
- 让“任务目录”从文件夹变成产品对象。

### Phase 4：治理和体验

1. 拆分 runtime store。
2. 结构化日志。
3. 记忆审核 UI。
4. 更多 GUI / GPUI 测试。

## 设计原则

1. 主流程固化，扩展能力插件化  
   Claude Code workflow、MCP dispatch、权限审批属于主流程；具体外部工具和业务 Skill 应插件化。

2. Prompt 只做意图，策略必须硬执行  
   路径限制、危险命令阻断、权限确认不能只靠 prompt。

3. Task 目录是用户产物边界  
   `.one` 只存内部配置和状态；用户代码、计划、日志、预览都应归属 task。

4. 先稳定少数闭环，再扩展更多平台  
   MCP + Dynamic Skill + Claude Code 是当前最重要的三条能力线。

5. 可解释性优先  
   用户要能看到：
   - agent 为什么这么做。
   - 调用了什么工具。
   - 产物在哪里。
   - 哪些操作被阻断。
   - 哪些记忆被保存。

## 风险与规避

### GPUI pre-1.0 风险

风险：

- API 变更频繁。
- 文档不完整。

规避：

- 封装常用 UI 交互组件。
- 避免过度依赖未稳定 API。
- 关键交互写 GPUI 测试。

### Claude Code CLI 行为变化

风险：

- 参数语义变化。
- 输出格式变化。
- permission mode 调整。

规避：

- 启动前检测 `claude --version`。
- 将 CLI 参数封装成 adapter。
- 优先使用 `stream-json` 结构化输出。
- 对关键参数做 smoke test。

### 权限绕过风险

风险：

- `bypassPermissions` 带来过高权限。

规避：

- PreToolUse hook。
- task_dir 路径校验。
- denylist + allowlist。
- 运行日志审计。

### 记忆污染风险

风险：

- 临时偏好进入长期记忆。

规避：

- memory scope。
- confidence。
- 用户审核。
- 可删除。

## Review 建议

建议 review 时重点确认以下问题：

1. P0 是否优先做 MCP + Dynamic Skill，而不是继续扩展 Claude Code UI？
2. Claude Code implementation 是否接受 task `.claude/settings.local.json` 和 hook 方案？
3. `task_artifacts` 表是否满足你想要的产物展示方式？
4. CodingWorkflow 是否需要支持多个并行 workflow，还是每个 task 同时只允许一个？
5. Dynamic Skill 的 command executor 是否应该默认禁用，必须显式开启？
6. 记忆审核 UI 是否放到 P2，还是提前到 P1？

## 一句话路线图

先把 MCP 和 Dynamic Skill 做成真正可调用的扩展生态，再用 Claude Code hooks 给 auto-accept 加硬边界，随后把 task artifact 和 workflow 状态持久化，最后做记忆治理和 UI 产品化。
