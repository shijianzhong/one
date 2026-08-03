# Workflows Tab 全 WebView 迁移任务清单

关联方案文档：[docs/workflows-tab-webview-migration-plan.md](../docs/workflows-tab-webview-migration-plan.md)

## 状态约定

- `[ ]` 未开始
- `[~]` 进行中
- `[x]` 已完成
- `[!]` 阻塞
- `[>]` 暂缓

## 执行规则

- 每个 Phase 开始前将任务标为 `[~]`。
- 每个 Phase 完成后必须自测并记录验证结果。
- WebView 负责 Workflows tab 视图层；Rust 负责 workflow/agent 实体、校验、保存、发布、运行。
- 不删除 Rust workflow 领域内核。
- 迁移完成后删除旧 GPUI Workflows tab 视图代码，避免后续理解混乱。

## 总体进度

- 当前阶段：完成
- 当前状态：Phase 9 自动验收已完成；GUI 截图验收暂缓
- 最近验证：最终 Rust/Web 编译、Web 前端构建、全量 Rust 测试通过

## Phase 0：边界确认与文档

目标：明确“整个 Workflows tab WebView”新目标，避免继续沿用局部画布方案。

- [x] 形成技术方案文档。
- [x] 形成本任务清单。
- [x] 明确 WebView 是 Workflows tab 完整视图层。
- [x] 明确 Rust 仍是唯一业务真相。
- [x] 明确 WebView 编辑 Agent 后映射到 Rust `WorkflowNode` / `WorkflowDefinition`。
- [x] 明确不删除 Rust workflow runtime/store/publish validation/routing policy/capability/copilot。

验收：

- [x] 文档能区分 UI 迁移和领域内核保留。
- [x] 后续 Phase 可按文档执行。

验证：

- [x] 文档检查完成：`docs/workflows-tab-webview-migration-plan.md` 和本任务文档已覆盖 WebView 视图层、Rust 领域真相、IPC、ViewModel、Agent 映射、删除边界和分阶段验证。

## Phase 1：IPC 协议和命名收敛

目标：从 canvas-only IPC 升级为 workflow builder IPC。

- [x] 将 `workflow_canvas_poc` 重命名为 `workflow_builder_webview`。
- [x] 将 POC/fallback 文案从主路径移除或重命名为 fallback。
- [x] 定义 Rust `WorkflowWebviewCommand`。
- [x] 定义 Rust `WorkflowBuilderHostMessage`。
- [x] Web 前端定义对应 TypeScript 类型。
- [x] 保持现有 canvas load/select/edge create/delete 兼容。
- [x] 增加 requestId command result 协议。

验收：

- [x] WebView 仍能加载现有 canvas。
- [x] Rust 能收到新 command 类型。
- [x] 旧 POC 命名不再误导主路径。

验证：

- [x] `cargo fmt`
- [x] `cargo check`
- [x] `cargo check --features workflow-webview`
- [x] `npm run typecheck`
- [x] `npm run build`
- [x] `ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test workflow_webview`

结果记录：

- [x] `src/workflow_webview.rs` 主入口改为 `workflow_builder_webview`，元素 ID 改为 `workflow-builder-webview`。
- [x] IPC 解析类型从 canvas-only `CanvasIpcMessage` 收敛为 `WorkflowWebviewCommand`，现有 node select / edge create / edge delete / error 消息兼容，并新增可选 `requestId`。
- [x] Host -> WebView 新增 `WorkflowBuilderHostMessage`，当前 `workflow:load` 已通过该类型序列化，后续可扩展 hydrate / command result。
- [x] Web 前端类型新增 `workflow:command_result` 和 requestId 字段，`subscribeHostMessages` 保持 load 消息收窄。
- [x] 旧 POC 文案改为 builder/fallback 语义，避免误解为临时主路径。

## Phase 2：WorkflowBuilderState ViewModel

目标：Rust 一次性向 WebView hydrate 完整 Workflows tab 状态。

- [x] 新增 Rust `WorkflowBuilderState`。
- [x] 新增 workflow summaries view。
- [x] 新增 selected workflow view。
- [x] 新增 edit state view：saved/dirty/save_failed。
- [x] 新增 templates view。
- [x] 新增 run statuses view。
- [x] WebView 支持 `workflows:hydrate`。
- [x] WebView 显示 workflow selector/list 的只读版本。

验收：

- [x] 打开 Workflows tab 后 WebView 能看到 workflow 列表和当前 workflow。
- [x] 不再依赖 GPUI 在 tab 下方渲染 workflow card 列表。

