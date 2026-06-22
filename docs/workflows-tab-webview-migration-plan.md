# Workflows Tab 全 WebView 迁移技术方案

## 1. 结论

Workflows tab 的长期形态应调整为：

```text
GPUI Capabilities Shell
└─ Workflows Tab WebView
   ├─ workflow selector / drafts list
   ├─ toolbar: publish / add agent / save / run / AI Copilot
   ├─ copilot brief
   ├─ workflow canvas
   ├─ Agent inspector
   ├─ routing editor
   ├─ JSON advanced editor
   └─ run / publish / validation status

Rust
├─ WorkflowDefinition / WorkflowNode / WorkflowEdge
├─ Agent config 映射
├─ routing policy validation
├─ publish validation
├─ WorkflowStore
├─ WorkflowRuntime
├─ capability manifest / MainAgent run_capability
└─ SQLite
```

WebView 负责完整视图层和编辑体验；Rust 仍然是唯一业务真相。WebView 编辑好的 Agent、连线、路由策略、Prompt、工具配置、输出 schema，都必须通过 IPC 映射回 Rust 的 workflow / agent 实体，由 Rust 校验、保存、发布和运行。

## 2. 当前实现和目标差异

当前实现：

```text
Workflows Tab（GPUI）
├─ GPUI 顶部标题和模板入口
├─ GPUI toolbar
├─ GPUI Copilot brief
├─ WebView canvas
├─ GPUI Agent inspector
├─ GPUI workflow list
├─ GPUI JSON 编辑和运行记录
└─ Rust 保存 / 运行 / 发布
```

目标实现：

```text
Workflows Tab（WebView）
├─ toolbar
├─ workflow selector / draft list
├─ Copilot brief
├─ canvas
├─ Agent inspector
├─ routing editor
├─ JSON advanced editor
├─ template shortcuts
└─ run / publish / validation feedback
```

GPUI 只负责：

- Capabilities 页面外壳和 tab 切换。
- WebView 容器生命周期。
- Toast / 顶层状态同步。
- 调用 Rust 领域服务。

Rust 保留：

- `src/workflows/definition.rs`
- `src/workflows/store.rs`
- `src/workflows/routing_policy.rs`
- `src/workflows/publish_validation.rs`
- `src/workflows/runtime.rs`
- `src/workflows/capability.rs`
- `src/workflows/copilot.rs`

这些不能删除。它们不是旧 UI，而是 workflow 的领域内核。

## 3. 设计原则

1. Rust 是唯一数据真相。

   WebView 可以维护临时编辑状态，但保存、发布、运行前必须提交给 Rust。Rust 返回规范化后的 workflow snapshot。

2. WebView 不直接写数据库。

   WebView 只发 IPC action。Rust 调用 `WorkflowStore` 写入 draft / version / capability manifest。

3. WebView 不绕过校验。

   所有 Agent 更新、edge 更新、routing 更新、JSON 更新，都必须走 Rust 校验。

4. IPC 使用命令/事件模型。

   WebView 发送 command；Rust 返回 ack/error 和最新 snapshot。避免靠多个零散事件猜状态。

5. 分阶段迁移。

   先让 WebView 承接 read-only shell，再逐步迁移 toolbar、Inspector、JSON、列表、运行状态。每个阶段可编译、可回退、可验证。

## 4. IPC 协议

### 4.1 Host -> WebView

```ts
type HostMessage =
  | { type: "workflows:hydrate"; state: WorkflowBuilderState }
  | { type: "workflow:updated"; workflow: WorkflowViewModel; editState: WorkflowEditViewState }
  | { type: "workflow:command_result"; requestId: string; ok: true; state: WorkflowBuilderState; message?: string }
  | { type: "workflow:command_result"; requestId: string; ok: false; error: string; state?: WorkflowBuilderState }
  | { type: "workflow:run_event"; workflowId: string; statuses: Record<string, string>; output?: unknown }
  | { type: "workflow:toast"; level: "info" | "success" | "warning" | "error"; message: string };
```

### 4.2 WebView -> Host

