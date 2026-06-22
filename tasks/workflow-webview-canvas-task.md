# 多 Agent 工作流编排 WebView 画布任务清单

关联方案文档：[docs/workflow-webview-canvas-technical-plan.md](../docs/workflow-webview-canvas-technical-plan.md)

## 状态约定

- `[ ]` 未开始
- `[~]` 进行中
- `[x]` 已完成
- `[!]` 阻塞
- `[>]` 暂缓

## 执行规则

- 每次修改代码前，先把当前任务标记为 `[~]`。
- 每个 Phase 完成后必须自测，并在本文档记录验证结果。
- 如果方案变更，先更新技术方案文档，再同步调整本文档。
- 废弃代码直接删除，不保留误导性旧路径。
- WebView 只负责 workflow canvas；Rust/GPUI 保持业务真相、校验、保存、发布和运行。
- 当前 Workflows tab 只编辑 workflow 内 Agent 节点实例，不编辑全局 Agent 模板。

## 总体进度

- 当前阶段：当前约定范围已完成
- 当前状态：Phase 0-11、Phase 13 已实现并复验通过；Phase 12 与远期能力按前序决策暂缓
- 最近验证：2026-06-22 完成 Rust/Web/核心 workflow 测试复验；GUI 截图验收仍受当前执行环境显示权限限制

## Phase 0：方案确认与任务拆分

目标：确认产品边界、技术边界、迭代顺序，并形成可执行任务清单。

- [x] 确认 GPUI 主应用不改为 WebView 套壳。
- [x] 确认 WebView 只作为 workflow canvas 的局部交互岛。
- [x] 确认右侧 Inspector / Agent 编辑器优先用 GPUI 原生实现。
- [x] 确认当前页面只编辑 workflow 内 Agent 实例。
- [x] 确认全局 Agent 模板后续独立放到左侧新导航。
- [x] 确认 `添加 Agent` 支持新增空的局部 Agent。
- [x] 确认 `AI Copilot` 生成多 Agent workflow draft，不直接发布。
- [x] 确认 Agent 间 routing policy 是 workflow builder 核心能力。
- [x] 完成技术方案文档。
- [x] 完成任务拆分文档。

验证：

- [x] 已检查现有 `WorkflowDefinition`、`WorkflowStore`、Workflows tab 代码结构。
- [x] 已确认当前项目没有 WebView 依赖。
- [x] 已确认现有 workflow 数据模型具备 nodes/edges 基础。

## Phase 1：WebView POC

目标：验证 WebView 是否能稳定嵌入 GPUI 的 Workflows tab。

- [x] 调研 GPUI 内嵌 WebView 的可行路径。
- [x] 评估 `wry` 是否能作为局部 WebView 宿主。
- [x] 评估 macOS 原生 `WKWebView` 嵌入 GPUI window 的可行性。
- [x] 明确 Linux 支持策略，避免 macOS-only 实现污染主路径。
- [x] 新增最小 Rust WebView 封装模块。
- [x] 在 Workflows tab 中加载本地 HTML。
- [x] WebView 显示静态 workflow canvas 占位页。
- [x] Rust 向 WebView 发送最小 `workflow:load` 消息。
- [x] WebView 向 Rust 发送最小 `node:selected` 消息。
- [x] 增加 POC 日志，能追踪 WebView 初始化、加载和 IPC 事件。
- [x] 如果 GPUI 内嵌不可行，记录阻塞原因并更新替代方案。

验收：

- [x] Workflows tab 能打开 WebView 画布。
- [x] WebView 不闪退、不阻塞主窗口。
- [x] Rust 能收到 WebView 的节点选择事件。
- [x] 关闭/切换页面不会泄露 WebView 资源。

验证命令：

- [x] `cargo fmt`
- [x] `cargo check`
- [x] `cargo check --features workflow-webview`
- [x] `ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test workflow_webview`
- [x] `ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test workflow_webview --features workflow-webview`
- [x] `ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test`

结果记录：

