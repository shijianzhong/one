# 多 Agent 工作流编排 WebView 画布技术方案

## 1. 背景与目标

当前 ONE 的「能力」体系已经具备基础数据模型和运行闭环：

- `WorkflowDefinition` 支持 `nodes` / `edges`。
- `WorkflowStore` 支持 draft 保存、发布为 capability、版本化。
- 能力库可以运行已发布能力。
- Workflows tab 目前以模板创建、列表卡片、JSON 编辑为主。

接下来需要把「工作流编排」升级为可视化多 Agent workflow builder，支持用户像截图中的工作流编辑器一样完成：

- 在画布中新增 Agent。
- 点击 Agent 节点编辑该节点内容。
- 通过连线表达多 Agent 协作关系。
- 保存 draft。
- 测试运行 workflow。
- 发布 workflow 为能力。
- 使用 AI Copilot 根据自然语言需求自动生成多 Agent 协作 workflow。

核心原则：

> GPUI 继续作为主应用原生 UI；WebView 只作为 workflow canvas 的局部渲染和交互岛。业务数据、校验、保存、发布、运行仍由 Rust 侧负责。

## 2. 产品边界

### 2.1 当前页面负责什么

能力页面的 Workflows tab 负责编辑「当前 workflow」：

- workflow 元信息：名称、描述、状态。
- workflow 内部节点。
- workflow 内部边。
- workflow 内每个 Agent 节点的实例配置。
- Agent 之间的路由策略和执行调度策略。
- workflow 保存、运行、发布。
- AI Copilot 生成或优化当前 workflow draft。

### 2.2 当前页面不负责什么

不在当前页面管理全局 Agent 模板。

全局 Agent 模板后续应独立放到左侧新导航，例如：

- `Agent`
- `智能体`
- `Agent Templates`

该页面负责创建、编辑、复用全局 Agent 模板。workflow 画布只消费这些模板，或者创建局部 Agent 实例。

### 2.3 Agent 类型边界

workflow 里的 Agent 节点分两类：

1. 局部 Agent 节点

   当前 workflow 内独有，不依赖全局模板。

2. 模板 Agent 节点

   来自全局 Agent 模板库，节点可覆盖部分配置。

第一阶段先实现局部 Agent 节点，后续再接全局模板库。

## 3. 为什么引入 WebView

### 3.1 GPUI 原生实现的短板

GPUI 适合桌面产品 UI、列表、面板、终端、编辑器、表单等。但 workflow canvas 涉及：

- 拖拽节点。
- 端口连接。
- 曲线连线。
- 缩放和平移。
- fit view。
- 节点选择。
- 右键菜单。
- minimap。
- 网格背景。
- 自动布局。
- 复杂命中检测。

如果全部用 GPUI 自研，会快速变成画布引擎开发，短期可做出类似外观，但长期维护成本高。

### 3.2 WebView 适合的部分

WebView 内可使用成熟 Web 生态，例如 React Flow / XYFlow：

- 节点与边模型成熟。
- 交互能力完整。
- 可扩展自定义节点。
- UI 状态和布局计算容易迭代。
- 适合做局部复杂交互画布。

### 3.3 引入边界

WebView 只负责：

- 渲染 workflow canvas。
- 处理节点拖拽、缩放、平移、连线、选择。
- 把用户交互事件通过 IPC 发给 Rust。

WebView 不负责：

- 直接访问数据库。
- 运行系统命令。
- 发布 capability。
- 执行 workflow。
- 访问终端。
- 做权限决策。
- 持久化业务数据。

## 4. 总体架构

```text
GPUI App
  ├─ Capabilities / Workflows Tab
  │   ├─ Top Toolbar
  │   │   ├─ 发布
  │   │   ├─ 添加 Agent
  │   │   ├─ 保存
  │   │   ├─ 运行
  │   │   └─ AI Copilot
  │   ├─ WorkflowCanvasWebView
  │   └─ Node Inspector / Agent 编辑器
  │
  ├─ Rust WorkflowStore
  ├─ Rust WorkflowRuntime
  ├─ Rust Capability Publisher
  └─ SQLite
```

数据流：

