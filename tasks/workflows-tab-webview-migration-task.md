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

- 当前阶段：Phase 0
- 当前状态：方案确认与任务拆分中
- 最近验证：待执行

## Phase 0：边界确认与文档

目标：明确“整个 Workflows tab WebView”新目标，避免继续沿用局部画布方案。

- [~] 形成技术方案文档。
- [~] 形成本任务清单。
- [x] 明确 WebView 是 Workflows tab 完整视图层。
- [x] 明确 Rust 仍是唯一业务真相。
- [x] 明确 WebView 编辑 Agent 后映射到 Rust `WorkflowNode` / `WorkflowDefinition`。
- [x] 明确不删除 Rust workflow runtime/store/publish validation/routing policy/capability/copilot。

验收：

- [ ] 文档能区分 UI 迁移和领域内核保留。
- [ ] 后续 Phase 可按文档执行。

验证：

- [ ] 文档检查完成。

## Phase 1：IPC 协议和命名收敛

目标：从 canvas-only IPC 升级为 workflow builder IPC。

- [ ] 将 `workflow_canvas_poc` 重命名为 `workflow_builder_webview`。
- [ ] 将 POC/fallback 文案从主路径移除或重命名为 fallback。
- [ ] 定义 Rust `WorkflowWebviewCommand`。
- [ ] 定义 Rust `WorkflowBuilderHostMessage`。
- [ ] Web 前端定义对应 TypeScript 类型。
- [ ] 保持现有 canvas load/select/edge create/delete 兼容。
- [ ] 增加 requestId command result 协议。

验收：

- [ ] WebView 仍能加载现有 canvas。
- [ ] Rust 能收到新 command 类型。
- [ ] 旧 POC 命名不再误导主路径。

验证：

- [ ] `cargo fmt`
- [ ] `cargo check`
- [ ] `cargo check --features workflow-webview`
- [ ] `npm run typecheck`
- [ ] `npm run build`

## Phase 2：WorkflowBuilderState ViewModel

目标：Rust 一次性向 WebView hydrate 完整 Workflows tab 状态。

- [ ] 新增 Rust `WorkflowBuilderState`。
- [ ] 新增 workflow summaries view。
- [ ] 新增 selected workflow view。
- [ ] 新增 edit state view：saved/dirty/save_failed。
- [ ] 新增 templates view。
- [ ] 新增 run statuses view。
- [ ] WebView 支持 `workflows:hydrate`。
- [ ] WebView 显示 workflow selector/list 的只读版本。

验收：

- [ ] 打开 Workflows tab 后 WebView 能看到 workflow 列表和当前 workflow。
- [ ] 不再依赖 GPUI 在 tab 下方渲染 workflow card 列表。

验证：

- [ ] `cargo fmt`
- [ ] `cargo check`
- [ ] `cargo check --features workflow-webview`
- [ ] `npm run typecheck`
- [ ] `npm run build`

## Phase 3：WebView Toolbar

目标：把 Publish/Add Agent/Save/Run/AI Copilot 入口迁移进 WebView。

- [ ] WebView 渲染 toolbar。
- [ ] WebView 发送 `workflow:add_agent`。
- [ ] Rust 复用 `append_empty_agent_node` 并返回 snapshot。
- [ ] WebView 发送 `workflow:save`。
- [ ] Rust 复用 `validate_and_save_workflow_definition`。
- [ ] WebView 发送 `workflow:run`。
- [ ] Rust 复用 `run_workflow_draft` 并推送 run statuses。
- [ ] WebView 发送 `workflow:publish`。
- [ ] Rust 复用 `publish_workflow_as_capability`。
- [ ] WebView 发送 `workflow:copilot_generate`。
- [ ] Rust 复用 `create_workflow_from_copilot_brief`。
- [ ] GPUI toolbar 保留临时 fallback，验证后删除。

验收：

- [ ] WebView toolbar 可完成新增 Agent、保存、运行、发布、Copilot 生成。
- [ ] 失败时 WebView 显示错误，Rust 不写非法状态。

验证：

- [ ] `cargo fmt`
- [ ] `cargo check`
- [ ] `cargo check --features workflow-webview`
- [ ] `npm run typecheck`
- [ ] `npm run build`
- [ ] `ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test capabilities::tests`

## Phase 4：WebView Agent Inspector

目标：把 Agent 节点编辑表单迁移进 WebView。

- [ ] WebView 支持选中 Agent 后显示 Inspector。
- [ ] WebView 支持 Basic 字段编辑。
- [ ] WebView 支持 Model 字段编辑。
- [ ] WebView 支持 Prompt 字段编辑。
- [ ] WebView 支持 Tools 字段编辑。
- [ ] WebView 支持 Output 字段编辑。
- [ ] WebView 支持 Settings/Routing 字段编辑。
- [ ] WebView 发送 `workflow:update_agent`。
- [ ] Rust 复用并收敛 `save_workflow_agent_node_update` 映射逻辑。
- [ ] Rust 返回规范化后的 workflow snapshot。
- [ ] 删除 GPUI Agent Inspector。

