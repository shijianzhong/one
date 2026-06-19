# Claude Code 两阶段编码工作流实现方案

## 目标

当用户在某个 task 中提出“做应用 / 开发功能 / 实现页面 / 修改代码”等编码类需求时，系统由 MainAgent 先完成意图理解和初步梳理；确认是编码任务后，自动启动 Claude Code 工作流。

Claude Code 工作流分两阶段：

1. 方案阶段：Claude Code 只做需求细化、方案调研、任务拆解，不修改文件。
2. 执行阶段：用户确认后，Claude Code 使用 auto-accept 模式执行编码。

终端区域展示 Claude Code 的实时执行过程；MainAgent 对话区域负责阶段性总结、等待确认和最终总结。

## 用户体验流程

1. 用户在 task 对话中输入编码需求。
2. MainAgent 判断这是编码任务。
3. MainAgent 在聊天区简要梳理用户需求。
4. MainAgent 调用内部工具 `start_coding_workflow`。
5. 系统自动打开右侧终端。
6. Claude Code 在当前 task 目录下启动第一阶段。
7. 第一阶段只输出方案，不修改文件。
8. 第一阶段完成后，MainAgent 在聊天区总结 Claude Code 输出。
9. 系统等待用户确认。
10. 用户确认后，Claude Code 进入第二阶段并执行编码。
11. 第二阶段使用 auto-accept，减少权限确认打断。
12. 编码完成后，MainAgent 在聊天区总结修改内容、运行方式和验证结果。

## 核心原则

- 不用简单关键词直接触发 Claude Code。
- 编码任务必须先经过 MainAgent 的意图理解。
- Claude Code 第一阶段不得修改文件。
- 第二阶段必须等待用户确认后才能执行。
- 第二阶段使用 auto-accept 模式。
- 所有任务产物都写入当前 task 目录。
- 终端展示 Claude Code 实时过程。
- 聊天区只展示摘要、确认和最终总结，不替代终端。

## 当前代码现状

### 路由入口

文件：`src/routing.rs`

当前流程：

```text
route_message()
  -> 写入用户消息
  -> quick_route()
  -> spawn_orchestrator_run()
```

`IntentRouter` 中已有 Coding intent，但目前 Coding 返回 `None`，不会真正进入专门编码流程。

### MainAgent

文件：`src/agents/core/main_agent.rs`

当前 MainAgent 提供：

- `run_in_terminal`
- `run_system_task`
- `remember`
- `recall`
- `propose_soul_update`

`run_in_terminal` 目前只是一个通用工具，是否调用、调用什么命令由 LLM 自己判断。

### Orchestrator

文件：`src/agents/core/orchestrator.rs`

当前会拦截 `run_in_terminal`，发送：

```rust
OrchestratorEvent::RunInTerminal { command, work_dir }
```

### Runtime 执行

文件：`src/runtime/job_manager.rs`

当前收到 `RunInTerminal` 后：

```text
terminal_visible = true
Backend::exec_command()
```

注意：当前执行方式是 `std::process::Command::output()`，输出结束后一次性返回，不是真正流式写入右侧 terminal emulator。

因此如果要让用户在终端区域看到 Claude Code 实时执行过程，需要新增可流式输出的 Claude Code runner。

## 方案概览

采用主工程固化流程，不先做外挂 Skill。

理由：

- 这是产品核心交互，不是单个工具调用。
- 涉及 MainAgent 意图理解、用户确认、终端展示、task 目录、运行状态、输出总结。
- 固化在主工程更容易先做出稳定体验。
- 后续流程稳定后，可以再抽象为 Skill 或 adapter。

## 新增组件

### 1. MainAgent 工具：`start_coding_workflow`

文件：`src/agents/core/main_agent.rs`

新增工具：

```text
start_coding_workflow
```

参数建议：