```text
SQLite workflows.definition_json
  -> Rust WorkflowStore::load
  -> CanvasModel
  -> WebView render
  -> user interaction
  -> IPC event
  -> Rust validate
  -> WorkflowDefinition update
  -> WorkflowStore::save_draft
  -> WebView refresh
```

## 5. UI 结构设计

### 5.1 Workflows tab 主布局

建议将 Workflows tab 改为三栏/两栏混合：

```text
┌────────────────────────────────────────────────────────────┐
│ 工作流编辑器                         发布 添加Agent 保存 运行 AI Copilot │
├────────────────────────────────────────────────────────────┤
│ workflow title chip · draft                                │
├───────────────────────────────────────────┬────────────────┤
│                                           │ Agent 编辑器     │
│             WebView Canvas                │                │
│                                           │ tabs/forms      │
│                                           │                │
└───────────────────────────────────────────┴────────────────┘
```

当未选中节点时，右侧 Inspector 显示 workflow 元信息：

- workflow 名称
- workflow 描述
- 输入 schema
- 输出 schema
- 保存 / 运行 / 发布状态

当选中 Agent 节点时，右侧 Inspector 显示 Agent 节点实例编辑器。

### 5.2 顶部按钮

顶部按钮语义：

- `发布`：将当前 workflow draft 发布为能力。
- `添加 Agent`：新增一个空的局部 Agent 节点。
- `保存`：保存当前 workflow draft。
- `运行`：测试执行当前 workflow。
- `AI Copilot`：根据自然语言需求生成或优化 workflow draft。

### 5.3 Agent 节点编辑器

点击画布中的每个 Agent 节点后，右侧显示 Agent 编辑器。注意：这里编辑的是 workflow 内节点实例，不是全局 Agent 模板。

建议分 tab：

- `基本`
- `模型`
- `提示词`
- `工具`
- `输出`
- `设置`

字段建议：

基本：

- Agent 名称
- 描述
- 分类
- 标签
- 节点版本

模型：

- 模型
- temperature
- max tokens
- timeout

提示词：

- system prompt
- task instruction
- context injection rules

工具：

- skill 列表
- MCP tool 列表
- system tool 列表
- terminal / coding runtime 权限

输出：

- output schema
- result format
- 是否需要 MainAgent 汇总

设置：

- 执行模式：sequential / parallel
- 路由模式：inherit / sequential / parallel / selector / handoff / graph
- 子 Agent 激活条件：all / any
- 允许重复执行
- 失败重试
- 人工确认
- 超时
- 是否允许写文件

### 5.4 路由策略编辑器

Agent 之间的路由策略必须作为 workflow builder 的核心能力，而不是只在边上写一个文本条件。

参考 AutoGen 的 AgentChat 设计，至少需要覆盖以下模式：

- Round-robin：多个 Agent 按固定顺序轮流发言或执行。
- Selector：由模型或选择函数根据上下文选择下一个 Agent。
- Swarm / Handoff：当前 Agent 主动把任务交接给另一个 Agent 或用户。
- GraphFlow：按有向图执行，支持顺序、并行、条件分支、循环和 join。

当前产品 UI 上需要有两层配置：

1. Workflow 级路由策略

   定义整个 workflow 默认采用哪种调度模型。

2. Agent / Edge 级路由策略

   定义某个 Agent 如何把任务交给子 Agent，或某条边如何被激活。

右侧 Agent 编辑器的「设置」tab 需要包含「路由策略」区块：

- 路由模式：
  - 继承 workflow 默认值
  - 顺序执行
  - 并行执行
  - 模型选择下一个 Agent
  - Agent 自主 handoff
  - 图路由
- 子 Agent 激活：
  - `all`：所有上游完成后执行
  - `any`：任一上游完成后执行
- 候选 Agent：
  - 默认使用下游连接节点
  - 可手动限制候选范围
- 重复执行：
  - 是否允许同一 Agent 连续执行
  - 最大循环次数
- 终止条件：
  - 文本命中
  - 最大消息数
  - 最大运行时间
  - 输出 schema 满足
  - 用户确认

边编辑器需要包含：