验证：

- [x] `cargo fmt`
- [x] `cargo check`
- [x] `cargo check --features workflow-webview`
- [x] `npm run typecheck`
- [x] `npm run build`
- [x] `ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test workflow_webview`

结果记录：

- [x] `src/workflow_webview.rs` 新增 `WorkflowBuilderState` / `WorkflowSummaryView` / `WorkflowEditStateView` / `WorkflowTemplateView`，并通过 `workflows:hydrate` 向 WebView 注入完整 tab 只读状态。
- [x] `src/capabilities.rs` 生成 builder state，包含 draft workflow summaries、当前 workflow canvas snapshot、edit state、templates 与当前 workflow node run statuses。
- [x] `web/workflow-canvas/src/types.ts` / `bridge.ts` 支持 `workflows:hydrate`。
- [x] `web/workflow-canvas/src/App.tsx` 渲染只读 drafts、templates、run status sidebar，同时保留现有 React Flow canvas 交互。
- [x] `web/workflow-canvas/src/styles.css` 补齐全 tab WebView 布局和暗色模式样式。

## Phase 3：WebView Toolbar

目标：把 Publish/Add Agent/Save/Run/AI Copilot 入口迁移进 WebView。

- [x] WebView 渲染 toolbar。
- [x] WebView 发送 `workflow:add_agent`。
- [x] Rust 复用 `append_empty_agent_node` 并返回 snapshot。
- [x] WebView 发送 `workflow:save`。
- [x] Rust 复用 `validate_and_save_workflow_definition`。
- [x] WebView 发送 `workflow:run`。
- [x] Rust 复用 `run_workflow_draft` 并推送 run statuses。
- [x] WebView 发送 `workflow:publish`。
- [x] Rust 复用 `publish_workflow_as_capability`。
- [x] WebView 发送 `workflow:copilot_generate`。
- [x] Rust 复用 `create_workflow_from_copilot_brief`。
- [x] GPUI toolbar 保留临时 fallback，验证后删除。

验收：

- [x] WebView toolbar 可完成新增 Agent、保存、运行、发布、Copilot 生成。
- [x] 失败时 WebView 显示错误，Rust 不写非法状态。

验证：

- [x] `cargo fmt`
- [x] `cargo check`
- [x] `cargo check --features workflow-webview`
- [x] `npm run typecheck`
- [x] `npm run build`
- [x] `ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test capabilities::tests`

结果记录：

- [x] `src/workflow_webview.rs` 新增 `workflow:add_agent` / `workflow:save` / `workflow:run` / `workflow:publish` / `workflow:copilot_generate` 命令解析，并通过统一 WebView event 队列进入 Rust。
- [x] `src/capabilities.rs` 复用现有 workflow helper 处理 WebView toolbar 命令，执行后依赖 `cx.notify()` 重新 hydrate snapshot。
- [x] `web/workflow-canvas/src/App.tsx` 渲染 Add Agent、Save、Run、Publish 和 AI Copilot 入口。
- [x] `web/workflow-canvas/src/types.ts` 扩展 toolbar command 类型。
- [x] `web/workflow-canvas/src/styles.css` 补齐 toolbar / Copilot 输入区样式。

## Phase 4：WebView Agent Inspector

目标：把 Agent 节点编辑表单迁移进 WebView。

- [x] WebView 支持选中 Agent 后显示 Inspector。
- [x] WebView 支持 Basic 字段编辑。
- [x] WebView 支持 Model 字段编辑。
- [x] WebView 支持 Prompt 字段编辑。
- [x] WebView 支持 Tools 字段编辑。
- [x] WebView 支持 Output 字段编辑。
- [x] WebView 支持 Settings/Routing 字段编辑。
- [x] WebView 发送 `workflow:update_agent`。
- [x] Rust 复用并收敛 `save_workflow_agent_node_update` 映射逻辑。
- [x] Rust 返回规范化后的 workflow snapshot。
- [x] 删除 GPUI Agent Inspector。已在 Phase 8 统一删除 fallback。

验收：

- [x] WebView 编辑 Agent 后，Rust `WorkflowDefinition.nodes[].config` 正确更新。
- [x] 非法 JSON / 数值 / routing 被 Rust 拦截。
- [x] 修改只影响当前 workflow 当前节点。

验证：

- [x] `cargo fmt`
- [x] `cargo check`
- [x] `cargo check --features workflow-webview`
- [x] `npm run typecheck`
- [x] `npm run build`
- [x] `ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test capabilities::tests`
- [x] 新增/保留 Agent update 映射测试。

