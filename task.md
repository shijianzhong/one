# 技能 / 能力 / 工作流落地任务

本文跟踪 `docs/capability-workflow-architecture-plan.md` 的实施进度。执行原则：

- `Skill` 是原子技能。
- `Workflow` 是多 Agent / Skill / MCP / 条件 / 人工确认组成的编排定义。
- `Capability` 是已发布、可调用、可复用的 workflow。
- 不改已有历史数据库字段名，避免无意义迁移。
- 每完成一项实现后更新本文状态。

## Phase 0：产品概念与导航分离

- [x] 左侧当前“能力 / Capabilities”入口改为“技能 / Skills”，继续打开现有 Skill Market。
- [x] Skill Market 标题与空状态文案改为“技能”语义。
- [x] 新增“能力 / Capabilities”主入口。
- [x] 新增 Capabilities 页面，包含“能力库 / 工作流”两个 tab 的空状态。
- [x] 确认无数据库迁移、无 runtime 行为变化。

## Phase 1：MainAgent 调用能力入口

- [x] 新增 `src/workflows` 模块。
- [x] 新增 `CapabilityManifest`。
- [x] 从本地 manifest 目录读取 enabled capabilities。
- [x] 仅在存在已发布能力时注册 `run_capability`。
- [x] 将已发布能力列表注入 MainAgent 上下文。
- [x] 文档补充 manifest 目录与 JSON 示例。
- [x] 将 `run_capability` 接到真实 `WorkflowRuntime`。

## Phase 2：Workflow 定义与存储

- [x] 新增 `WorkflowDefinition` / `WorkflowNode` / `WorkflowEdge` 类型。
- [x] 新增 `WorkflowStatus`、`WorkflowNodeKind`。
- [x] 新增 `WorkflowStore`。
- [x] 新增 `workflows` 表。
- [x] 新增 `workflow_versions` 表。
- [x] 支持创建、保存、读取 draft workflow。
- [x] 为 workflow definition 解析和兼容性补单元测试。

验证：

- `cargo fmt`
- `cargo check`
- `ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test`（89 passed）

## Phase 3：运行时拆分

- [x] 从 `Orchestrator` 抽出 `ToolDispatcher`。
- [x] 从 `Orchestrator` 抽出单 Agent loop 为 `AgentRuntime`。
- [x] 保留 `MainAgent` 作为默认入口 Agent。
- [x] 让当前 Orchestrator 逐步降级为兼容门面或删除。
- [x] 为 ToolDispatcher 覆盖 Builtin / Skill / MCP / runtime bridge 路径测试。

验证：

- `cargo fmt`
- `cargo check`
- `ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test`（95 passed）

## Phase 4：最小 WorkflowRuntime

- [x] 新增 `WorkflowRuntime`。
- [x] 新增 `WorkflowRun` 状态模型。
- [x] 新增 `workflow_runs` 表。
- [x] 新增 `workflow_run_events` 表。
- [x] 支持 Agent 节点。
- [x] 支持 Skill 节点。
- [x] 支持 Output 节点。
- [x] 支持 debug run 并记录事件。
- [x] `run_capability` 调用真实 workflow run。

说明：

- `run_capability` 已在 manifest 命中后读取本地 DB 的 workflow definition 并调用 `WorkflowRuntime`。
- `WorkflowRuntime` 当前不持有 `sqlez::Connection`，避免非 `Send/Sync` DB 连接跨 async `.await` 进入工具调用 future。
- `run_capability` 通过短生命周期 DB 写入记录 `workflow_runs` / `workflow_run_events`，不会让 DB connection 跨 `.await`。
- Agent 节点当前支持 `main` / `mainagent` / `main_agent`，其他 agent id 等 Phase 6 的 `AgentDefinition` 落地后扩展。

验证：

- `cargo fmt`
- `cargo check`
- `ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test`（98 passed）

## Phase 5：发布为 Capability

