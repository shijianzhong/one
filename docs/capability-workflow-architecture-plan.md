# 技能 / 能力 / 工作流长期架构方案

## 1. 结论

当前产品方向可以进一步收敛为三层：

```text
Skill（技能）
  原子能力。通常是一个工具、一个插件、一个外部执行器或一个本地系统操作。

Workflow（工作流）
  编排定义。由多个 Agent、Skill、MCP tool、条件、人工确认、输入输出节点组成。

Capability（能力）
  已发布、可调用、可复用的工作流产品形态。用户在主窗口调用的是 Capability。
```

也就是说：

- 左侧当前的“能力 / Capabilities”应改名为“技能 / Skills”，承载现有 Skill Market。
- 新增“能力 / Capabilities”，展示已经发布的 workflow。
- 工作流编排过程放在“能力”页面内的“工作流”tab 中。
- 工作流调试、保存、发布后，生成一个 Capability。
- MainAgent 在主聊天窗口可以调用这些 Capability。

这个方向合理，且和当前代码已经完成的 `AgentTrait / AgentRunContext / ToolSource` 收敛方向一致。不要把 Capability 实现成另一种 Skill；Capability 应该是 workflow runtime 的发布产物，Skill 只是 workflow 可使用的底层资源。

## 2. 调研参考

### LangGraph

LangGraph 把 agent workflow 建模为 graph，核心元素是 State、Nodes、Edges。Node 做具体工作，Edge 决定下一步；同时强调 checkpoint、interrupt、resume、runtime context 和 state schema。参考：<https://langchain-ai.github.io/langgraph/concepts/multi_agent/>

可借鉴点：

- Workflow 应该有显式 state，而不是只靠聊天历史。
- 节点与边应该是可持久化定义。
- 需要 checkpoint / resume，尤其是涉及人工确认时。
- Graph 节点可以是 agent，也可以是普通代码或工具。

### AutoGen

AutoGen 使用 team 表达多 agent 协作，例如 round-robin、selector、swarm 等。官方文档也明确建议：简单任务先用单 agent，只有单 agent 不足时再使用 multi-agent team。参考：<https://microsoft.github.io/autogen/stable/user-guide/agentchat-user-guide/tutorial/teams.html>

可借鉴点：

- 多 agent 不应成为默认复杂度。
- 需要 termination condition，避免无限对话。
- 需要 streaming trace，用户才能理解多个 agent 在做什么。
- Team/Workflow run 的结果应包含完整 message trace 和 stop reason。

### Dify

Dify 将 Workflow / Chatflow 区分为两类应用：Workflow 一次性运行，Chatflow 带对话层。它们都基于视觉画布和节点系统，节点可调用模型、知识库、工具、代码、条件分支等。参考：<https://docs.dify.ai/en/use-dify/build/workflow-chatflow>

可借鉴点：

- “构建流程”和“调用应用”应分开。
- 工作流发布后应像应用一样被调用。
- 非对话型 Workflow 和对话型 Chatflow 可以共享底层节点系统。

### CrewAI

CrewAI Flows 强调事件驱动、state management、条件分支、human feedback、持久化、plot/visualization，并可把 Agent 或 Crew 加入 Flow。参考：<https://docs.crewai.com/en/concepts/flows>

可借鉴点：

- Flow state 必须持久化。
- Human-in-the-loop 是一等节点，而不是异常流程。
- 可视化不是装饰，而是调试和信任建设的一部分。

## 3. 当前项目现状判断

### 已具备的基础

当前项目已经有几块可复用基础：

- `AgentTrait`：可以作为单个 Agent 的统一抽象。
- `AgentRunContext`：可以承载 history、metadata、tool sources、cancel flag、user input channel。
- `ToolSource` / `ToolRegistry`：已能统一 Builtin / Skill / MCP 工具来源。
- `SkillRegistry`：已有 builtin skill、dynamic skill、preview / execute 两阶段。
- `CodingWorkflowState` / `coding_workflows`：已经开始做 workflow 状态持久化。
- `RunEvent`：已有运行日志基础。
- `MainAgent`：可以继续作为主入口 Agent，负责理解用户意图和调用工具。