验收：

- [ ] WebView 编辑 Agent 后，Rust `WorkflowDefinition.nodes[].config` 正确更新。
- [ ] 非法 JSON / 数值 / routing 被 Rust 拦截。
- [ ] 修改只影响当前 workflow 当前节点。

验证：

- [ ] `cargo fmt`
- [ ] `cargo check`
- [ ] `cargo check --features workflow-webview`
- [ ] `npm run typecheck`
- [ ] `npm run build`
- [ ] `ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test capabilities::tests`
- [ ] 新增/保留 Agent update 映射测试。

## Phase 5：WebView Workflow List 与模板入口

目标：把 workflow draft list 和模板 shortcut 迁移进 WebView。

- [ ] WebView 显示 draft workflow list。
- [ ] WebView 支持选择 workflow。
- [ ] WebView 支持模板 shortcut。
- [ ] Rust 复用 `create_workflow_from_template`。
- [ ] 删除 GPUI workflow card list。
- [ ] 删除 GPUI template gallery。

验收：

- [ ] 用户能在 WebView 内切换 workflow。
- [ ] 创建模板 workflow 后自动选中并刷新。

验证：

- [ ] `cargo fmt`
- [ ] `cargo check`
- [ ] `cargo check --features workflow-webview`
- [ ] `npm run typecheck`
- [ ] `npm run build`

## Phase 6：WebView JSON Advanced Editor

目标：把 JSON 高级编辑迁移进 WebView，但校验仍由 Rust 做。

- [ ] WebView 显示当前 workflow JSON。
- [ ] WebView 支持 JSON 编辑。
- [ ] WebView 发送 `workflow:update_json`。
- [ ] Rust 解析为 `WorkflowDefinition`。
- [ ] Rust 执行 routing validation。
- [ ] Rust 保存 draft 或返回错误。
- [ ] 删除 GPUI JSON 编辑入口。

验收：

- [ ] 合法 JSON 可保存并刷新画布。
- [ ] 非法 JSON 或非法 routing 不污染数据库。

验证：

- [ ] `cargo fmt`
- [ ] `cargo check`
- [ ] `cargo check --features workflow-webview`
- [ ] `npm run typecheck`
- [ ] `npm run build`
- [ ] `ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test capabilities::tests`
- [ ] `ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test workflows::store`

## Phase 7：状态、错误和运行事件闭环

目标：WebView 内完整展示 dirty/save/run/publish/validation 状态。

- [ ] WebView 显示 saved/dirty/save_failed。
- [ ] WebView 显示 command pending。
- [ ] WebView 显示 command error。
- [ ] WebView 显示 run statuses。
- [ ] WebView 显示 publish success/failure。
- [ ] Rust 推送 `workflow:run_event`。
- [ ] Rust 推送 `workflow:toast` 或 command result message。

验收：

- [ ] 用户不需要看 GPUI toast 才知道 workflow 状态。
- [ ] 终态和错误可在 WebView 内追踪。

验证：

- [ ] `cargo fmt`
- [ ] `cargo check`
- [ ] `cargo check --features workflow-webview`
- [ ] `npm run typecheck`
- [ ] `npm run build`
- [ ] `ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test workflows::runtime`

## Phase 8：删除旧 GPUI Workflows 视图代码

目标：迁移完成后清理旧 UI，避免双实现造成理解错误。

- [ ] 删除 GPUI `workflow_builder_shell`。
- [ ] 删除 GPUI `workflow_inspector_panel`。
- [ ] 删除 GPUI workflow card list。
- [ ] 删除 GPUI template gallery。
- [ ] 删除旧 editor state 只服务 GPUI 表单的部分。
- [ ] 清理未使用 imports / dead code。
- [ ] 保留 Rust 领域 helper 和测试。

验收：

- [ ] Workflows tab 只有一个 WebView UI 实现。
- [ ] Rust workflow 领域内核仍被 WebView IPC 调用。
- [ ] 编译 warning 不新增明显死代码。

验证：

- [ ] `cargo fmt`
- [ ] `cargo check`
- [ ] `cargo check --features workflow-webview`
- [ ] `npm run typecheck`
- [ ] `npm run build`
- [ ] `ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test`

## Phase 9：最终验收

目标：确认整 tab WebView 体验满足长期方向。

- [ ] 新建 workflow。
- [ ] 添加 Agent。
- [ ] 编辑 Agent。
- [ ] 创建/删除边。
- [ ] 编辑 routing。
- [ ] 保存 draft。
- [ ] 运行 draft。
- [ ] 发布 capability。
- [ ] MainAgent 可调用发布能力。
- [ ] JSON 高级编辑可用。
- [ ] Copilot 生成 workflow 可用。

验证：

- [ ] `cargo fmt`
- [ ] `cargo check`
- [ ] `cargo check --features workflow-webview`
- [ ] `npm run typecheck`
- [ ] `npm run build`
- [ ] `ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test`
- [>] GUI 截图验收。依赖本机显示环境，当前自动环境可能受限。