- 条件表达式
- 激活组 `activation_group`
- 激活条件 `activation_condition`: `all` / `any`
- 优先级
- 是否可循环

画布上要能看出路由语义：

- Sequential：普通单线。
- Parallel：fan-out 多线，节点 badge 显示 `Parallel`。
- Selector：边或节点显示 `Selector`。
- Handoff：边显示 handoff 标识。
- Conditional：边上显示条件摘要。
- Loop：回边或循环 badge。

## 6. 数据模型设计

### 6.1 当前 WorkflowDefinition

当前已有结构：

```rust
pub struct WorkflowDefinition {
    pub id: String,
    pub name: String,
    pub description: String,
    pub status: WorkflowStatus,
    pub version: i64,
    pub nodes: Vec<WorkflowNode>,
    pub edges: Vec<WorkflowEdge>,
    pub metadata: Value,
}
```

该结构可以继续作为唯一持久化真相。

建议在 `metadata` 中引入 workflow 级 routing 配置，避免第一阶段修改 Rust 结构体导致迁移成本过高：

```json
{
  "routing": {
    "mode": "graph",
    "selector": {
      "type": "model",
      "allow_repeated_agent": false,
      "candidate_policy": "connected_downstream",
      "max_turns": 20
    },
    "termination": {
      "max_steps": 30,
      "max_runtime_seconds": 900,
      "conditions": [
        { "type": "output_schema_satisfied" }
      ]
    }
  }
}
```

长期可以把 routing 提升为强类型字段：

```rust
pub struct WorkflowDefinition {
    pub routing: WorkflowRoutingPolicy,
    ...
}
```

但第一阶段建议保持 `metadata.routing`，减少数据库和兼容性影响。

### 6.2 局部 Agent 节点

新增空 Agent 时，生成 workflow 内局部节点：

```json
{
  "id": "agent_1",
  "name": "新 Agent",
  "kind": "agent",
  "agent_id": "local:agent_1",
  "config": {
    "source": "local",
    "description": "",
    "category": "general",
    "tags": [],
    "model": {
      "name": "",
      "temperature": 0.2,
      "max_tokens": 4096,
      "timeout_seconds": 300
    },
    "prompt": {
      "system": "",
      "instruction": "",
      "context_rules": []
    },
    "tools": {
      "skills": [],
      "mcp_tools": [],
      "system_tools": [],
      "coding_runtimes": []
    },
        "execution": {
          "mode": "sequential",
          "retry": 0,
          "requires_approval": false,
          "timeout_seconds": 300
        },
        "routing": {
          "mode": "inherit",
          "activation": "all",
          "allow_repeated_execution": false,
          "max_loops": 1,
          "handoff_targets": []
        },
        "permissions": {
          "filesystem": "ask",
          "terminal": "ask",
      "network": "ask"
    },
    "output_schema": {}
  }
}
```

### 6.3 路由策略数据模型

建议新增内部概念 `WorkflowRoutingPolicy`，即使第一阶段先存在 JSON 中，代码中也应该用结构化解析，不要散落字符串判断。

```rust
pub enum WorkflowRoutingMode {
    Sequential,
    Parallel,
    Selector,
    Handoff,
    Graph,
}

pub enum ActivationCondition {
    All,
    Any,
}

pub struct WorkflowRoutingPolicy {
    pub mode: WorkflowRoutingMode,
    pub selector: Option<SelectorRoutingConfig>,
    pub termination: TerminationConfig,
}

pub struct NodeRoutingPolicy {
    pub mode: Option<WorkflowRoutingMode>,
    pub activation: ActivationCondition,
    pub allow_repeated_execution: bool,
    pub max_loops: usize,
    pub handoff_targets: Vec<String>,
}

pub struct EdgeRoutingPolicy {
    pub condition: String,
    pub activation_group: Option<String>,
    pub activation_condition: ActivationCondition,
    pub priority: i64,
    pub allow_loop: bool,
}
```

边级策略可先扩展在 `WorkflowEdge.condition` 和 `metadata.edge_routing` 中：