- [x] 新增 `src/workflow_webview.rs`，默认 feature 关闭时显示降级占位，避免影响现有应用。
- [x] macOS/Windows 下通过可选 `workflow-webview` feature 使用 `wry` 子 WebView；当前 macOS 编译验证通过。
- [x] Rust 侧通过 `evaluate_script` 发送 `workflow:load`，WebView 侧通过 `window.ipc.postMessage` 回传 `workflow:ready`、`workflow:loaded`、`node:selected`。
- [x] Linux 当前不启用 `wry` 运行时，只显示 unsupported/disabled 状态；后续如果要支持 Linux，需要单独验证 X11/WebKitGTK 和 Wayland 降级。

## Phase 2：Canvas 前端工程与数据接入

目标：用 Web 技术渲染真实 workflow nodes/edges。

- [x] 新建 `web/workflow-canvas/` 前端工程。
- [x] 选择并安装画布库，优先 `@xyflow/react`。
- [x] 配置 Vite / React / TypeScript。
- [x] 定义 Web 侧 `CanvasWorkflow`, `CanvasNode`, `CanvasEdge` 类型。
- [x] 定义 WebView IPC bridge。
- [x] 实现 `workflow:load` 消息处理。
- [x] 实现节点卡片 UI，接近参考图风格。
- [x] 实现边渲染、缩放、平移、fit view。
- [x] 实现空画布状态。
- [x] Rust 侧新增 `WorkflowDefinition -> CanvasModel` 转换。
- [x] Rust 侧从现有 `WorkflowStore` 加载真实 draft workflow。
- [x] Workflows tab 选中 workflow 后把真实数据传给 WebView。
- [x] 前端 build 产物纳入本地资源加载路径。

验收：

- [x] 现有 draft workflow 能在画布展示。
- [x] 节点标题、描述、类型、执行模式 badge 正确。
- [x] 边显示正确。
- [x] 大小窗口下画布可用。

验证命令：

- [x] `cargo fmt`
- [x] `cargo check`
- [x] `cargo check --features workflow-webview`
- [x] `ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test workflow_webview`
- [x] `npm run typecheck`
- [x] `npm run build`

结果记录：

- [x] 新增 `web/workflow-canvas/` Vite + React + TypeScript 工程，使用 `@xyflow/react` 渲染画布。
- [x] Web 侧通过 `one-message` 接收 Rust 的 `workflow:load`，通过 `window.ipc.postMessage` 回传 `workflow:ready`、`workflow:loaded`、`node:selected`。
- [x] Rust 侧 `CanvasWorkflow::from_definition` 将 draft workflow nodes/edges 转为 Web 画布模型。
- [x] Workflows tab 优先展示当前 `editing_workflow_id` 对应 workflow，未编辑时回退第一个 draft。
- [x] WebView feature 启用且 dist 存在时加载 `web/workflow-canvas/dist/index.html`，否则回退内置 POC HTML。

## Phase 3：工作流编辑器主布局

目标：把 Workflows tab 从列表卡片升级为编辑器页面。

- [x] 新增 Workflows tab 顶部工具栏。
- [x] 顶部按钮包含 `发布`、`添加 Agent`、`保存`、`运行`、`AI Copilot`。
- [x] 左侧/中间区域承载 WebView canvas。
- [x] 右侧区域承载 Inspector。
- [x] 未选中节点时，Inspector 显示 workflow 元信息。
- [>] 选中 Agent 节点时，Inspector 显示 Agent 编辑器。此项归入 Phase 5 的 `node:selected` -> AppState -> Inspector 编辑器闭环。
- [x] 保留现有 JSON 编辑能力作为高级/调试入口。
- [x] 保留现有模板创建能力，或迁移为 `AI Copilot / 模板` 入口。
- [x] 保留最近运行记录入口，不遮挡主画布。

验收：

- [x] Workflows tab 首屏是可编辑画布，不再只是列表。
- [x] 顶部按钮语义清晰。
- [x] 右侧 Inspector 不遮挡画布。
- [x] 窄窗口下布局仍可操作。

验证命令：

- [x] `cargo fmt`
- [x] `cargo check`
- [x] `cargo check --features workflow-webview`

结果记录：

