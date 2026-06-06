# 编排与多项目支持升级任务进度 (Orchestration & Multi-project Support)

## Phase 0+4: 路径硬编码修复 + Session 透传 (Path Hardcoding Fix & Session Isolation) - [✅ 已完成]
- [x] **Step A: 结构体与接口改造**
  - [x] `src/agents/core/orchestrator.rs`: `Orchestrator` 结构体增加 `work_dir: std::sync::Mutex<std::path::PathBuf>` 字段
  - [x] `src/agents/core/orchestrator.rs`: `Orchestrator::new()` 增加 `work_dir` 参数
  - [x] `src/agents/core/factory.rs`: `AgentFactory::create_orchestrator()` 增加 `work_dir: PathBuf` 参数
- [x] **Step B: 调用链打通**
  - [x] `src/runtime/job_manager.rs`: `spawn_orchestrator_run` 获取并透传 `self.get_work_dir()`
  - [x] `src/agents/core/orchestrator.rs`: `run_sub_agent("coding")` 移除 `"."` 硬编码，改用 `self.work_dir.lock()`
  - [x] `src/agents/core/orchestrator.rs`: `run_sub_agent("coding")` 将 `context.session_id` 传给 `execute_instruction_stream`
- [x] **Step C: 验证通过**
  - [x] 编译通过 (`cargo check` 无新 error)
  - [x] 确认 Orchestrator 启动的 Claude Code 将运行在 Workspace Root 目录下

## Phase 1: Memory L3 写入修复 (Memory L3 Persistence Fix) - [✅ 已完成]
- [x] **Step A: 集成 L3 写入调用**
  - [x] `src/runtime/job_manager.rs`: 在 Orchestrator 结束回调的 snapshot 线程中调用 `save_task_memory_async`
- [x] **Step B: 验证通过**
  - [x] 编译通过，L2 snapshot + L3 chunk 在 orchestrator 完成任务后同步写入

## Phase 2: 文件改动实时感知 (Real-time Change Perception) - [✅ 已完成]
- [x] **Step A: Claude Code 输出格式调研**
  - [x] 实测结论：`--output-format stream-json` 的 `result` 字段是一个纯文本字符串，**没有结构化的文件变更元数据**
  - [x] 改为使用 `git diff --name-only` 方案：Claude Code 进程结束后检测 working tree 中的文件变更
- [x] **Step B: 协议与解析扩展**
  - [x] `src/agents/claude_code.rs`: `ClaudeStreamEvent` 增加 `ModifiedFiles { files: Vec<String> }`
  - [x] `src/agents/claude_code.rs`: 新增 `detect_modified_files()` 方法执行 `git diff --name-only`
  - [x] `src/agents/claude_code.rs`: 在 `execute_instruction_stream` 返回前发送 `ModifiedFiles` 事件
- [x] **Step C: UI 数据链路**
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