```json
{
  "user_request": "用户原始需求",
  "main_agent_summary": "MainAgent 对需求的初步梳理",
  "known_constraints": ["已知约束"],
  "suggested_direction": "可选，建议技术方向",
  "clarification_focus": ["希望 Claude Code 第一阶段重点澄清的问题"]
}
```

工具职责：

- 不直接执行编码。
- 向 Orchestrator/Runtime 发出编码工作流启动事件。
- 由 Runtime 接管两阶段 Claude Code 流程。

### 2. Orchestrator 事件

文件：`src/agents/core/orchestrator.rs`

新增事件：

```rust
CodingWorkflowRequested {
    user_request: String,
    main_agent_summary: String,
    known_constraints: Vec<String>,
    suggested_direction: Option<String>,
    clarification_focus: Vec<String>,
}
```

MainAgent 调用 `start_coding_workflow` 后，Orchestrator 转发该事件。

### 3. Runtime 状态机

新增或扩展文件：

```text
src/runtime/coding_workflow.rs
src/runtime/job_manager.rs
```

状态建议：

```rust
enum CodingWorkflowStage {
    Idle,
    PlanningRunning,
    AwaitingApproval,
    Implementing,
    Done,
    Failed,
    Cancelled,
}
```

状态内容建议：

```rust
struct CodingWorkflowState {
    task_id: usize,
    task_dir: PathBuf,
    user_request: String,
    main_agent_summary: String,
    plan_path: PathBuf,
    log_path: PathBuf,
    stage: CodingWorkflowStage,
    last_plan_summary: Option<String>,
}
```

## Task 目录约定

所有产物写入当前 task 目录。

示例：

```text
<workspace>/
  tasks/
    <task-id-slug>/
      CLAUDE_PLAN.md
      claude-code.log
      <应用代码和项目文件...>
```

说明：

- `CLAUDE_PLAN.md`：第一阶段方案输出。
- `claude-code.log`：Claude Code 执行日志。
- 应用代码、配置、资源文件都直接在当前 task 目录下创建或修改。
- 不写入全局 `.one`。
- `.one` 只用于全局配置或内部状态，不用于用户 task 产物。

## 第一阶段：方案梳理

### 触发时机

MainAgent 判断为编码任务并调用 `start_coding_workflow` 后自动触发。

### Claude Code cwd

当前 task 目录。

### Claude Code 模式

第一阶段不使用 auto-accept 写文件，或者在 prompt 中强约束不改文件。

### 第一阶段 Prompt 草案

```text
你是 Claude Code。当前阶段只做需求澄清、方案调研和任务拆解，不要修改任何文件，不要创建任何项目文件。

用户原始需求：
{user_request}

MainAgent 初步梳理：
{main_agent_summary}

已知约束：
{known_constraints}

建议方向：
{suggested_direction}

请在当前 task 目录下只输出方案内容，并将完整方案写入 CLAUDE_PLAN.md。

你需要完成：

1. 复述用户目标和边界
2. 检查当前目录结构，判断是否已有项目基础
3. 梳理需要实现的核心功能
4. 调研并比较适合的实现方案
5. 给出推荐方案和理由
6. 拆解成可执行任务清单
7. 列出需要用户确认的问题
8. 明确下一阶段编码时会创建或修改哪些主要文件

重要限制：

- 当前阶段不要写业务代码
- 当前阶段不要初始化项目
- 当前阶段不要安装依赖
- 当前阶段不要修改已有源代码
- 只允许输出分析和方案
```

### 第一阶段完成后

Runtime 读取：

```text
CLAUDE_PLAN.md
claude-code.log
```

然后在聊天区追加摘要消息：

```text
Claude Code 已完成方案梳理：

- 目标理解：...
- 推荐方案：...
- 主要任务：...
- 需要确认：...

确认后我会让 Claude Code 进入编码阶段。
```

状态进入：

```text
AwaitingApproval
```

## 用户确认

### 确认触发

当 `CodingWorkflowStage::AwaitingApproval` 时，用户消息优先交给 coding workflow 处理。

确认语义示例：