- [x] 新增 workflow builder shell：顶部工具栏、左侧 WebView canvas、右侧 workflow inspector。
- [x] `Publish` 使用现有发布逻辑；`Add Agent`、`Save`、`Run`、`AI Copilot` 在后续 Phase 前给出明确 toast，不做假实现。
- [x] 现有 workflow 列表、模板创建、JSON 编辑和最近运行记录继续保留。

## Phase 4：添加空 Agent

目标：支持在当前 workflow 中新增局部 Agent 节点。

- [x] 定义局部 Agent 默认 config。
- [x] 新增 Rust helper：生成唯一 local Agent node id。
- [x] 新增 Rust helper：创建空 local Agent node。
- [x] 点击 `添加 Agent` 时写入当前 workflow draft。
- [x] 新节点默认放到画布可见区域中心或当前选中节点旁。
- [>] 新节点创建后自动选中。此项并入 Phase 5 的选中节点状态闭环。
- [x] WebView 刷新并展示新节点。
- [>] 右侧 Inspector 自动打开 Agent 编辑器。此项并入 Phase 5。
- [x] 新节点保存后重开页面仍存在。

验收：

- [x] 不依赖全局 Agent 模板。
- [x] 新 Agent 默认配置完整。
- [>] 新 Agent 可以继续编辑。编辑器并入 Phase 5。
- [x] 不影响其他 workflow。

验证命令：

- [x] `cargo fmt`
- [x] `cargo check`
- [x] `cargo check --features workflow-webview`
- [x] `ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test capabilities::tests`
- [x] 新增 Rust 单元测试：local Agent 默认 config
- [x] 新增 Rust 单元测试：node id 去重

结果记录：

- [x] `Add Agent` 使用 `append_empty_agent_node` 直接保存当前 workflow draft，不依赖全局 Agent 模板。
- [x] 新节点使用 `local_agent` / `local_agent_N` 去重 id，kind 写入 `Agent { agent_id: "local:<node_id>" }`。
- [x] 默认 config 覆盖 model、prompt、tools、output、settings、routing，后续 Inspector 可直接编辑这些字段。
- [x] 画布位置当前由 `CanvasWorkflow::from_definition` 自动网格布局生成，新增节点会出现在可见画布布局中；持久化节点坐标归入后续画布编辑/dirty 状态阶段。
- [x] Phase 4 验证通过：`cargo fmt`、`cargo check`、`cargo check --features workflow-webview`、`ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test capabilities::tests`。

## Phase 5：Agent 节点实例编辑器

目标：点击画布 Agent 后，右侧可编辑该 workflow 内节点实例。

- [x] AppState 增加当前选中 workflow id。
- [x] AppState 增加当前选中 node id。
- [x] WebView `node:selected` 同步到 AppState。
- [x] Inspector 读取当前 workflow node。
- [x] 实现 `基本` tab：名称、描述、分类、标签、版本。
- [x] 实现 `模型` tab：模型、temperature、max tokens、timeout。
- [x] 实现 `提示词` tab：system prompt、instruction、context rules。
- [x] 实现 `工具` tab：skills、MCP tools、system tools、coding runtimes。
- [x] 实现 `输出` tab：output schema、result format、MainAgent 汇总开关。
- [x] 实现 `设置` tab 基础字段：重试、超时、人工确认、权限。
- [x] 保存 Agent 编辑器内容到 `WorkflowDefinition.nodes[].config`。
- [x] 保存后通知 WebView 刷新节点标题和描述。
- [x] 编辑失败显示 toast 和错误原因。

验收：

- [x] 编辑只影响当前 workflow 的当前节点。
- [x] 不影响全局 Agent 模板。
- [x] 不影响其他 workflow。
- [x] JSON 存储合法。

验证命令：

- [x] `cargo fmt`
- [x] `cargo check`
- [x] `cargo check --features workflow-webview`
- [x] `npm run typecheck`
- [x] `npm run build`
- [x] `ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test capabilities::tests`
- [x] 新增 Rust 单元测试：保存节点 config
- [x] 新增 Rust 单元测试：非法 node id 保存失败

结果记录：