### 还缺的关键层

缺的是“工作流定义与运行时”：

- 没有 `WorkflowDefinition`。
- 没有 `WorkflowRun` 的统一状态机。
- 没有节点、边、条件、输入输出 schema。
- 没有 workflow builder / debug UI。
- 没有把 workflow 发布成 capability 的模型。
- 没有让 MainAgent 把 capability 作为 tool 调用。

因此下一阶段不应该继续膨胀现有 `Orchestrator`，而是新增 workflow runtime，并逐步把当前单 Agent loop 改造成 workflow runtime 中的一种节点。

## 4. 产品信息架构

### 左侧导航建议

```text
Workspace / Tasks

Skills（技能）
  当前 Skill Market。
  管理原子技能、安装插件、查看 executor、preview/execute。

Capabilities（能力）
  用户可直接调用的能力库。
  每个能力本质上是一个已发布 workflow。
  页面内包含：
    - 能力库 tab
    - 工作流 tab
```

### Skills 页面

Skills 页面承载现有能力市场，但命名从“能力”改为“技能”。

职责：

- 查看内置技能。
- 安装动态技能。
- 查看技能描述、参数、危险等级。
- 手动 preview / execute。
- 展示 skill 是否可被 workflow 使用。

不做：

- 不做多 agent 编排。
- 不做复杂状态机。
- 不把一个 workflow 打包成 skill。

### Capabilities 页面

Capabilities 页面承载“已发布能力”。

能力卡片字段：

- 名称。
- 描述。
- 输入 schema。
- 输出 schema。
- 使用的 agents。
- 使用的 skills / MCP tools。
- 权限等级。
- 最近运行状态。
- 发布版本。

操作：

- Run：在主窗口以表单或自然语言调用。
- Pin：固定到主窗口快捷入口。
- Edit Workflow：进入工作流 tab 编辑源 workflow。
- Duplicate：复制为新 workflow。
- Disable：停用能力。

### 工作流 tab

工作流 tab 是编排和调试场所。

第一阶段不建议做复杂画布，先做结构化表单 + step list：

```text
Workflow: Research and Draft

Input
  topic: string
  audience: string

Steps
  1. Agent: Researcher
     tools: web_search, recall, file_read
  2. Agent: Analyst
     depends_on: Researcher.output
  3. Human Approval
     approve / revise
  4. Agent: Writer
     tools: doc_summarizer, remember

Output
  final_markdown: string
```

后续再升级为可视化 graph canvas。

## 5. 核心概念模型

### Skill

Skill 是原子工具能力：

```rust
pub trait Skill: Send + Sync {
    fn manifest(&self) -> SkillManifest;
    async fn preview(&self, args: Value) -> Result<SkillPreview>;
    async fn execute(&self, args: Value, source: Option<&str>) -> Result<SkillExecution>;
}
```

继续沿用现有设计。

### Agent

Agent 是可执行角色：

```rust
pub trait AgentTrait: Send + Sync {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn soul_prompt(&self) -> &str;
    fn tool_filter(&self) -> Option<Vec<String>>;
    async fn step_stream(...) -> Result<AgentResponse>;
}
```

建议新增持久化配置：

```rust
pub struct AgentDefinition {
    pub id: String,
    pub name: String,
    pub role: String,
    pub goal: String,
    pub instructions: String,
    pub model: Option<String>,
    pub tool_filter: Vec<String>,
}
```

### Workflow

Workflow 是编排定义，不是运行实例：

```rust
pub struct WorkflowDefinition {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: i64,
    pub status: WorkflowStatus,
    pub input_schema: serde_json::Value,
    pub output_schema: serde_json::Value,
    pub nodes: Vec<WorkflowNode>,
    pub edges: Vec<WorkflowEdge>,
}
```

### Capability

Capability 是已发布 workflow 的调用入口：