```ts
type WebviewCommand =
  | { type: "workflow:ready" }
  | { type: "workflow:select"; workflowId: string }
  | { type: "workflow:create_from_template"; template: "echo" | "mainagent" | "approval" }
  | { type: "workflow:add_agent"; workflowId: string }
  | { type: "workflow:update_agent"; workflowId: string; nodeId: string; patch: AgentNodePatch }
  | { type: "workflow:create_edge"; workflowId: string; sourceNodeId: string; targetNodeId: string }
  | { type: "workflow:delete_edge"; workflowId: string; edgeId: string }
  | { type: "workflow:update_routing"; workflowId: string; scope: RoutingScope; value: unknown }
  | { type: "workflow:update_json"; workflowId: string; definitionJson: string }
  | { type: "workflow:save"; workflowId: string }
  | { type: "workflow:run"; workflowId: string }
  | { type: "workflow:publish"; workflowId: string }
  | { type: "workflow:copilot_generate"; brief: string; sourceWorkflowId?: string | null }
  | { type: "workflow:canvas_error"; workflowId?: string | null; message: string };
```

每个 mutating command 建议带 `requestId`，Rust 处理完成后返回 `workflow:command_result`，前端据此更新 pending 状态和错误信息。

## 5. ViewModel

WebView 不直接消费完整 `WorkflowDefinition`。Rust 应提供面向 UI 的 `WorkflowBuilderState`：

```ts
interface WorkflowBuilderState {
  workflows: WorkflowSummaryView[];
  selectedWorkflowId: string | null;
  workflow: WorkflowViewModel | null;
  editState: WorkflowEditViewState | null;
  capabilities: CapabilitySummaryView[];
  templates: WorkflowTemplateView[];
  webviewStatus: string;
}
```

`WorkflowViewModel` 包含：

- workflow id/name/description/status/version
- canvas nodes/edges
- selected node
- agent inspector fields
- workflow-level routing
- JSON advanced editor text
- last run statuses

Rust 负责从 `WorkflowDefinition` 转为 ViewModel，也负责从 WebView patch 写回 `WorkflowDefinition`。

## 6. Agent 映射

WebView Agent 编辑器字段：

- 基本：name、description、category、tags、version
- 模型：provider、model、temperature、max_tokens、timeout_seconds
- Prompt：system、instructions
- Tools：skills、mcp_tools、system_tools、coding_runtimes
- Output：format、schema、summarize_with_mainagent
- Settings：retry、timeout_seconds、human_confirmation、permissions
- Routing：node-level routing policy

Rust 映射到：

- `WorkflowNode.name`
- `WorkflowNode.config.description`
- `WorkflowNode.config.metadata`
- `WorkflowNode.config.model`
- `WorkflowNode.config.prompt`
- `WorkflowNode.config.tools`
- `WorkflowNode.config.output`
- `WorkflowNode.config.settings`
- `WorkflowNode.config.routing`

映射函数必须集中在 Rust，避免 WebView 自己复制领域规则。

## 7. 需要迁移的 GPUI UI

从 `src/capabilities.rs` 迁入 WebView：

- `workflow_builder_shell`
- 顶部 `Publish / Add Agent / Save / Run / AI Copilot`
- `workflow_inspector_panel`
- Agent 字段编辑器
- workflow draft list / card
- template shortcuts
- JSON advanced editor入口
- dirty/save failed 状态展示

保留在 Rust，但改为 IPC handler：

- `append_empty_agent_node`
- `create_workflow_edge`
- `delete_workflow_edge`
- `save_workflow_agent_node_update`
- `validate_and_save_workflow_definition`
- `publish_workflow_as_capability`
- `run_workflow_draft`
- `create_workflow_from_copilot_brief`
- `create_workflow_from_template`

## 8. 删除和重命名策略

应删除或重命名的是误导性 UI 代码，不是领域内核。

建议：

- `workflow_canvas_poc` 重命名为 `workflow_builder_webview`。
- `poc_html` 重命名为 `fallback_html` 或删除 POC 文案。
- `WorkflowCanvasEvent` 扩展/替换为 `WorkflowWebviewCommand`。
- GPUI 的旧 Inspector 在 WebView Inspector 验证完成后删除。
- GPUI 的旧 toolbar 在 WebView toolbar 验证完成后删除。
- GPUI 的 workflow list/template gallery 在 WebView 版本验证完成后删除。

不应删除：

- workflow definition/store/runtime/capability/publish validation/routing policy/copilot。
- MainAgent `run_capability` tool。

## 9. 验证策略

每个 Phase 至少跑：

```bash
cargo fmt
cargo check
cargo check --features workflow-webview
npm run typecheck
npm run build
```

涉及业务行为时补跑：

```bash
ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test capabilities::tests
ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test workflows::store
ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test workflows::runtime
ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test publish_validation
ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test workflows::copilot
```

最终收口跑：

```bash
ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test
```

GUI 截图验收仍依赖本机可用显示环境；当前自动执行环境可能无法 `screencapture`。