```json
{
  "edge_routing": {
    "edge.main_to_summary": {
      "activation_group": "research_done",
      "activation_condition": "all",
      "priority": 10,
      "allow_loop": false
    }
  }
}
```

后续如果边配置复杂化，再把 `WorkflowEdge` 扩展为强类型字段。

### 6.4 未来全局模板 Agent 节点

后续左侧 Agent 模板页完成后，workflow 节点可以引用模板：

```json
{
  "id": "agent_research_1",
  "name": "研究 Agent",
  "kind": "agent",
  "agent_id": "template:research_agent",
  "config": {
    "source": "template",
    "template_id": "research_agent",
    "overrides": {
      "prompt": {
        "instruction": "只关注 Twitter AI 资讯"
      },
      "tools": {
        "skills": ["agent-reach"]
      }
    }
  }
}
```

### 6.5 发布快照策略

发布 capability 时必须固化 workflow definition 快照。

原因：

- 发布后的能力需要可复现。
- 后续全局 Agent 模板变化不应破坏已发布能力。
- 用户重新运行旧版本能力时应得到同一套节点配置。

策略：

- draft 阶段可以引用 template。
- publish 阶段解析 template + overrides，写入发布版本 definition_json。
- `workflow_versions` 保存不可变版本。
- capability 记录 `workflow_id` 和 `workflow_version`。

## 7. WebView 与 Rust IPC 协议

### 7.1 Rust -> WebView

初始化：

```json
{
  "type": "workflow:load",
  "payload": {
    "workflow": {
      "id": "workflow.twitter_ai_digest",
      "name": "Twitter AI资讯抓取与邮件推送工作流",
      "status": "draft",
      "version": 1
    },
    "nodes": [],
    "edges": [],
    "selection": {
      "node_id": null
    }
  }
}
```

刷新节点：

```json
{
  "type": "workflow:patch",
  "payload": {
    "nodes": [],
    "edges": []
  }
}
```

错误提示：

```json
{
  "type": "workflow:error",
  "payload": {
    "message": "edge target node not found"
  }
}
```

### 7.2 WebView -> Rust

选择节点：

```json
{
  "type": "node:selected",
  "payload": {
    "node_id": "agent_1"
  }
}
```

节点位置变化：

```json
{
  "type": "node:position_changed",
  "payload": {
    "node_id": "agent_1",
    "position": { "x": 480, "y": 260 }
  }
}
```

新增边：

```json
{
  "type": "edge:created",
  "payload": {
    "from_node_id": "agent_1",
    "to_node_id": "agent_2",
    "condition": "always"
  }
}
```

删除节点：

```json
{
  "type": "node:deleted",
  "payload": {
    "node_id": "agent_1"
  }
}
```

删除边：

```json
{
  "type": "edge:deleted",
  "payload": {
    "edge_id": "agent_1_to_agent_2"
  }
}
```

请求保存：

```json
{
  "type": "workflow:save_requested",
  "payload": {}
}
```

## 8. AI Copilot 设计

### 8.1 目标

AI Copilot 根据用户自然语言需求生成多 Agent workflow draft。

示例：

> 帮我做一个每天抓取 Twitter AI 资讯，总结重点，并通过邮件推送给我的能力。

生成内容：

- workflow 名称
- workflow 描述
- Agent 节点列表
- 每个 Agent 的职责
- 每个 Agent 的 prompt
- 每个 Agent 的工具需求
- 节点之间的边
- 并行/串行策略
- 输入 schema
- 输出 schema

### 8.2 交互

```text
点击 AI Copilot
  -> 输入需求
  -> MainAgent / WorkflowDesignerAgent 生成 WorkflowDefinition draft
  -> Rust 校验
  -> 保存为 draft
  -> WebView 展示
  -> 用户继续编辑
```

AI Copilot 生成结果不直接发布。

### 8.3 专用 Agent

建议新增内部 `WorkflowDesignerAgent`，职责是把自然语言需求转换成 `WorkflowDefinition`。

输入：

- 用户需求
- 当前 workflow draft
- 可用内置 Agent
- 可用 skills
- 可用 MCP tools
- 项目权限约束

输出必须是结构化 JSON：

