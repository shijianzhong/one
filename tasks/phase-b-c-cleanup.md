# Phase B+C: 主工程清理 + Skill 整合

## 目标

主工程中删除所有与 Claude Code 直接交互的代码，改为通过 `coding_assistant` Skill 间接调用。Skill 执行期间通过 `on_progress` 回调在 UI 展示进度。

## 改动清单

### Phase B: 主工程删除所有 Claude Code 代码

#### B-1: `src/agents/claude_code.rs` ✅ 已完成
- [x] 删除整个文件
- [x] 从 `src/agents/mod.rs` 删除 `pub mod claude_code`

#### B-2: `src/agents/core/mod.rs` ✅ 已完成
- [x] 删除 `pub mod coding_agent`
- [x] 删除 `pub use coding_agent::CodingAgent`

#### B-3: `src/agents/core/factory.rs` ✅ 已完成
- [x] 删除 `CodingAgent` 引用
- [x] 删除 `CodingAgent::new()` 调用
- [x] 从 `sub_agents` 中删除 coding agent

#### B-4: `src/agents/core/orchestrator.rs` ✅ 已完成
- [x] 删除 `ClaudeStreamEvent` 引用
- [x] 删除 `run_sub_agent("coding")` 整个分支
- [x] 删除 `OrchestratorEvent::SubAgentStream` 变体
- [x] 保留 `AwaitingUserInput` 变体

#### B-5: `src/runtime/events.rs` ✅ 已完成
- [x] 删除整个文件
- [x] 从 `src/runtime/mod.rs` 删除 `pub mod events` 和 `pub use events::*`

#### B-6: `src/runtime/job_manager.rs` 🔲 待完成（大量修改）
- [ ] 删除所有 `ClaudeRunPanelState`、`ClaudeRunEvent`、`ClaudeRunStatus` 等类型引用
- [ ] 删除 `current_claude_run`、`claude_child_pid`、`pending_claude_question` 字段
- [ ] 删除 `spawn_claude_code_run`、`apply_claude_run_event`、`continue_claude_with_answer` 等方法
- [ ] 删除 `insert_subagent_message`、`update_subagent_message_event`、`SubagentMessageState` 引用
- [ ] 删除 `SubAgentStream` 事件处理（从 orchestrator 事件 match 中）
- [ ] 删除 `persist_current_claude_state`、`load_claude_state_for_task` 等方法
- [ ] 删除 `begin_claude_run` 方法
- [ ] 保留 `spawn_orchestrator_run`、`cancel_current_run`（简化）、多轮交互、summarize、system tools

#### B-7: `src/agents/types.rs` 🔲 待完成
- [ ] 删除 `ClaudeRunPanelState`、`ClaudeRunStatus`、`ClaudeRunTone`、`ClaudeRunEvent`
- [ ] 删除 `SubagentStatus`、`SubagentMessageState`、`SubagentEventEntry`、`SubagentEventTone`
- [ ] 删除 `PendingQuestion`、`PreviewState`、`PreviewStatus`、`ArtifactEntry`
- [ ] 删除 `RequestKind::ClaudeCode` 枚举变体
- [ ] 删除 `RoutingDecision::ClaudeCode` 枚举变体

#### B-8: `src/ui/chat.rs` 🔲 待完成
- [ ] 删除 `live_run`（Claude Code 运行状态）相关代码
- [ ] 删除 subagent 卡片渲染
- [ ] 删除 `render_general_ai_pending_message`（改为 Skill 进度展示）
- [ ] 删除 `ClaudeRunPanelState` 引用

#### B-9: `src/app_state.rs` 🔲 待完成
- [ ] 删除 `PendingClaudeQuestion` 结构体
- [ ] 删除 `claude_meta` 相关引用

#### B-10: `src/workspace.rs` 🔲 待完成
- [ ] 删除 `ArtifactEntry` 引用（如果不再需要）
- [ ] 删除 `load_artifacts_for_task_dir`（如果不再需要）

#### B-11: `src/main.rs` 🔲 待完成
- [ ] 删除 `RequestKind::ClaudeCode` 引用（如果存在）

#### B-12: `src/i18n/mod.rs` 🔲 待完成
- [ ] 删除 Claude Code 相关的翻译键（if any）

#### B-13: `Cargo.toml` 🔲 待完成
- [ ] 删除 `uuid` 依赖

### Phase C: Skill 进度回调 + UI 整合

#### C-1: Skill 进度回调
- [ ] `src/skills/coding_assistant.rs` — 确保 `on_progress` 回调在 `run_claude` 的 stdout 解析循环中每行调用
- [ ] `src/agents/core/orchestrator.rs` — `execute_tool_calls_and_feed_back` 中的 `run_system_task` 工具调用检测到 `coding_assistant` 时，传入 on_progress 回调
- [ ] `src/runtime/job_manager.rs` — 接收 Skill 的进度回调，更新 `request_status_text`

#### C-2: UI 展示 Skill 执行进度
- [ ] `src/ui/chat.rs` — `render_composer` 底部状态栏显示 Skill 执行进度（替代 subagent 卡片）
- [ ] 进度格式：`"正在分析需求... [Claude Code] assistant: xxx"` 或 `"正在编码... [Claude Code] tool_use: Write"`

#### C-3: 编码结果展示
- [ ] Skill 的 `executor` 返回的结果（`summary` + `success_items`）展示在聊天区作为普通 assistant 消息
- [ ] 如果结果中包含文件列表，在消息下方展示文件列表

## 实施建议

**建议分批提交：**

1. **Batch 1（一次性提交）**：B-6, B-7, B-8, B-9, B-10, B-11, B-12, B-13 — 这些相互依赖，需要一起改
2. **Batch 2**：C-1, C-2 — Skill 进度回调和 UI 展示
3. **Batch 3**：C-3 — 编码结果展示优化

## 风险

- Batch 1 涉及 6+ 个文件的大范围删除，建议改完后 `cargo check` 逐步修复
- `RequestKind::ClaudeCode` 和 `RoutingDecision::ClaudeCode` 的删除可能影响路由逻辑
- Skill 的 on_progress 回调需要在异步上下文中跨线程调用，注意 `Send + Sync`
- 删除 `subagent_messages` 前，确认这些数据没有在其他地方被引用（如 UI 渲染、task 切换时的清理等）