- [x] WebView IPC `node:selected` 进入 Rust 队列，Workflows tab 消费后同步 `selected_workflow_id` / `selected_workflow_node_id`。
- [x] Inspector 支持编辑 workflow-local Agent 实例，不依赖也不修改全局 Agent 模板。
- [x] 基本、模型、提示词、工具、输出、设置字段保存到 `WorkflowDefinition.nodes[].config`；工具字段当前使用 JSON 数组编辑器承载，后续可替换为专用选择器。
- [x] 保存前校验数字、布尔、JSON array、output schema；错误通过 toast 展示，不写入非法 JSON。
- [x] Phase 5 验证通过：`cargo fmt`、`cargo check`、`cargo check --features workflow-webview`、`npm run typecheck`、`npm run build`、`ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test capabilities::tests`。

## Phase 6：连线编辑

目标：支持 WebView 创建和删除 workflow edge。

- [x] WebView 支持节点端口连接。
- [x] WebView 发送 `edge:created` IPC。
- [x] Rust 校验 from/to 节点存在。
- [x] Rust 生成唯一 edge id。
- [x] Rust 保存 edge 到 `WorkflowDefinition.edges`。
- [x] WebView 支持删除 edge。
- [x] WebView 发送 `edge:deleted` IPC。
- [x] Rust 删除 edge 并保存。
- [x] 边条件默认 `always`。
- [x] 右侧 Inspector 或边编辑入口可编辑 condition。

验收：

- [x] 无法连接不存在的节点。
- [x] 默认不允许 self-loop。
- [x] 保存后重开页面边仍存在。
- [x] 删除后重开页面边不再存在。

验证命令：

- [x] `cargo fmt`
- [x] `cargo check`
- [x] `cargo check --features workflow-webview`
- [x] `npm run typecheck`
- [x] `npm run build`
- [x] `ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test capabilities::tests`
- [x] 新增 Rust 单元测试：创建 edge
- [x] 新增 Rust 单元测试：删除 edge
- [x] 新增 Rust 单元测试：非法 edge 被拒绝

结果记录：

- [x] ReactFlow `onConnect` 发送 `edge:created`，`onEdgesChange/remove` 发送 `edge:deleted`。
- [x] Rust IPC 解析 edge 事件后进入统一 canvas event 队列，由 Workflows tab 消费并写入 draft。
- [x] 创建 edge 前校验 from/to 节点存在，禁止 self-loop，默认 condition 为 `always`。
- [x] 删除 edge 前校验 edge id 存在；失败通过 toast 返回，不静默污染状态。
- [x] Phase 6 验证通过：`cargo fmt`、`cargo check`、`cargo check --features workflow-webview`、`npm run typecheck`、`npm run build`、`ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test capabilities::tests`。

## Phase 7：路由策略编辑

目标：内置支持 Agent 之间不同路由策略，参考 AutoGen 的协作模式。

- [x] 定义 Rust routing policy 结构化解析层。
- [x] 支持 workflow 级 routing policy，存入 `metadata.routing`。
- [x] 支持 node 级 routing policy，存入 node config。
- [x] 支持 edge 级 routing policy，存入 `metadata.edge_routing` 或 edge config。
- [x] Inspector `设置` tab 增加路由策略区块。
- [x] 支持 `sequential`。
- [x] 支持 `parallel`。
- [x] 支持 `selector`。
- [x] 支持 `handoff`。
- [x] 支持 `graph`。
- [x] 支持 activation condition：`all` / `any`。
- [x] 支持 selector 候选 Agent 配置。
- [x] 支持 handoff targets 配置。
- [x] 支持 max loops / termination 配置。
- [x] WebView 节点 badge 展示路由模式。
- [x] WebView 边样式展示 conditional / handoff / loop。
- [x] Rust 保存前校验 routing policy。

验收：

- [x] 路由配置保存后重开仍存在。
- [x] 非法 routing 配置被 Rust 拦截。
- [x] 画布能看出 sequential / parallel / selector / handoff / graph 语义。

验证命令：

- [x] `cargo fmt`
- [x] `cargo check`
- [x] `cargo check --features workflow-webview`
- [x] `npm run typecheck`
- [x] `npm run build`
- [x] `ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test routing_policy`
- [x] `ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test capabilities::tests`
- [x] 新增 Rust 单元测试：routing policy parse
- [x] 新增 Rust 单元测试：routing policy validation
- [x] 新增 Rust 单元测试：edge routing validation