```json
{
  "workflow": {
    "id": "workflow.twitter_ai_digest",
    "name": "Twitter AI资讯抓取与邮件推送工作流",
    "description": "抓取 Twitter AI 动态，总结重点，并邮件发送。"
  },
  "nodes": [],
  "edges": []
}
```

Rust 侧必须进行 schema 校验和业务校验，不能直接信任模型输出。

### 8.4 AI Copilot 必须生成路由策略

AI Copilot 不能只生成 nodes 和 edges，还必须生成 routing policy。

生成时需要明确：

- 整个 workflow 的默认路由模式。
- 哪些 Agent 并行。
- 哪些 Agent 顺序执行。
- 哪些 Agent 由 selector 决定。
- 哪些 Agent 可以 handoff。
- join 节点使用 `all` 还是 `any`。
- 终止条件。
- 每条条件边的 condition。

生成结果示例：

```json
{
  "workflow": {
    "id": "workflow.twitter_ai_digest",
    "name": "Twitter AI资讯抓取与邮件推送工作流",
    "description": "抓取 Twitter AI 动态，总结重点，并邮件发送。",
    "metadata": {
      "routing": {
        "mode": "graph",
        "termination": {
          "max_steps": 20,
          "conditions": [
            { "type": "output_schema_satisfied" }
          ]
        }
      }
    }
  },
  "nodes": [
    {
      "id": "agent_fetch_twitter",
      "name": "Twitter抓取代理",
      "kind": "agent",
      "agent_id": "local:agent_fetch_twitter",
      "config": {
        "routing": {
          "mode": "parallel",
          "activation": "all"
        }
      }
    }
  ],
  "edges": [
    {
      "id": "main_to_fetch",
      "from_node_id": "agent_main",
      "to_node_id": "agent_fetch_twitter",
      "condition": "always"
    }
  ]
}
```

AI Copilot 的结果必须先进入 draft，用户确认后才能保存或发布。

## 9. 添加 Agent 设计

点击 `添加 Agent`：

1. Rust 创建一个空局部 Agent 节点。
2. 生成唯一 node id，例如 `agent_1`。
3. 默认放置到画布可见区域中心或最后选中节点旁边。
4. 保存到 workflow draft。
5. 通知 WebView 刷新。
6. 自动选中新节点。
7. 右侧打开 Agent 编辑器。

默认节点：

```json
{
  "id": "agent_1",
  "name": "新 Agent",
  "kind": "agent",
  "agent_id": "local:agent_1",
  "config": {
    "source": "local",
    "description": "",
    "prompt": {
      "system": "",
      "instruction": ""
    },
    "tools": [],
    "execution": {
      "mode": "sequential"
    }
  }
}
```

## 10. 保存与校验

### 10.1 保存触发

保存可以由三类动作触发：

- 用户点击 `保存`。
- Inspector 表单保存。
- WebView 发生结构性编辑后自动标记 dirty，用户保存。

建议第一阶段使用显式保存，避免频繁写库和难以追踪状态。

### 10.2 校验规则

保存前 Rust 校验：

- workflow id 非空。
- workflow name 非空。
- node id 唯一。
- edge id 唯一。
- edge 的 from/to 节点存在。
- 不允许 self-loop，除非后续明确支持。
- routing mode 必须是已知枚举。
- activation condition 必须是 `all` 或 `any`。
- selector 候选 Agent 必须存在。
- handoff target 必须存在并且在允许列表内。
- loop 必须配置 max loops。
- Agent 节点必须有 agent_id。
- local Agent 的 config.source 必须是 `local`。
- template Agent 的 config.source 必须是 `template`。
- output schema 必须是合法 JSON object。
- 工具引用必须可解析，找不到时给 warning，不直接阻塞 draft 保存。

发布前校验更严格：

- 至少一个节点。
- 至少一个输出节点或可推导最终输出。
- 所有必须工具可用。
- 所有 required prompt 字段完整。
- 不允许未配置的 local Agent。
- 不允许断开的必要节点。
- selector 模式必须有可用 selector 模型或 deterministic fallback。
- graph 模式不能存在无法终止的循环。
- parallel join 节点必须有明确 activation condition。