- [x] Workflow draft 可发布为 immutable workflow version。
- [x] 新增 `capabilities` 表。
- [x] Capability manifest 从 DB 读取，文件 manifest 作为兼容 fallback。
- [x] Capabilities 页面展示已发布能力。
- [x] 支持手动运行 capability。
- [x] MainAgent 根据能力列表稳定选择 `run_capability`。

说明：

- Capabilities 页面当前展示 DB-backed capabilities，并兼容本地文件 manifest fallback。
- 手动运行能力当前是最小可用形态：卡片上的 Run 按钮以空 JSON `{}` 作为输入触发执行。
- MainAgent 上下文现在注入 capability id、workflow id、version、description、input_schema，并明确要求只在任务明确匹配时调用 `run_capability`，且不能编造 `capability_id`。

验证：

- `cargo fmt`
- `cargo check`
- `ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test`（100 passed）

## Phase 6：多 Agent 编排

- [x] 新增 `AgentDefinition`。
- [x] Agent 节点支持选择不同 AgentDefinition。
- [x] 支持 Condition 节点。
- [x] 支持基于 edge condition 的基础路径选择。
- [x] 支持 HumanApproval 节点。
- [x] 支持 graph 执行的 `max_steps` 保护。
- [x] 支持终止条件。
- [x] 支持超时。
- [x] 支持中断后恢复。

说明：

- 当前 `AgentDefinition` 是 runtime-level 定义，工作流 Agent 节点可通过 `config.agent_definition` 或 config 顶层 `system_prompt` / `tool_filter` / `model` 等字段定义自定义 Agent。
- `main` / `mainagent` / `main_agent` 继续走现有 MainAgent，保持主入口命名和行为稳定。
- 暂未新增 Agent DB 表或管理 UI，避免在 workflow builder 未成型前引入过早持久化模型。
- WorkflowRuntime 在 workflow 存在 edges 时按 graph 执行；没有 edges 时保持原线性执行，降低兼容风险。
- Edge condition 当前支持 `true` / `false` / `always` / `default`，以及 `field == value`、`$.field != value` 这类确定性表达式。
- HumanApproval 当前是运行时暂停边界：执行到该节点后返回 `awaiting_human_approval`，不会继续执行后续节点；审批 UI、审批结果写回和恢复执行仍在后续“中断后恢复”任务中。
- Workflow metadata 支持 `termination_condition`，语法复用 edge condition；命中后返回 `status=terminated` 的结果包装并停止执行。
- Workflow metadata 支持 `timeout_ms` / `timeout_secs`，用于限制整个 workflow run 的最长执行时间。
- WorkflowRuntime 新增 `resume_definition`，可从 HumanApproval 暂停节点继续执行；graph workflow 会按暂停节点的 outgoing edge 选择下一步，线性 workflow 会从暂停节点之后继续。
- 当前恢复能力是 runtime-level API，审批 UI、审批结果写入 DB、从历史 run 一键恢复仍归入 Phase 7 产品化。

验证：

- `cargo fmt`
- `cargo check`
- `ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test`（110 passed）

## Phase 7：产品化

- [x] 工作流只读 graph preview。
- [x] 工作流 graph 节点 quick-add。
- [x] 工作流 graph edge condition edit。
- [ ] 工作流可视化 graph editor。
- [x] Workflow list / create / publish 最小 UI。
- [x] Workflow edit UI。
- [x] Capability 手动运行输入表单。
- [x] Capability 版本管理。
- [x] Capability import/export。
- [x] Capability template gallery。
- [x] Capability run history。
- [x] Capability run event details。
- [x] Capability dependency view。
- [x] Capability dependency availability check。
- [x] HumanApproval 审批 UI 与 DB-backed resume。
- [x] HumanApproval 审批备注写入 run event。
- [ ] HumanApproval 审批人权限与多级审批。

说明：