结果记录：

- [x] 新增 `src/workflows/routing_policy.rs`，结构化支持 `sequential`、`parallel`、`selector`、`handoff`、`graph`。
- [x] workflow 级 routing 读取 `metadata.routing`；node 级 routing 读取 `nodes[].config.routing`；edge 级 routing 读取 `metadata.edge_routing[edge_id]`。
- [x] `parallel` 要求 `activation: all|any`，`selector` 要求 `selector_candidates`，`handoff` 要求 `handoff_targets`，`max_loops` 必须大于 0。
- [x] Agent Inspector 的 routing policy JSON 编辑器保存前使用 Rust 结构化解析和 workflow 节点引用校验。
- [x] 发布 workflow 前调用 `validate_definition_routing`，非法 routing 不会进入能力发布路径。
- [x] WebView 节点 badge 展示 node routing mode；边样式读取 edge routing mode，selector/handoff 使用动态边效果，parallel 使用差异颜色。
- [x] Phase 7 验证通过：`cargo fmt`、`cargo check`、`cargo check --features workflow-webview`、`npm run typecheck`、`npm run build`、`ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test routing_policy`、`ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test capabilities::tests`。

## Phase 8：保存、Dirty 状态与错误处理

目标：避免 WebView state 和 Rust state 不一致。

- [x] Rust 保持 workflow 数据唯一真相。
- [x] WebView 编辑后标记 dirty。
- [x] 顶部显示 dirty 状态。
- [x] 点击 `保存` 触发 Rust 校验和保存。
- [x] 保存成功后清除 dirty。
- [x] 保存失败时保留 dirty 并展示错误。
- [x] 切换 workflow 前提示未保存变更。
- [x] 切换 tab 前提示未保存变更。
- [x] WebView IPC 异常有错误 toast。
- [x] WebView 加载失败有降级 UI。
- [x] 增加关键链路日志。

验收：

- [x] 用户不会误以为已经保存。
- [x] WebView 异常不会导致应用崩溃。
- [x] 保存失败能定位问题。

验证命令：

- [x] `cargo fmt`
- [x] `cargo check`
- [x] `cargo check --features workflow-webview`
- [x] `npm run typecheck`
- [x] `npm run build`
- [x] `ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test capabilities::tests`
- [x] 新增 Rust 单元测试：dirty/save 状态 reducer 或 helper

结果记录：

- [x] 新增 `WorkflowEditState`，记录每个 workflow 的 dirty、变更原因和最近错误。
- [x] `Add Agent`、Agent Inspector 保存、WebView edge 创建/删除、模板创建都会标记 workflow 为 dirty；顶部 builder 状态展示 `Saved` / `Unsaved changes` / `Save failed`。
- [x] 顶部 `Save` 不再是占位，会加载当前 draft、执行 Rust routing 校验、重新保存 draft，成功后清除 dirty，失败保留 dirty 和错误。
- [x] JSON 高级编辑保存接入同一套校验与 dirty 状态，非法 routing 不会绕过 Rust 校验写入。
- [x] 切换 workflow 或从 Workflows tab 切走时，如果当前 workflow dirty，会通过 warning toast 提示。
- [x] WebView 新增 `canvas:error` IPC，前端捕获 `error` / `unhandledrejection` 并上报；Rust IPC 解析失败也会进入事件队列并展示 toast。
- [x] `workflow_builder` / `workflow_webview` 日志链路记录 dirty、saved、save failed、canvas IPC error。
- [x] Phase 8 验证通过：`cargo fmt`、`cargo check`、`cargo check --features workflow-webview`、`npm run typecheck`、`npm run build`、`ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test capabilities::tests`。

## Phase 9：运行测试与调度器

目标：支持直接运行当前 draft，并按 routing policy 调度 Agent。