## 11. 运行测试

点击 `运行`：

1. 保存当前 draft。
2. 创建临时 workflow run。
3. 用当前 draft definition 执行，不要求先发布。
4. 运行状态显示在 UI。
5. 节点级状态回传给 WebView：
   - pending
   - running
   - waiting_user
   - succeeded
   - failed
6. WebView 节点用边框、badge、状态点展示运行状态。

运行事件结构：

```json
{
  "type": "node:runtime_status",
  "payload": {
    "node_id": "agent_1",
    "status": "running",
    "message": "正在抓取 Twitter AI 资讯"
  }
}
```

### 11.1 路由调度运行时

当前 workflow runtime 需要从「按节点列表执行」升级为「按 routing policy 调度」。

建议新增 `WorkflowScheduler`：

```text
WorkflowRuntime
  -> WorkflowScheduler
       -> GraphExecutionState
       -> RouteDecision
       -> NodeRunner
```

职责拆分：

- `WorkflowRuntime`：负责 run 生命周期、事件记录、错误处理。
- `WorkflowScheduler`：根据 routing policy 决定下一批可执行节点。
- `NodeRunner`：执行具体节点，调用 Agent / Skill / MCP / HumanApproval / Output。
- `GraphExecutionState`：记录节点状态、输入输出、已执行次数、等待条件。

节点状态：

- `pending`
- `ready`
- `running`
- `waiting_user`
- `succeeded`
- `failed`
- `skipped`

调度策略：

1. Sequential

   按边或节点顺序一次只执行一个 Agent。

2. Parallel

   同一层级所有 ready 节点并发执行。join 节点根据 `activation_condition` 判断：

   - `all`：所有上游 succeeded 后 ready。
   - `any`：任意上游 succeeded 后 ready。

3. Selector

   运行时把当前上下文、候选 Agent、历史输出交给 selector。

   selector 可以是：

   - rule selector
   - model selector
   - custom function selector

   第一阶段推荐只实现 model selector 和 deterministic fallback。

4. Handoff

   当前 Agent 的输出中可以声明 handoff：

   ```json
   {
     "handoff_to": "agent_summary",
     "reason": "抓取完成，需要总结"
   }
   ```

   Rust 侧校验目标必须在允许 handoff_targets 中。

5. Graph

   默认有向图执行模式。边的 condition 决定下游是否 ready。

终止条件：

- 所有可达节点完成。
- output node 完成。
- selector 返回 stop。
- 达到 max steps。
- 达到 max runtime。
- 人工终止。
- 输出 schema 满足。

### 11.2 AutoGen 参考映射

AutoGen AgentChat 的团队模式可以映射为 ONE 的 routing policy：

| AutoGen 概念 | ONE routing policy | 用途 |
|---|---|---|
| RoundRobinGroupChat | `mode: sequential` 或 `mode: round_robin` | 固定顺序轮流执行 |
| SelectorGroupChat | `mode: selector` | 由模型/函数选择下一个 Agent |
| Swarm / Handoff | `mode: handoff` | Agent 自主交接任务 |
| GraphFlow | `mode: graph` | 显式 DAG / 条件图执行 |

ONE 不应照搬 AutoGen API，而应吸收它的协作模式，把这些模式固化为可视化 workflow 的调度策略。

## 12. 发布为能力

点击 `发布`：

1. 先执行发布前校验。
2. 生成 capability id。
3. 固化 workflow version。
4. 写入 `workflow_versions`。
5. 写入或更新 capability manifest。
6. 能力库中可被 MainAgent 调用。

发布后：

- 当前 workflow status 变为 `published`。
- 若继续编辑，应创建新 draft version 或让用户明确进入编辑草稿。

## 13. MainAgent 调用能力

已发布 capability 应继续由现有能力系统暴露给 MainAgent：

- MainAgent 在 prompt 中看到已发布能力列表。
- 用户需求明确匹配能力时，MainAgent 调用 `run_capability`。
- `run_capability` 使用 capability 指向的 workflow version。
- workflow runtime 执行固定版本。

编排器只负责生产 capability，不改变 MainAgent 调用能力的主路径。