```rust
pub struct CapabilityManifest {
    pub id: String,
    pub workflow_id: String,
    pub workflow_version: i64,
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
    pub output_schema: serde_json::Value,
    pub danger_level: DangerLevel,
    pub enabled: bool,
}
```

Capability 不复制 workflow 定义，只引用某个 workflow version。

### WorkflowRun

WorkflowRun 是一次运行实例：

```rust
pub struct WorkflowRun {
    pub id: String,
    pub workflow_id: String,
    pub workflow_version: i64,
    pub task_id: Option<usize>,
    pub status: WorkflowRunStatus,
    pub input_json: String,
    pub state_json: String,
    pub output_json: Option<String>,
    pub current_node_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}
```

## 6. Workflow 节点类型

第一阶段建议只支持 6 类节点：

```rust
pub enum WorkflowNodeKind {
    Agent,
    Skill,
    Capability,
    Condition,
    HumanApproval,
    Output,
}
```

### Agent 节点

运行一个 `AgentTrait`。

配置：

- agent definition。
- input mapping。
- allowed tools。
- max steps。
- stop condition。

### Skill 节点

调用已有 Skill。

配置：

- skill_id。
- args template。
- apply: preview / execute。
- permission policy。

### Capability 节点

调用另一个已发布 Capability。

用途：

- 复用能力。
- 组合复杂能力。
- 形成能力图谱。

需要限制递归深度，防止能力互相调用导致循环。

### Condition 节点

根据 state 选择下一条边。

第一阶段只支持简单表达式：

```text
state.score >= 0.8
state.approved == true
state.result contains "error"
```

不要第一阶段引入完整脚本语言。

### HumanApproval 节点

暂停 workflow，等待用户确认。

输出：

```json
{
  "approved": true,
  "feedback": "..."
}
```

这应直接复用现有 `user_input_rx` / approval 模型，但要落到 WorkflowRun 状态里。

### Output 节点

把 workflow state 映射成最终 output。

## 7. Runtime 架构

建议新增：

```text
src/workflows/
  mod.rs
  definition.rs
  registry.rs
  runtime.rs
  store.rs
  capability.rs
  events.rs
```

### WorkflowRegistry

负责读取 workflow definitions 和 capability manifests。

来源：

- SQLite。
- 后续可支持文件导入。

### WorkflowRuntime

负责运行 workflow：

```rust
pub struct WorkflowRuntime {
    agent_runtime: AgentRuntime,
    tool_dispatcher: ToolDispatcher,
    store: WorkflowStore,
}
```

不要让 `Orchestrator` 继续承担 workflow 编排。当前 `Orchestrator` 应逐步降级为：

```text
AgentRuntime
  单个 Agent 的 loop
  处理 step_stream、tool calls、context refresh
```

多 agent 编排应由 `WorkflowRuntime` 完成。

### ToolDispatcher

从当前 `Orchestrator::dispatch_tool` 中抽出。

职责：

- Builtin tool。
- Skill。
- MCP。
- Runtime bridge tool。
- Permission。
- 结构化结果。

这样 Agent 节点和 Workflow 节点都能复用工具执行。

### CapabilityTool

发布后的 Capability 应注册为 Tool，让 MainAgent 能调用：

```text
ToolSource::Capability { capability_id }
```

第一阶段也可以不改 `ToolSource`，先把 Capability 暴露成 builtin tool：

```text
run_capability(capability_id, input)
```

但长期应成为独立 ToolSource。

## 8. 数据库建议

第一阶段新增表，避免改现有 `main_agent_summary` 等历史字段。

### workflows

```sql
CREATE TABLE workflows (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  version INTEGER NOT NULL DEFAULT 1,
  status TEXT NOT NULL DEFAULT 'draft',
  definition_json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
```

### workflow_versions

```sql
CREATE TABLE workflow_versions (
  id TEXT PRIMARY KEY,
  workflow_id TEXT NOT NULL,
  version INTEGER NOT NULL,
  definition_json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  UNIQUE(workflow_id, version)
);
```

### capabilities