```text
确认
开始
可以
按这个做
继续
执行
```

也允许用户补充：

```text
按方案 A 做，但是 UI 用 React
不要用 Tailwind
页面要移动端优先
```

如果用户补充修改意见，则第二阶段 prompt 带上用户补充内容。

### 不确认时

用户可以说：

```text
先别做
重新梳理
换个方案
```

后续可扩展为重新运行第一阶段。

第一版可以先支持：

- 确认 -> 第二阶段
- 其他内容 -> 作为补充确认信息，仍等待明确确认

## 第二阶段：执行编码

### 触发时机

用户确认后触发。

### Claude Code cwd

当前 task 目录。

### Claude Code 模式

使用 auto-accept，流程尽量顺畅。

命令建议：

```sh
claude -p "<implementation_prompt>" --permission-mode bypassPermissions
```

具体参数需按本机 Claude Code CLI 支持情况确认。

### 第二阶段 Prompt 草案

```text
你是 Claude Code。现在进入编码执行阶段。

工作目录就是当前 task 目录。所有代码、配置、资源和文档都应创建或修改在当前 task 目录内。

用户原始需求：
{user_request}

MainAgent 初步梳理：
{main_agent_summary}

第一阶段方案：
{CLAUDE_PLAN.md 内容}

用户确认/补充：
{approval_message}

请根据已确认方案执行编码任务。

执行要求：

1. 在当前 task 目录内完成实现
2. 如果需要创建应用项目，直接在当前 task 目录创建
3. 保持结构清晰，避免无关文件
4. 需要依赖时可创建配置文件并安装依赖
5. 完成后运行必要的检查或启动验证
6. 输出最终总结，包括：
   - 创建/修改了哪些文件
   - 如何运行
   - 做了哪些验证
   - 还有哪些后续建议
```

### 第二阶段完成后

Runtime 捕获输出，并在聊天区追加最终总结：

```text
Claude Code 编码完成：

- 已完成：...
- 主要文件：...
- 运行方式：...
- 验证结果：...
```

状态进入：

```text
Done
```

## 终端输出设计

### 当前问题

现有 `RunInTerminal` 使用：

```rust
std::process::Command::output()
```

这不是实时终端输出。

### 推荐实现

新增一个 Claude Code 专用 runner：

```text
CodingTerminalRunner
```

职责：

- 在当前 task 目录启动 Claude Code 子进程
- 实时读取 stdout/stderr
- 把输出追加到 `claude-code.log`
- 把输出推送到 UI 终端显示
- 进程结束时返回 exit status 和完整输出
- 支持取消

### UI 展示方案

第一版可以选择：

#### 方案 A：复用 terminal emulator

将 Claude Code 命令写入真实 terminal emulator，让用户看到实际交互。

优点：

- 用户看到的就是真终端。
- 改动较小。

缺点：

- Runtime 不容易可靠知道命令何时结束。
- 不容易完整捕获输出做总结。
- 自动化流程较弱。

#### 方案 B：新增流式 runner，UI 渲染 runner 输出

优点：

- 可以实时展示。
- 可以可靠捕获输出。
- 可以知道阶段结束。
- 可以保存日志。
- 方便 MainAgent 总结。

缺点：

- 实现略多。

推荐采用方案 B。

## MainAgent Prompt 调整

MainAgent system prompt 增加编码任务规则：

```text
当用户请求开发应用、实现功能、修改代码、创建页面、修复 bug、重构项目等编码任务时：

1. 先简要理解和整理用户需求
2. 不要直接给出完整代码实现
3. 调用 start_coding_workflow
4. 第一阶段由 Claude Code 做详细方案梳理和任务拆解
5. 等用户确认后，第二阶段再让 Claude Code 执行编码
6. 聊天区负责总结 Claude Code 的阶段输出，不替代终端输出
```

## 路由策略

不建议在 `IntentRouter` 中用关键词直接执行 Claude Code。

推荐策略：