结果记录：

- [x] `src/workflow_webview.rs` 新增 `WorkflowAgentUpdateView` / `WorkflowAgentInspectorView`，`WorkflowBuilderState` hydrate 当前选中 Agent 的可编辑表单数据。
- [x] `src/workflow_webview.rs` 新增 `workflow:update_agent` IPC 命令并进入统一 WebView event 队列。
- [x] `src/capabilities.rs` 根据当前选中 Agent 生成 `selectedAgent` ViewModel，字段默认值与旧 GPUI Inspector 保持一致。
- [x] `src/capabilities.rs` 将 WebView `WorkflowAgentUpdateView` 转为内部 `WorkflowAgentNodeUpdate`，继续复用 Rust 侧 JSON / 数值 / routing 校验与保存逻辑。
- [x] `web/workflow-canvas/src/App.tsx` 渲染右侧 Agent Inspector 表单并提交 `workflow:update_agent`。
- [x] `web/workflow-canvas/src/styles.css` 升级为 sidebar / canvas / inspector 三栏布局并补齐表单样式。

## Phase 5：WebView Workflow List 与模板入口

目标：把 workflow draft list 和模板 shortcut 迁移进 WebView。

- [x] WebView 显示 draft workflow list。
- [x] WebView 支持选择 workflow。
- [x] WebView 支持模板 shortcut。
- [x] Rust 复用 `create_workflow_from_template`。
- [x] 删除 GPUI workflow card list。
- [x] 删除 GPUI template gallery。

验收：

- [x] 用户能在 WebView 内切换 workflow。
- [x] 创建模板 workflow 后自动选中并刷新。

验证：

- [x] `cargo fmt`
- [x] `cargo check`
- [x] `cargo check --features workflow-webview`
- [x] `npm run typecheck`
- [x] `npm run build`

结果记录：

- [x] `src/workflow_webview.rs` 新增 `workflow:select` 与 `workflow:create_from_template` IPC 命令。
- [x] `src/capabilities.rs` 复用 `create_workflow_from_template` 创建模板 draft，并选中新 workflow。
- [x] `src/capabilities.rs` 从 Workflows tab 主渲染路径移除旧 GPUI workflow card list 和 template gallery。
- [x] `web/workflow-canvas/src/App.tsx` 为 draft list 和 template list 挂接真实命令。
- [x] `web/workflow-canvas/src/types.ts` 扩展 workflow 选择与模板创建命令类型。

## Phase 6：WebView JSON Advanced Editor

目标：把 JSON 高级编辑迁移进 WebView，但校验仍由 Rust 做。

- [x] WebView 显示当前 workflow JSON。
- [x] WebView 支持 JSON 编辑。
- [x] WebView 发送 `workflow:update_json`。
- [x] Rust 解析为 `WorkflowDefinition`。
- [x] Rust 执行 routing validation。
- [x] Rust 保存 draft 或返回错误。
- [x] 删除 GPUI JSON 编辑入口。已在 Phase 8 统一删除 fallback。

验收：

- [x] 合法 JSON 可保存并刷新画布。
- [x] 非法 JSON 或非法 routing 不污染数据库。

验证：

- [x] `cargo fmt`
- [x] `cargo check`
- [x] `cargo check --features workflow-webview`
- [x] `npm run typecheck`
- [x] `npm run build`
- [x] `ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test capabilities::tests`
- [x] `ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test workflows::store`

结果记录：

- [x] `src/workflow_webview.rs` 新增 `workflow:update_json` IPC 命令和 `WorkflowBuilderState.workflowJson` hydrate 字段。
- [x] `src/capabilities.rs` 复用 `save_workflow_definition_json` 处理 WebView JSON 保存，继续由 Rust 执行解析、routing validation 和 draft 保存。
- [x] `web/workflow-canvas/src/App.tsx` 在右侧 inspector 增加 Advanced JSON 编辑区，保存时发送当前 workflow JSON。
- [x] `web/workflow-canvas/src/types.ts` 补齐 JSON 更新消息类型。
- [x] `web/workflow-canvas/src/styles.css` 补齐 JSON 编辑器样式。

## Phase 7：状态、错误和运行事件闭环

目标：WebView 内完整展示 dirty/save/run/publish/validation 状态。

- [x] WebView 显示 saved/dirty/save_failed。
- [x] WebView 显示 command pending。
- [x] WebView 显示 command error。
- [x] WebView 显示 run statuses。
- [x] WebView 显示 publish success/failure。
- [x] Rust 通过 hydrate 推送 workflow activity/run status。
- [x] Rust 推送 WebView 可见 workflow activity message。

