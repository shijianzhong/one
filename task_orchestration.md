# 编排与多项目支持升级任务进度 (Orchestration & Multi-project Support)

## Phase 0+4: 路径硬编码修复 + Session 透传 - [✅ 已完成]

**改动文件：**
- `src/agents/core/orchestrator.rs` — `work_dir: Mutex<PathBuf>` 字段、修复 `"."` 硬编码、`session_id` 透传
- `src/agents/core/factory.rs` — `create_orchestrator()` 新增 `work_dir` 参数
- `src/runtime/job_manager.rs` — `spawn_orchestrator_run` 透传 `self.get_work_dir()`
- 编译通过 ✅

## Phase 1: Memory L3 写入修复 - [✅ 已完成]

**改动文件：**
- `src/runtime/job_manager.rs` — orchestrator finish 回调中调用 `save_task_memory_async`
- 编译通过 ✅

## Phase 2: 文件改动实时感知 - [✅ 已完成]

**调研结论：** Claude Code CLI `stream-json` 的 `result` 是纯文本，无结构化文件变更元数据 → 改用 `git diff --name-only`

**改动文件：**
- `src/agents/claude_code.rs` — `ModifiedFiles` 枚举变体 + `detect_modified_files()` 方法
- `src/agents/types.rs` — `ClaudeRunPanelState` 新增 `modified_files` 字段
- `src/runtime/job_manager.rs` — `apply_claude_run_event` / `update_subagent_message_event` 处理 `ModifiedFiles`
- `src/runtime/events.rs` — `map_claude_to_run_event` 覆盖 `ModifiedFiles` 模式
- 编译通过 ✅

## Phase 3: 多项目动态环境切换 - [✅ 已完成]

**改动文件：**
- `src/agents/core/main_agent.rs` — `UpdateWorkDirTool` + System Prompt 引导
- `src/agents/core/orchestrator.rs` — `execute_tool_calls_and_feed_back` 拦截 `update_work_dir`
- 编译通过 ✅

## 方案C：跨 Task Memory 自动提取 - [✅ 已完成]

### 问题
LLM 口头说"我用 remember 记住"但未真正调用工具，导致 profile 为空，
切换到新 task 后不记得"小一"这个名字。

### C-1: 任务结束时自动将 facts 写入 profile
`src/memory/snapshot.rs` → `generate_snapshot_sync()`:
- snapshot 生成后，自动从 `key_facts` 和 `preferences` 中提取事实
- 用户相关事实（命名、偏好等）写入 global scope
- 项目相关事实写入 workspace scope
- **不再依赖 LLM 自觉调用 remember 工具**

### C-2: 增强 build_memory_context 注入质量
`src/memory/snapshot.rs` → `build_memory_context()`:
- L3 检索从最多 3 条增加到 5 条
- 内容截断从 200 字符增加到 400 字符，使用安全的 char boundary 截断
- 新增注入相关 task 的 snapshot key facts
- 本 task 的 snapshot 信息也注入到 system prompt（当前 task 的 key facts、summary、preferences）

### 改动文件
- `src/memory/snapshot.rs` — 两个函数均修改
- 编译通过 ✅
  - [x] `src/agents/types.rs`: `ClaudeRunPanelState` 增加 `modified_files: Vec<String>` 字段
  - [x] `src/runtime/job_manager.rs`: `apply_claude_run_event` 处理 `ModifiedFiles` 事件
  - [x] `src/runtime/job_manager.rs`: `update_subagent_message_event` 子代理卡片事件中添加文件变更展示
  - [x] `src/runtime/events.rs`: `map_claude_to_run_event` 添加 `ModifiedFiles` 覆盖模式

## Phase 3: 多项目动态环境切换 (Dynamic Environment Switch) - [✅ 已完成]
- [x] **Step A: 实现目录切换工具**
  - [x] `src/agents/core/main_agent.rs`: 实现 `UpdateWorkDirTool` (带路径存在性校验)
  - [x] `src/agents/core/orchestrator.rs`: `execute_tool_calls_and_feed_back` 新增 `update_work_dir` 拦截分支，更新 `self.work_dir`
  - [x] 使用 `std::sync::Mutex<PathBuf>` 保证跨线程安全
- [x] **Step B: Prompt 引导升级**
  - [x] `src/agents/core/main_agent.rs`: 更新 System Prompt，告知 LLM 有目录切换能力及使用场景