```sql
CREATE TABLE capabilities (
  id TEXT PRIMARY KEY,
  workflow_id TEXT NOT NULL,
  workflow_version INTEGER NOT NULL,
  name TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  manifest_json TEXT NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
```

### workflow_runs

```sql
CREATE TABLE workflow_runs (
  id TEXT PRIMARY KEY,
  workflow_id TEXT NOT NULL,
  workflow_version INTEGER NOT NULL,
  capability_id TEXT,
  task_id INTEGER,
  status TEXT NOT NULL,
  input_json TEXT NOT NULL,
  state_json TEXT NOT NULL,
  output_json TEXT,
  current_node_id TEXT,
  error TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
```

### workflow_run_events

```sql
CREATE TABLE workflow_run_events (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  run_id TEXT NOT NULL,
  node_id TEXT,
  event_type TEXT NOT NULL,
  payload_json TEXT NOT NULL,
  created_at TEXT NOT NULL
);
```

## 9. 与现有代码的映射

### 现有 Skills Market

当前：

```rust
MainView::SkillsMarket
SkillsMarketState
render_skills_market
Translations::CAPABILITIES
```

建议：

- `MainView::SkillsMarket` 可暂时保留内部名，UI 文案先改为 Skills。
- 后续再重命名为 `MainView::Skills`。
- 避免一次性大改 UI enum 和历史文案。

### 新增 Capabilities view

新增：

```rust
pub(crate) enum MainView {
    Chat,
    SkillsMarket,
    Capabilities,
}
```

Capabilities view 内部：

```rust
enum CapabilitiesTab {
    Library,
    Workflows,
}
```

### MainAgent 调用能力

MainAgent system prompt 中新增：

```text
当用户请求一个已发布能力可以完成的任务时，优先调用 run_capability。
```

当前第一步落地采用无 DB 迁移方案：从本地 manifest 目录读取已发布能力，只有存在 enabled capability 时才向 MainAgent 注册 `run_capability`。

manifest 目录：

```text
~/Library/Application Support/one/capabilities/*.json  # macOS data_dir
~/.one/capabilities/*.json                             # fallback
```

manifest 示例：

```json
{
  "id": "research_brief",
  "name": "Research Brief",
  "description": "生成结构化调研简报",
  "workflow_id": "workflow.research_brief",
  "workflow_version": 1,
  "enabled": true,
  "input_schema": {
    "type": "object",
    "properties": {
      "topic": { "type": "string" },
      "audience": { "type": "string" }
    },
    "required": ["topic"]
  },
  "output_schema": {
    "type": "object",
    "properties": {
      "markdown": { "type": "string" }
    }
  }
}
```

ToolRegistry 注册：

```text
run_capability
```

参数：

```json
{
  "capability_id": "research.brief",
  "input": {
    "topic": "..."
  }
}
```

### Workflow tab 调试

调试运行不直接变成 Capability。

流程：

```text
Draft Workflow
  -> Debug Run
  -> Fix
  -> Publish
  -> CapabilityManifest
  -> MainAgent 可调用
```

## 10. 实施阶段

### Phase 0：命名与导航清理

目标：不改 runtime，只消除产品概念混淆。

任务：

- 左侧当前“能力”文案改为“技能”。
- Skill Market 标题改为“技能”。
- 新增空的 Capabilities 页面。
- Capabilities 页面显示两个 tab：能力库、工作流。
- 写清空状态文案：
  - 能力库：发布工作流后会出现在这里。
  - 工作流：在这里编排多 Agent 工作流。

验收：

- 用户不会再把 Skill Market 理解为多 Agent 能力。
- 无数据库迁移。
- 无 runtime 改动。

### Phase 1：WorkflowDefinition 与本地存储

目标：可以创建、保存、编辑 draft workflow。

任务：

- 新增 `src/workflows/definition.rs`。
- 新增 DB 表：`workflows`、`workflow_versions`。
- 新增 `WorkflowStore`。
- Capabilities / Workflows tab 支持：
  - 新建 workflow。
  - 编辑名称、描述。
  - 添加 step list。
  - 保存 draft。

