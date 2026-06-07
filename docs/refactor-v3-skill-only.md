# 重构方案 v3：Skill 接管全部 Claude Code 逻辑

## 核心原则

**主工程零 Claude Code 代码。** 主工程只通过 `run_system_task("coding_assistant", ...)` 与 Skill 交互，不直接调用 Claude Code CLI。

Skill 执行 Claude Code 时，通过回调将进度信息传回主工程，主工程在 UI 展示。

## 改动清单

### Phase A：创建 Skill

创建 `skills/coding_assistant/`，包含 `mod.rs` 和 `Cargo.toml`。

**Skill 接口：**

```rust
impl Skill for CodingAssistantSkill {
    fn id(&self) -> &str { "coding_assistant" }
    
    async fn preview(&self, args: Value) -> Result<SkillPreview> {
        // 阶段1：需求分析
        // 调 Claude Code CLI（带进度回调）：
        //   claude -p "分析以下编码需求需要哪些信息。只输出JSON格式：{task}"
        // 解析 JSON，返回预览 + 待确认字段列表
    }
    
    async fn execute(&self, args: Value) -> Result<SkillOutput> {
        // 有 confirmed → 阶段2：编码执行
        //   调 Claude Code CLI（带进度回调）：
        //     claude -p "根据以下完整需求编码：{task}，已确认信息：{confirmed}"
        //   返回编码结果（变更文件列表、执行摘要）
        
        // 无 confirmed → 阶段1：需求分析（同 preview）
    }
}
```

**Skill 进度回调：**

Skill 的 `preview` 和 `execute` 方法在执行 Claude Code 期间，通过一个 `on_progress` 回调将进度信息实时传回主工程：

```rust
// Skill 定义
pub struct CodingAssistantSkill {
    pub on_progress: Option<Box<dyn Fn(String) + Send>>,
}

// 执行 Claude Code 时，每行 stdout 都通过回调发送
if let Some(ref cb) = self.on_progress {
    cb(format!("[ClaudeCode] {line_type}: {content_preview}"));
}
```

主工程在调用 Skill 时传入回调：

```rust
// 主工程调用 Skill
let skill = CodingAssistantSkill {
    on_progress: Some(Box::new(|msg| {
        // 更新 UI 状态栏
        self.job_manager.request_status_text = Some(msg);
        cx.notify();
    })),
};
let result = skill.execute(args).await;
```

### Phase B：主工程删除所有 Claude Code 相关代码

| 文件 | 改动 |
|------|------|
| `src/agents/claude_code.rs` | **删除整个文件** |
| `src/agents/core/orchestrator.rs` | 删除 `run_sub_agent("coding")` 分支；删除 `CodingAgent` 引用；删除 `sub_agents` 中的 coding agent；删除 `OrchestratorEvent::SubAgentStream` 变体 |
| `src/agents/core/factory.rs` | 删除 `CodingAgent::new()` 调用 |
| `src/agents/core/main_agent.rs` | system prompt 中删除编码工作流说明；`run_claude_code` 工具删除（不再需要） |
| `src/agents/core/mod.rs` | 删除 `CodingAgent` 导出 |
| `src/agents/coding_agent.rs` | **删除整个文件**（如果有） |
| `src/agents/mod.rs` | 删除 `claude_code` 模块引用 |
| `src/runtime/job_manager.rs` | **删除 `spawn_claude_code_run`**；删除 `ClaudeRunPanelState` 相关状态；删除 `apply_claude_run_event`；删除 `update_subagent_message_event`；删除 `ClaudeStreamEvent` 引用；删除 `insert_subagent_message`、`continue_claude_with_answer`、`continue_subagent_with_answer` |
| `src/agents/types.rs` | 删除 `ClaudeRunPanelState`、`ClaudeRunEvent`、`ClaudeRunStatus`、`ClaudeRunTone`、`SubagentStatus`、`SubagentMessageState`、`SubagentEventEntry`、`SubagentEventTone`、`PendingQuestion`、`PreviewState`、`PreviewStatus`、`ArtifactEntry` 等 Claude Code 相关类型 |
| `src/ui/chat.rs` | 删除 subagent 卡片渲染；删除 `render_general_ai_pending_message`（改为 Skill 进度展示）；删除 `live_run` 和 `general_ai_live_run_id` 相关逻辑 |
| `src/runtime/events.rs` | **删除整个文件**（`ClaudeStreamEvent`、`OrchestratorWrapperEvent` 等全部移除） |
| `Cargo.toml` | 移除 `uuid` 依赖（不再需要生成 claude session id） |

### Phase C：保留和新增

**保留的组件：**

| 组件 | 说明 |
|------|------|
| `Orchestrator`（或重命名为 `MainLoop`） | 保留，作为 MainAgent 的执行循环 |
| `OrchestratorEvent` | 保留（去除 SubAgentStream 后），MainAgent 需要 Plan、AssistantDelta、StepFinished、AwaitingUserInput 等事件 |
| 多轮交互通道 | 保留（`user_input_rx` / `orchestrator_user_input_tx`），MainAgent 提问时暂停等待用户回复 |
| `restore_task_context` | 保留，用于 task 切换 |
| `cancel_current_run` | 保留，用于停止 Orchestrator |
| `route_message` | 保留，增加 Skill 进度输入检测 |
| `IntentRouter`（关键词池） | 保留（已清空），所有请求走 Orchestrator |

**新增的组件：**

| 组件 | 说明 |
|------|------|
| `Skill` 系统的 `on_progress` 回调 | 允许 Skill 在执行过程中传回进度信息 |
| `request_status_text` 在 Skill 执行时显示进度 | 替代 subagent 卡片，在输入框上方展示 Claude Code 执行进度 |
| `ContentPart::RequirementForm` | 后续优化，用于展示 Skill 返回的 JSON 需求清单表单 |

### Phase D：UI 表单渲染（后续优化）

| 文件 | 改动 |
|------|------|
| `src/memory/types.rs` | 新增 `ContentPart::RequirementForm` 变体 |
| `src/ui/components.rs` | 解析 JSON 表单定义，渲染选项按钮 + 输入框 + 提交按钮 |
| `src/ui/chat.rs` | 新增 `RequirementForm` 的渲染分支 |

## 实施顺序

```
Phase A：创建 Skill
  → 不依赖主工程改动，可独立验证 Skill 能调 Claude Code
  
Phase B：主工程删除 Claude Code 代码
  → 编译通过
  → 验证编码功能通过 Skill 调用正常

Phase C：Skill 进度回调 + UI 整合
  → Skill 执行期间输入框显示进度
  → 编码结果以文本形式展示在聊天区

Phase D：UI 表单渲染（可选，后续优化）
  → JSON 需求清单渲染为交互式表单
```

## 风险

- **Skill 内部调 Claude Code 需要访问 `Config`（模型配置）** — 当前 Skill 的 `execute` 参数是否有途径访问主工程的配置？需要确认 `Skill` trait 的签名是否支持。
- **删掉 `spawn_claude_code_run` 后，Skill 内部 `Command::new("claude")` 需要自己管理进程** — 包括 stdout 解析、超时、取消等。这增加了 Skill 的复杂度。
- **`on_progress` 回调的线程安全** — Skill 可能在异步上下文中执行，回调需要 `Send + Sync`。
- **`OrchestratorEvent::SubAgentStream` 删除后**，子代理实时流的事件路由消失，`OrchestratorEvent` 仅保留 Plan、AssistantDelta、StepFinished、AwaitingUserInput、ToolCall、ToolResult。