- [x] 新增或重构 `WorkflowScheduler`。
- [x] 定义 `GraphExecutionState`。
- [x] 定义节点运行状态：pending / ready / running / waiting_user / succeeded / failed / skipped。
- [x] Sequential 调度可执行。
- [x] Parallel 调度可执行。
- [x] Graph 调度可执行。
- [x] Selector 调度可解析，第一阶段允许 fallback。
- [x] Handoff 调度可解析，第一阶段允许受限执行。
- [x] 节点状态通过 IPC 回传 WebView。
- [x] WebView 节点状态可视化。
- [x] waiting_user 状态能提示 MainAgent 或 UI。
- [x] `运行` 当前 draft 不要求先发布。
- [x] 运行事件写入现有 workflow run/event 表。

验收：

- [x] draft 可测试运行。
- [x] sequential / parallel / graph 三类基础策略可执行。
- [x] 节点状态变化可视化。
- [x] 失败信息可追踪。

验证命令：

- [x] `cargo fmt`
- [x] `cargo check`
- [x] `cargo check --features workflow-webview`
- [x] `npm run typecheck`
- [x] `npm run build`
- [x] `ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test workflows::runtime`
- [x] `ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test capabilities::tests`
- [x] 新增 Rust 单元测试：sequential scheduler
- [x] 新增 Rust 单元测试：parallel scheduler all join
- [x] 新增 Rust 单元测试：parallel scheduler any join
- [x] 新增 Rust 单元测试：graph condition
- [x] 相关 workflow runtime 测试

结果记录：

- [x] 新增 `WorkflowScheduler`，`WorkflowRuntime::run_definition` 主路径改为通过 scheduler 执行。
- [x] 新增 `GraphExecutionState` 和 `NodeRunStatus`，覆盖 pending / ready / running / waiting_user / succeeded / failed / skipped。
- [x] `sequential` 保持原线性执行语义；有 edge 时仍按 graph 路由执行。
- [x] `parallel` 支持 `activation: all|any`，并行执行入口节点并汇总每个节点输出；`any` 允许部分失败。
- [x] `graph` 保持条件边路由和 max_steps 防循环保护；`selector` / `handoff` 第一阶段按已解析策略降级到 graph/linear 执行路径。
- [x] `run_definition` 返回 `node_status`，Workflows tab 运行 draft 后写入 `workflow_node_run_statuses`，下一次 canvas load 通过 WebView IPC 传给前端。
- [x] WebView Agent 节点新增运行状态 badge，支持 succeeded / failed / running / waiting_user 的 light/dark 样式。
- [x] 顶部 `Run` 按钮可直接运行当前 draft，不要求发布，并写入 `workflow_runs` / `workflow_run_events` 的 draft run 事件。
- [x] Phase 9 验证通过：`cargo fmt`、`cargo check`、`cargo check --features workflow-webview`、`npm run typecheck`、`npm run build`、`ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test workflows::runtime`、`ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test capabilities::tests`。

## Phase 10：发布能力

目标：把当前 workflow draft 发布为可被 MainAgent 调用的能力。

- [x] 发布前执行严格校验。
- [x] 至少一个节点。
- [x] 至少一个输出节点或可推导最终输出。
- [x] local Agent 必填字段完整。
- [x] selector 有可用模型或 fallback。
- [x] graph 不存在不可终止循环。
- [x] parallel join 有明确 activation condition。
- [x] 固化 workflow version。
- [x] 写入 `workflow_versions`。
- [x] 写入或更新 capability manifest。
- [x] 发布成功后能力库可见。
- [x] MainAgent 能看到并调用已发布能力。

验收：

- [x] 发布后能力可运行。
- [x] 修改 draft 不影响已发布版本。
- [x] 全局模板未来变更不影响已发布能力。

验证命令：

- [x] `cargo fmt`
- [x] `cargo check`
- [x] `cargo check --features workflow-webview`
- [x] `npm run typecheck`
- [x] `npm run build`
- [x] `ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test publish_validation`
- [x] `ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test workflows::store`
- [x] `ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test workflows::runtime`
- [x] `ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test capabilities::tests`
- [x] 新增 Rust 单元测试：发布前校验
- [x] 新增 Rust 单元测试：发布快照不受 draft 修改影响
- [x] 现有 capability 运行测试

结果记录：