暂不做：

- 复杂画布。
- 并行分支。
- Capability 发布。

验收：

- 重启后 draft workflow 仍存在。
- workflow definition JSON 可读、可 migration。

### Phase 2：单 Agent Workflow Runtime

目标：让 workflow 能跑最小闭环。

支持节点：

- Agent。
- Skill。
- Output。

任务：

- 抽出 `AgentRuntime`，承接当前 `Orchestrator` 的单 Agent loop。
- 抽出 `ToolDispatcher`。
- 新增 `WorkflowRuntime`。
- 新增 `workflow_runs`、`workflow_run_events`。
- Workflow tab 可 debug run。

验收：

- 用户创建一个包含 Agent + Skill + Output 的 workflow。
- 可以运行并看到每步事件。
- 运行结果持久化。

### Phase 3：发布为 Capability

目标：workflow 发布后成为可调用能力。

任务：

- 新增 `CapabilityManifest`。
- 新增 `capabilities` 表。
- Workflow tab 增加 Publish。
- Capabilities Library 展示已发布能力。
- 新增 `run_capability` builtin tool。
- MainAgent prompt 注入 capability 列表。

验收：

- 用户能在 Capabilities 页面手动运行能力。
- MainAgent 能通过 `run_capability` 调用能力。
- Capability run 写入 workflow_runs。

### Phase 4：多 Agent 编排

目标：真正支持多个 Agent 共同完成一个 workflow。

任务：

- 新增 AgentDefinition 管理。
- Agent 节点支持选择不同 AgentDefinition。
- 支持 condition。
- 支持 HumanApproval。
- 支持 termination condition。
- 支持 max step / max token / timeout。

验收：

- 可以实现 “Researcher -> Analyst -> Reviewer -> Writer”。
- Reviewer 可要求 Writer 修改。
- 用户能在 HumanApproval 节点确认或驳回。
- 中断后可恢复。

### Phase 5：可视化与能力生态

目标：产品化。

任务：

- Workflow graph visualization。
- Capability 版本管理。
- Capability import/export。
- Capability template gallery。
- Capability run history。
- Capability dependencies view。

## 11. 风险与边界

### 不要把 Capability 做成 Skill

Skill 是原子执行器。Capability 是 workflow 发布产物。

如果把 Capability 做成 Skill，会导致：

- workflow run trace 难以保存。
- 节点状态难以恢复。
- 版本管理混乱。
- 多 agent 编排被压缩成一个黑盒 execute。

### 不要过早做复杂画布

复杂画布成本高，且容易拖慢 runtime 建设。第一阶段用 step list 足够验证产品闭环。

### 不要一次性重命名数据库字段

当前已有 `main_agent_summary` 等字段。为了避免迁移风险，短期保留历史字段名，只在新模型里使用 `workflow_*` / `capability_*` 命名。

### Orchestrator 不要继续膨胀

当前 `Orchestrator` 还在承担单 Agent loop、tool dispatch、事件转换。下一步应拆成：

```text
AgentRuntime
ToolDispatcher
WorkflowRuntime
```

这样多 agent workflow 才不会被旧单 agent loop 绑住。

## 12. 推荐的下一步

最小可落地顺序：

1. UI 文案：当前“能力”改“技能”，新增“能力”页面。
2. 新增 `src/workflows` 模块和 workflow definition 类型。
3. 新增 workflows / workflow_versions 表。
4. Capabilities 页面做空状态 + draft workflow 列表。
5. 抽 `ToolDispatcher`，减少 Orchestrator 负担。
6. 抽 `AgentRuntime`，把当前 Orchestrator 降级为单 Agent runtime。
7. 做最小 `WorkflowRuntime`，支持 Agent -> Skill -> Output。
8. 发布 workflow 为 capability。
9. MainAgent 增加 `run_capability`。

这个顺序的好处是：每一步都能独立验证，不需要一口气做完多 Agent 编排；同时最终结构不会走偏。
