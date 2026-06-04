# Solo3 GPUI 迭代计划：优化、安全与架构升级

## 当前源码评估（2026-06-04）
- [x] 核心 Agent 架构已从 `Coordinator/GeneralAgent` 迁移到 `MainAgent + Orchestrator`。
- [x] 主路由已默认落到 `Orchestrator`，`IntentRouter` 只保留快速确定性路由。
- [x] `coordinator.rs`、`general_agent.rs`、`agent_jobs.rs` 已从源码树移除。
- [x] `PermissionMode` 默认值已从 `Bypass` 改为 `Default`。
- [x] Claude Code 只在 `Bypass` 模式追加 `--dangerously-skip-permissions`；命令预览需保持和真实参数一致。
- [~] `JobManager` 已抽出状态结构，但大量方法仍是 `impl AppState`，尚未完成真正解耦。
- [~] Task Event Log 的表结构和部分写入路径已存在，但还没有统一收敛到 `RunRecorder`。

## 对 `1.md` 的合理性结论
- 合理：MainAgent 作为主对话模型，使用 `run_claude_code`、`run_system_task`、`remember`、`recall` 等工具驱动专项能力，这与当前代码方向一致。
- 合理：Orchestrator 拦截长时/流式工具调用，实时透传 Claude Code 事件给 UI。
- 需调整：当前 `Orchestrator` 源码不应继续使用 `coordinator` 命名和 delegate 注释，否则会误导后续维护。
- 需谨慎：`MainAgent` 可以调用 Claude Code，但不代表普通 `CodingAgent` 必须删除；它仍可作为专项 Agent 或未来非 CLI 编码路径保留。

## 下一阶段迭代计划

### 阶段 5：路由收敛与清理 (Routing Convergence)
- [x] **收敛路由语义**：`IntentRouter` 负责快速确定性命中，复杂/模糊请求进入 `Orchestrator`。
- [x] **清理过时入口**：移除 `spawn_general_ai_run` 命名的旧入口。
- [ ] **验证测试用例**：重新运行并修正 `intent_router.rs` 等相关测试，不能在未验证时标记“测试已通过”。

### 阶段 6：安全加固 (Security & Permissions)
- [x] **默认权限升级**：`PermissionMode` 默认值为 `Default`。
- [x] **ShellTool 权限拦截**：Shell 执行经过统一权限策略。
- [x] **Claude Code 危险参数收敛**：仅 `Bypass` 模式追加 `--dangerously-skip-permissions`。
- [ ] **权限策略补强**：`Default` 当前仍是允许执行，并未实现真正的询问/提示 UI；后续需要把 `Default` 接到用户确认流程。

### 阶段 7：架构解耦与重构 (Refactoring Heavy-lifters)
- [x] **抽出 runtime 模块**：
  - `src/runtime/job_manager.rs`：集中运行状态和任务调度相关方法。
  - `src/runtime/events.rs`：集中运行事件类型转换。
- [x] **修复 JobManager 接线后的编译问题**：UI/路由访问统一改为 `self.job_manager.*`。
- [ ] **收敛事件日志**：将 Claude Code、SystemTools、Orchestrator、GeneralAI 统一改用 `RunRecorder`，减少手写 `task_db::append_run_event`。
- [ ] **移除 AppState 强耦合**：把 `job_manager.rs` 中的任务生命周期逻辑逐步迁移到独立 `JobManager` 方法，只通过明确输入/回调访问 UI 状态。

### 阶段 8：内存与存储优化 (Memory & Data)
- [ ] **抽象 MemoryStore**：定义统一接口，支持当前 TF-IDF 和未来 Vector DB。
- [ ] **规范存储路径**：将 memory/profile/soul 等持久化路径从当前目录迁移到应用配置/数据目录，避免不同启动目录导致记忆分裂。

## 当前优先级
1. 保持项目可编译：先跑通 `cargo check`。
2. 收敛架构命名：清理 Orchestrator 中 Coordinator/delegate 残留。
3. 收敛事件日志：用 `RunRecorder` 替代零散手写日志。
4. 继续推进 JobManager 独立化。

## 进度记录
- **2026-06-03**：完成第一轮架构改造，MainAgent 上线。
- **2026-06-03**：启动第二轮优化：路由收敛与安全加固。
- **2026-06-03**：完成 `agent_jobs.rs` 初步拆分，建立 `runtime` 模块。
- **2026-06-04**：校准任务文档，修复 JobManager 接线导致的 UI/路由编译错误，并修正 Claude Code 命令预览。