验收：

- [x] 用户不需要看 GPUI toast 才知道 workflow 状态。
- [x] 终态和错误可在 WebView 内追踪。

验证：

- [x] `cargo fmt`
- [x] `cargo check`
- [x] `cargo check --features workflow-webview`
- [x] `npm run typecheck`
- [x] `npm run build`
- [x] `ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test workflows::runtime`

结果记录：

- [x] `src/app_state.rs` 新增 workflow activity 状态，用于记录每个 workflow 最近一次操作结果。
- [x] `src/workflow_webview.rs` 在 `WorkflowBuilderState` 中加入 `activity` ViewModel。
- [x] `src/capabilities.rs` 在 WebView 命令路径写入 pending/success/error/info activity，包括 save/run/publish/copilot/template/json/agent/edge/canvas error。
- [x] `web/workflow-canvas/src/App.tsx` 在 inspector 顶部增加 Workflow Status 面板，显示 edit state、activity 和 run status 摘要。
- [x] `web/workflow-canvas/src/styles.css` 增加状态面板样式，错误和长文本可读可换行。

## Phase 8：删除旧 GPUI Workflows 视图代码

目标：迁移完成后清理旧 UI，避免双实现造成理解错误。

- [x] 删除 GPUI `workflow_builder_shell`。
- [x] 删除 GPUI `workflow_inspector_panel`。
- [x] 删除 GPUI workflow card list。
- [x] 删除 GPUI template gallery。
- [x] 删除旧 editor state 只服务 GPUI 表单的部分。
- [x] 清理未使用 imports / dead code。
- [x] 保留 Rust 领域 helper 和测试。

验收：

- [x] Workflows tab 只有一个 WebView UI 实现。
- [x] Rust workflow 领域内核仍被 WebView IPC 调用。
- [x] 编译 warning 不新增明显死代码。

验证：

- [x] `cargo fmt`
- [x] `cargo check`
- [x] `cargo check --features workflow-webview`
- [x] `npm run typecheck`
- [x] `npm run build`
- [x] `ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test`

结果记录：

- [x] `workflow_builder_shell` 收敛为 `workflow_builder_webview_host`，Workflows tab 主路径只承载 WebView。
- [x] 删除旧 GPUI inspector、模板 gallery、workflow card、graph preview、JSON editor 和 quick node UI。
- [x] 保留并继续复用 Rust workflow 领域 helper：template/copilot/publish/save/load/runtime/store/routing validation。
- [x] 旧 UI 符号扫描无命中：`workflow_builder_shell|workflow_inspector_panel|workflow_template_gallery|workflow_card|workflow_graph_preview|workflow_json_editor|workflow_editor_panel|WorkflowQuickNodeKind|append_workflow_node|update_workflow_edge_condition`。

## Phase 9：最终验收

目标：确认整 tab WebView 体验满足长期方向。

- [x] 新建 workflow。
- [x] 添加 Agent。
- [x] 编辑 Agent。
- [x] 创建/删除边。
- [x] 编辑 routing。
- [x] 保存 draft。
- [x] 运行 draft。
- [x] 发布 capability。
- [x] MainAgent 可调用发布能力。
- [x] JSON 高级编辑可用。
- [x] Copilot 生成 workflow 可用。

验证：

- [x] `cargo fmt`
- [x] `cargo check`
- [x] `cargo check --features workflow-webview`
- [x] `npm run typecheck`
- [x] `npm run build`
- [x] `ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test`
- [>] GUI 截图验收。依赖本机显示环境，当前自动环境可能受限。

结果记录：

- [x] 新建 workflow / 添加 Agent / 模板创建 / Copilot 创建通过 WebView IPC 命令进入 Rust 领域 helper。
- [x] Agent 编辑、routing 编辑、JSON 高级编辑由 WebView 提交，Rust 负责映射、解析、校验和保存。
- [x] 创建/删除边由 WebView canvas 事件提交，Rust 修改 `WorkflowDefinition.edges` 并刷新 hydrate。
- [x] 运行 draft 和发布 capability 仍调用 Rust runtime/publish validation；发布能力保持 MainAgent 能力源调用路径。
- [x] 最终验证通过：`cargo fmt`、`cargo check`、`cargo check --features workflow-webview`、`npm run typecheck`、`npm run build`、`ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test`。
- [>] GUI 截图验收暂缓：当前自动环境没有稳定显示上下文；需在本机应用窗口中人工确认最终视觉与交互。