```text
用户消息
  -> MainAgent
  -> MainAgent 判断是编码任务
  -> 调用 start_coding_workflow
  -> Runtime 接管 workflow
```

但是在 `CodingWorkflowStage::AwaitingApproval` 时，用户消息应优先交给 workflow，而不是重新进入普通 MainAgent。

## 数据持久化

第一版可以用文件持久化：

```text
CLAUDE_PLAN.md
claude-code.log
```

后续可扩展 DB：

```text
task_runs
run_events
coding_workflows
```

建议第一版只依赖 task 目录文件，降低复杂度。

## 错误处理

### Claude CLI 不存在

聊天区提示：

```text
未找到 Claude Code CLI，请确认 claude 命令可用。
```

### 第一阶段失败

状态进入 `Failed`，聊天区展示错误摘要，终端保留日志。

### 第二阶段失败

状态进入 `Failed`，聊天区展示：

- exit code
- 最后若干行日志
- 建议用户重试或补充信息

### 用户取消

复用现有停止逻辑，后续可加：

```text
cancel_coding_workflow()
```

## 权限策略

第二阶段使用 auto-accept：

```sh
--permission-mode bypassPermissions
```

说明：

- 用户体验更顺。
- 风险更高。
- 因为所有执行限制在当前 task 目录，风险可控一些。
- 后续可加设置项，让用户选择 auto-accept 或手动确认。

## 实施步骤

### Step 1：新增 Workflow 类型

新增：

```text
src/runtime/coding_workflow.rs
```

定义：

- `CodingWorkflowStage`
- `CodingWorkflowState`
- prompt 构造函数

### Step 2：扩展 JobManager/AppState 状态

增加：

```rust
coding_workflow: Option<CodingWorkflowState>
```

或放在 `JobManager` 内。

### Step 3：MainAgent 新增工具

在 `src/agents/core/main_agent.rs` 增加：

```text
start_coding_workflow
```

并更新 system prompt。

### Step 4：Orchestrator 新增事件

新增：

```rust
OrchestratorEvent::CodingWorkflowRequested
```

并在 tool call 中拦截。

### Step 5：Runtime 处理事件

在 `spawn_orchestrator_run()` 的事件循环里处理：

```text
CodingWorkflowRequested => start_coding_workflow_planning()
```

### Step 6：实现第一阶段 Claude Code runner

第一阶段：

- 打开终端区域
- cwd = task dir
- 启动 Claude Code
- 输出写入 `claude-code.log`
- 方案写入 `CLAUDE_PLAN.md`
- 完成后聊天区总结
- 状态进入 `AwaitingApproval`

### Step 7：确认后执行第二阶段

在 `route_message()` 入口优先判断：

```text
如果 coding_workflow.stage == AwaitingApproval
  -> 处理用户确认
  -> 启动 implementation
```

### Step 8：最终总结

第二阶段结束后，读取日志尾部或 Claude Code 最终输出，追加聊天区总结。

## 第一版验收标准

1. 用户输入“帮我做一个 Todo 应用”。
2. MainAgent 不直接写代码，而是调用编码工作流。
3. 右侧终端自动打开。
4. Claude Code 第一阶段输出方案。
5. 当前 task 目录出现：
   - `CLAUDE_PLAN.md`
   - `claude-code.log`
6. 聊天区展示方案摘要并等待确认。
7. 用户输入“确认，开始做”。
8. Claude Code 第二阶段用 auto-accept 执行编码。
9. 应用代码出现在当前 task 目录。
10. 聊天区展示最终总结和运行方式。

## 后续扩展

- 将 Claude Code runner 抽象为 provider。
- 支持 Codex / Gemini / Qwen 等其他 coding agent。
- 将 workflow 抽成 Skill。
- 支持重新规划。
- 支持用户选择方案 A/B。
- 支持运行结果预览。
- 支持从 `CLAUDE_PLAN.md` 恢复 interrupted workflow。
- 支持显示修改文件列表。
- 支持取消、重试、继续。