- [x] 新增 `src/workflows/publish_validation.rs`，发布前统一执行严格校验，并在 UI 发布入口和 `WorkflowStore::publish_as_capability` 双层调用，防止绕过。
- [x] 发布校验覆盖空 workflow、输出边界、local Agent 必填字段、selector model/fallback、不可终止循环、routing policy。
- [x] `parallel` 的 `activation: all|any` 由 Phase 7 的 `validate_definition_routing` 在发布链路复用校验。
- [x] 发布成功后写入 `workflow_versions` 不可变定义快照，并 upsert `capabilities` manifest。
- [x] 能力库读取 `capability_manifests()`；MainAgent prompt 注入 `format_capabilities_for_prompt`，工具侧 `run_capability` schema 枚举已发布 capability id。
- [x] 新增 store 测试证明：后续 draft/import 修改不会改变已发布 v1 快照，能力仍按 manifest 指向版本读取。
- [x] 当前没有全局 Agent 模板接入；已发布能力使用 workflow-local 节点配置快照，因此未来模板变更不会影响当前已发布定义。

## Phase 11：AI Copilot

目标：根据自然语言需求生成多 Agent workflow draft。

- [x] 设计 `WorkflowDesignerAgent` 输入输出 schema。
- [x] AI Copilot 弹窗或侧栏输入需求。
- [x] 收集当前可用内置 Agent。
- [x] 收集当前可用 skills。
- [>] 收集当前可用 MCP tools。MCP 按前序决策暂缓，schema/context 已预留字段。
- [x] 生成 workflow 元信息。
- [x] 生成 local Agent 节点。
- [x] 生成 edges。
- [x] 生成 routing policy。
- [x] 生成节点 prompt。
- [x] 生成工具建议。
- [x] 生成 output schema。
- [x] Rust 校验模型输出。
- [x] 校验失败时给出可读错误。
- [x] 支持 AI 根据校验错误二次修复。
- [x] 生成结果进入 draft，不直接发布。
- [x] WebView 展示生成结果。
- [x] 用户可继续编辑。

验收：

- [x] AI Copilot 能从一句需求生成可编辑 workflow draft。
- [x] 生成内容包含 routing policy。
- [x] 不直接发布能力。
- [x] 非法模型输出不会污染数据库。

验证命令：

- [x] `cargo fmt`
- [x] `cargo check`
- [x] `cargo check --features workflow-webview`
- [x] `npm run typecheck`
- [x] `npm run build`
- [x] `ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test workflows::copilot`
- [x] `ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test capabilities::tests`
- [x] 新增 Rust 单元测试：AI 输出 schema 校验
- [x] 新增 Rust 单元测试：AI 生成 workflow 导入 draft

结果记录：

- [x] 新增 `src/workflows/copilot.rs`，定义 `WorkflowDesignerDraft`、`WorkflowDesignerAgent`、`WorkflowDesignerEdge` schema。
- [x] 有模型 API key 时调用系统模型/轻量模型生成 JSON；解析或校验失败后带错误信息进行一次修复；仍失败时返回可读错误，不写数据库。
- [x] 无 API key 时使用 deterministic fallback 生成可编辑多 Agent workflow draft，保证离线也能打通产品路径。
- [x] Workflows Builder 顶部新增 `Copilot brief` 输入行，点击 `AI Copilot` 后异步生成新 draft、保存、选中并刷新 WebView。
- [x] Copilot 生成结果包含 workflow metadata、local Agent 节点、顺序边、routing policy、节点 prompt、工具建议、output schema。
- [x] 生成结果只进入 `workflows` draft，不写 `workflow_versions`，不写 capability manifest。

## Phase 12：全局 Agent 模板接入

前置：左侧全局 Agent 模板页面完成。

- [>] 新增左侧 Agent 模板导航。
- [>] 全局 Agent 模板 CRUD。
- [>] 添加 Agent 时可选择全局模板。
- [>] workflow 节点引用 `template:<id>`。
- [>] 支持 node config overrides。
- [>] 发布时解析模板并固化快照。
- [>] 支持提示用户同步模板更新。

验收：

