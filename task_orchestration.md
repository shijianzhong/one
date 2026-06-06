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

## Task 删除问题修复 - [✅ 已完成]

### 根因分析
1. **P0** `delete_task` 只删了 `messages` 和 `tasks` 表，遗漏 `task_runs`、`run_events`、`agent_instances`、`agent_conversations`
2. **P1** 删除前未检查任务是否在运行，导致数据库状态混乱
3. **P2** 未清理 JobManager 状态 (`current_claude_run`、`general_ai_task_id`、`subagent_messages`、`task_active_states`) 和文件系统目录
4. **P3** 所有错误被 `.ok()` 静默吞掉，用户无感知

### 改动文件
- `src/task_db.rs` — `delete_task` 级联清理 5 张关联表
- `src/ui/nav.rs` — 删除前检查 `is_task_active`；删除后清理 JobManager 状态 + 文件系统目录；成功/失败均向 chat 发消息提示
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