## 14. 技术选型

### 14.1 Rust WebView

候选：

- `wry`
- 平台原生 WKWebView 封装
- GPUI 自定义平台嵌入

优先调研 `wry` 是否能稳定嵌入 GPUI window。若 GPUI 没有稳定 child webview 宿主能力，需要评估：

- 是否使用独立浮层窗口。
- 是否在 macOS 原生层挂载 WKWebView。
- 是否先做 WebView POC，再决定正式方案。

### 14.2 Web Canvas

候选：

- `@xyflow/react`
- React + Vite
- TypeScript

第一阶段推荐：

- Vite
- React
- @xyflow/react
- Zustand 或 React state

构建产物打包到应用资源目录，由 Rust 加载本地 HTML。

## 15. 目录结构建议

```text
src/
  capabilities.rs
  workflow_canvas/
    mod.rs
    ipc.rs
    model.rs
    validation.rs
    webview.rs

web/workflow-canvas/
  package.json
  vite.config.ts
  src/
    main.tsx
    App.tsx
    canvas/
      WorkflowCanvas.tsx
      AgentNode.tsx
      nodeTypes.ts
      edges.ts
    ipc/
      bridge.ts
      messages.ts
    styles.css

assets/workflow-canvas/
  index.html
  assets/*.js
  assets/*.css
```

## 16. 迭代计划

### Phase 0：方案确认

目标：

- 确认 WebView 只作为 canvas。
- 确认右侧 Inspector 用 GPUI 原生。
- 确认全局 Agent 模板后续单独做。

产出：

- 本方案文档。
- 任务拆分文档。

### Phase 1：WebView POC

目标：

- 在 Workflows tab 中嵌入一个 WebView。
- 加载本地 HTML。
- 显示静态 workflow canvas。
- Rust 能发送 workflow JSON 给 WebView。
- WebView 能发送 `node:selected` 给 Rust。

验收：

- 页面可打开。
- 画布不闪退。
- 点击节点 Rust 能收到事件并打日志。

### Phase 2：Canvas 数据接入

目标：

- 从 `WorkflowDefinition` 转换成 WebView canvas model。
- WebView 渲染真实 nodes/edges。
- 支持 pan/zoom/fit view。
- 支持节点选中。

验收：

- 现有 draft workflow 能在画布显示。
- 节点标题、描述、执行模式、类型 badge 正确。
- 边正确显示。

### Phase 3：添加空 Agent

目标：

- 顶部 `添加 Agent` 创建局部 Agent 节点。
- 新节点保存到 draft。
- WebView 刷新并选中新节点。
- 右侧 Inspector 打开 Agent 编辑器。

验收：

- 不依赖全局 Agent 模板。
- 新节点有完整默认 config。
- 保存后重开页面仍存在。

### Phase 4：Agent 节点实例编辑

目标：

- 点击 Agent 节点编辑当前 workflow 内节点实例。
- 支持基本、模型、提示词、工具、输出、设置。
- 保存写回 `WorkflowDefinition.nodes[].config`。
- WebView 节点标题和描述随保存刷新。

验收：

- 编辑不会影响其他 workflow。
- 编辑不会影响未来全局 Agent 模板。
- JSON 存储合法。

### Phase 5：连线编辑

目标：

- WebView 支持创建 edge。
- WebView 支持删除 edge。
- Rust 侧校验 edge。
- 保存到 `WorkflowDefinition.edges`。

验收：

- from/to 节点存在。
- 无非法 self-loop。
- 保存后重开仍存在。

### Phase 6：路由策略编辑

目标：

- 支持 workflow 级 routing policy。
- 支持 Agent 节点级 routing policy。
- 支持边级 routing policy。
- 右侧 Inspector 可编辑 sequential / parallel / selector / handoff / graph。
- 画布用 badge 和边样式展示路由语义。

验收：

- 路由配置保存到 `WorkflowDefinition.metadata` 或 node config。
- 保存后重开仍存在。
- 非法 routing 配置被 Rust 校验拦截。

### Phase 7：保存、Dirty 状态、错误处理

目标：