- [>] 修改全局模板不影响已发布 capability。
- [>] 当前 workflow 可选择是否同步模板更新。

## Phase 13：视觉与交互打磨

目标：让 workflow builder 达到长期可用的产品质量。

- [x] 节点卡视觉接近参考图，但符合 ONE 现有主题。
- [x] 支持 light/dark theme。
- [x] 节点文字不溢出。
- [x] 边标签不遮挡节点。
- [x] 右侧 Inspector 表单密度适中。
- [x] 画布缩放控件可用。
- [x] fit view 可用。
- [x] minimap 根据需要启用。
- [x] 大 workflow 下性能可接受。
- [x] 空状态明确引导 `添加 Agent` 和 `AI Copilot`。
- [x] 错误状态有清晰修复路径。

验收：

- [x] 常见 laptop 宽度可用。
- [x] 侧栏打开时仍可编辑核心内容。
- [x] 多节点、多边场景可读。

验证命令：

- [x] `cargo fmt`
- [x] `cargo check`
- [x] `cargo check --features workflow-webview`
- [x] `npm run typecheck`
- [x] `npm run build`
- [x] `ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test publish_validation`
- [x] `ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test workflows::store`
- [x] `ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test workflows::runtime`
- [x] `ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test workflows::copilot`
- [x] `ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test capabilities::tests`
- [x] `ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test`
- [>] 手动 UI 验收截图。已尝试启动 GUI 并截图，但当前执行环境无法从显示器创建截图，后续需人工打开应用确认。

结果记录：

- [x] Web canvas 节点从 220px 压缩到 204px，降低阴影强度，标题允许两行，描述压缩为更稳定的两行文本。
- [x] Web canvas header 从 52px 降到 44px，控件和 minimap 统一轻量边框/低阴影，避免侧栏打开后显得拥挤。
- [x] GPUI Workflow Builder 高度从 430px 降到 410px，画布最小高度从 384px 降到 332px，Copilot brief 行压缩纵向 padding。
- [x] 保留 product UI 风格：克制色彩、低装饰、明确状态，不引入营销式 hero 或重装饰卡片。
- [x] `cargo run --features workflow-webview` 能编译并启动 `target/debug/one`，日志显示 GUI starting、terminal emulator 初始化、MCP connected。
- [x] 2026-06-22 复验通过：`cargo fmt`、`cargo check`、`cargo check --features workflow-webview`、`npm run typecheck`、`npm run build`、`publish_validation`、`workflows::store`、`workflows::runtime`、`workflows::copilot`、`capabilities::tests`、全量 `cargo test` 154 项。
- [>] `screencapture -x /private/tmp/one-workflow-phase13.png` 在当前执行环境失败：`could not create image from display`；AppleScript 激活 `application "one"` 也受限。因此截图验收不能由本轮自动完成。

## 暂缓事项

- [>] 全局 Agent 模板页面和模板库管理。
- [>] 模板市场。
- [>] 多人协作编辑。
- [>] workflow 版本 diff。
- [>] 可视化调试回放。
- [>] MCP Apps 式外部 UI 插件。

## 当前阻塞与风险

- [x] WebView 嵌入 GPUI 的 POC 已完成；默认 feature 关闭时有降级占位，`workflow-webview` feature 编译通过。
- [x] WebView IPC 已完成 POC 并进入主链路；支持 ready/load/node selected/edge create/delete/error。
- [x] workflow runtime 已在 Phase 9 引入 scheduler，并完成 sequential / parallel / graph 基础调度验证。
- [>] Linux 平台 WebView 支持策略暂缓。当前 Linux 不启用 `wry` 运行时，只显示 unsupported/disabled 状态；后续需要单独验证 X11/WebKitGTK 和 Wayland。
- [>] 全局 Agent 模板接入暂缓。当前 workflow 只编辑局部 Agent 实例，发布能力固化 workflow-local 快照。

## 变更记录

- 2026-06-21：创建任务文档，基于 `workflow-webview-canvas-technical-plan.md` 拆分 Phase 0-13。
- 2026-06-22：完成当前约定范围收口复验；Phase 12 全局 Agent 模板、Linux WebView、手动截图验收和远期能力继续暂缓。