- Workflows tab 现在读取本地 DB draft workflows 并展示列表。
- 支持创建一个示例 HumanApproval workflow draft。
- 支持将 draft workflow 发布为 capability；发布后能力会出现在 Capabilities library。
- 暂未提供完整 workflow 编辑器，示例创建只是为了打通 create/list/publish 的最小闭环。
- Capabilities library 现在展示最近 workflow runs，包括 run id、workflow id、version、status 和错误摘要。
- Run history 当前是最近运行列表，并支持展开单个 run 查看事件详情。
- Capabilities library 的能力卡片现在包含 JSON 输入框，手动 Run 会先解析输入 JSON，解析失败只提示错误，不启动 workflow；解析成功后将输入传给 `run_capability`。
- HumanApproval workflow 执行到审批节点后会记录 `human_approval_requested` 事件并保持 run 为 `running`；Recent runs 中的 running run 提供 Approve / Reject 操作，操作后写入 `human_approval_resolved` 事件并通过 `WorkflowRuntime::resume_definition` 恢复执行，完成后标记 run 为 `succeeded`，失败则标记为 `failed`。
- 当前 HumanApproval UI 是最小产品化入口：审批按钮位于 Recent runs；审批备注会写入 `human_approval_resolved` event payload，并可在 run Details 中追溯；审批人权限和多级审批仍未实现。
- Workflows tab 的 draft workflow 卡片现在提供 Edit 入口，展开后可查看和编辑完整 Workflow JSON；Save JSON 会校验并反序列化为 `WorkflowDefinition`，仅允许保存 draft workflow，再通过 `WorkflowStore::save_draft` 写回 DB。
- Workflow edit UI 当前是 JSON 级编辑器；workflow 卡片已提供只读 graph preview，展示节点、节点类型、线性顺序和 explicit edges；后续仍需叠加真正的可视化 graph editor。
- Graph preview 已支持从卡片上快速追加 Output、HumanApproval、MainAgent 节点；如果 workflow 已经有 explicit edges，会自动追加一条从当前最后节点到新节点的 `always` edge；更完整的节点属性编辑、选中节点插入、边创建/删除和拖拽布局仍归入 graph editor。
- Graph preview 的 edge 列表已支持直接编辑 condition 并保存到 draft workflow；更完整的节点属性编辑、选中节点插入、边创建/删除和拖拽布局仍归入 graph editor。
- Recent runs 中每个 run 现在提供 Details / Hide 操作，可展开查看 `workflow_run_events`，事件 payload 会按 JSON pretty print 展示；HumanApproval 的请求与 resolved 事件也可在这里追踪。
- Capability import/export 现在使用统一的 `CapabilityExportPackage` JSON 包格式，包含 capability manifest 与 workflow definition；能力卡片提供 Export JSON，将包复制到剪贴板；Library 顶部提供 Import JSON 输入框，导入后写入 workflow / workflow_versions / capabilities。
- 当前 import/export 是 JSON 文本与剪贴板级实现，尚未接原生文件选择器；后续文件导入导出可复用同一包格式。
- Capability 版本管理现在支持在能力卡片中展开 Published versions，查看 `workflow_versions` 中的 immutable versions，并将当前 capability 激活到指定版本。
- `run_capability`、`resume_capability_run`、Capability export 已改为优先按 manifest / run 的 `workflow_version` 读取 `workflow_versions`，避免版本切换后仍运行当前 workflow definition。
- Capability dependency view 已接入能力卡片，按当前激活 workflow version 读取定义并展示 Agent / Skill / MCP 依赖，以及 condition / approval / output 节点数量；当前只做依赖可视化，不做依赖可用性和权限校验。
- Capability dependency availability check 已接入依赖视图：本地 skill 会用 registry 做存在性检查；非 MainAgent 的 agent 会标记为需要 inline AgentDefinition；MCP tool 按当前决策标记为 deferred，不做端到端连通。
- Workflows tab 已新增 template gallery，可从 Echo output、MainAgent task、HumanApproval 三类常见模式创建 editable draft workflow；模板只负责创建草稿，仍需编辑/发布后才成为可调用能力。

验证：

- `cargo fmt`
- `cargo check`
- `ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test`（112 passed）

## 当前建议下一步

1. 补齐工作流可视化 graph editor。
2. 增加 HumanApproval 审批人权限、多级审批与更完整的审批详情 UI。
3. 增加 workflow graph editor 的节点/边可视化编辑能力。