- 显示 dirty 状态。
- 保存成功/失败 toast。
- WebView 和 Rust 错误同步。
- 导航离开前提示未保存。

验收：

- 用户不会误以为已保存。
- IPC 异常不会导致应用崩溃。

### Phase 8：运行测试

目标：

- `运行` 当前 draft。
- 节点状态回传 WebView。
- waiting_user 状态能提示 MainAgent 或 UI。
- runtime 根据 routing policy 调度节点。

验收：

- 测试运行不需要发布。
- 节点状态可视化。
- 失败信息可追踪。
- sequential / parallel / graph 三类基础策略可执行。

### Phase 9：发布能力

目标：

- 发布前校验。
- 固化 workflow version。
- 生成/更新 capability。
- 能力库可见。
- MainAgent 可调用。

验收：

- 发布后能力可运行。
- 修改 draft 不影响已发布版本。

### Phase 10：AI Copilot

目标：

- 输入自然语言需求。
- 生成 WorkflowDefinition draft。
- 生成多 Agent 节点和边。
- 生成 routing policy。
- 生成节点 prompt、工具建议、输出 schema。
- 用户可继续编辑。

验收：

- AI 生成结果必须先进入 draft。
- Rust 校验失败时给出可读错误。
- 不直接发布。

### Phase 11：全局 Agent 模板接入

前置：

- 左侧新增 Agent 模板导航。
- 全局模板 CRUD 完成。

目标：

- 添加 Agent 时可选择全局模板。
- workflow 节点可引用 template。
- 支持 overrides。
- 发布时固化模板快照。

验收：

- 修改全局模板不影响已发布 capability。
- 当前 workflow 可选择是否同步模板更新。

## 17. 风险与对策

### 17.1 GPUI 嵌 WebView 风险

风险：

- GPUI 可能没有稳定 child WebView 宿主。
- macOS / Linux 行为差异较大。

对策：

- 先做 Phase 1 POC。
- POC 不通过时，退回 GPUI 原生简化画布，或者使用独立编辑窗口。

### 17.2 WebView 安全风险

风险：

- WebView 获得过多能力会扩大攻击面。

对策：

- 只加载本地打包资源。
- 禁止远程 URL。
- 禁止 WebView 直接访问文件系统和数据库。
- 所有写操作必须经过 Rust IPC 校验。

### 17.3 数据一致性风险

风险：

- WebView state 和 Rust state 不一致。

对策：

- Rust 是唯一数据真相。
- WebView 每次保存后以 Rust 返回结果刷新。
- 所有节点/边 id 由 Rust 生成或校验。

### 17.4 AI Copilot 生成错误

风险：

- 模型输出不合法 JSON。
- 工具引用不存在。
- workflow 结构不可执行。

对策：

- 强 schema 校验。
- 生成后进入 draft，不直接发布。
- 给用户显示修复建议。
- 允许 AI Copilot 基于校验错误二次修复。

## 18. 测试策略

Rust 单元测试：

- WorkflowDefinition 校验。
- 添加 Agent 默认 config。
- 保存节点配置。
- 创建/删除边。
- 发布前校验。
- 发布快照不受 draft 修改影响。

Web 测试：

- Canvas 渲染 nodes/edges。
- 点击节点发送 `node:selected`。
- 拖拽节点发送 position change。
- 连线发送 edge create。

集成测试：

- Rust -> WebView load。
- WebView -> Rust selected。
- 添加 Agent -> 保存 -> reload。
- AI Copilot 生成 -> 校验 -> 展示。

人工验收：

- 常见 laptop 宽度下可用。
- 右侧 Inspector 不遮挡画布。
- 节点文本不溢出。
- 大 workflow 可 pan/zoom。

## 19. 最终判断

引入 WebView 是合理的，但必须是局部引入。

推荐路线：

- GPUI 保持主应用。
- WebView 只用于 workflow canvas。
- 右侧 Inspector 继续 GPUI 原生。
- Rust 保持业务真相和安全边界。
- 第一阶段先 POC 验证嵌入能力，再正式实现。

这样可以用 Web 生态解决复杂画布交互，同时不破坏 ONE 当前原生桌面应用的整体架构。
