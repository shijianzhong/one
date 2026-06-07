# Fix: Orchestrator 场景下子代理提问无法回复

## 问题

当 Orchestrator 内的 Coding Agent 调用 Claude Code 执行任务时，Claude Code 可能向用户提问（如"使用什么技术栈？"）。当前架构下：

1. `AskUserQuestion` 事件通过 `SubAgentStream` 路由到了 `update_subagent_message_event`
2. `pending_claude_question` 被设置，但 **UI 没有读取它**
3. 输入框上的按钮因为 `request_in_flight == true` 显示为"停止"，用户无法发送回答
4. 即使能发送，`continue_claude_with_answer` 会启动**新的** Claude Code 进程，而非回复当前进程

## 方案原理

将 claude 子进程的 stdin 从 `Stdio::null()` 改为 `Stdio::piped()`，当检测到 `AskUserQuestion` 时，通过 oneshot channel 将问题上报给 Orchestrator 主循环，主循环暂停等待用户回答。用户回答后，通过 stdin pipe 直接写入当前 claude 进程。

## 改动清单

### 1. `src/agents/claude_code.rs`

**改动 A：stdin 改为 pipe 模式**

```rust
// 当前
.stdin(Stdio::null())

// 改为
.stdin(Stdio::piped())
```

**改动 B：暴露 stdin 写入方法**

新增函数 `answer_question`，接受子进程 stdin 和回答内容，写入 stdin 后关闭 pipe 让 claude 继续：

```rust
pub fn answer_question(stdin: &mut ChildStdin, answer: &str) -> Result<()> {
    use std::io::Write;
    stdin.write_all(answer.as_bytes())?;
    stdin.write_all(b"\n")?;
    stdin.flush()?;
    Ok(())
}
```

**改动 C：`execute_instruction_stream` 返回 `(Result<String>, Option<ChildStdin>)`**

当前返回 `Result<String>`，需要同时返回 stdin handle 以便外部写入。或通过回调方式在提问时传出 stdin handle。

**更优设计**：不修改返回值，而是在 `execute_instruction_stream` 内部通过 sender 把 stdin handle 传出去：

```rust
pub fn execute_instruction_stream(
    project_dir: &PathBuf,
    instruction: &str,
    session_id: Option<&str>,
    sender: UnboundedSender<ClaudeStreamEvent>,
    cancel_flag: Option<Arc<AtomicBool>>,
    child_pid: Option<&AtomicU32>,
    // 新增：当 claude 提问时，通过此 sender 把 stdin 传给外部
    stdin_sender: Option<tokio::sync::oneshot::Sender<std::process::ChildStdin>>,
) -> Result<String> {
```

或更简单：在 `Started` 事件后立即发送 stdin handle 到 sender。

---

### 2. `src/agents/core/orchestrator.rs`

**改动 A：coding agent 分支检测 `AskUserQuestion` 并暂停等待**

在 `run_sub_agent("coding")` 的 event loop 中，当收到 `ClaudeStreamEvent::AskUserQuestion` 时：

```rust
ClaudeStreamEvent::AskUserQuestion { prompt, options } => {
    // 1. 创建一个 oneshot channel 用于接收用户回答
    let (answer_tx, answer_rx) = tokio::sync::oneshot::channel::<String>();
    
    // 2. 通过 on_event 将问题 + answer_tx 上报
    on_event(OrchestratorEvent::SubAgentQuestion {
        agent_id: agent_id.clone(),
        prompt,
        options,
        answer_tx,  // 外部通过此 channel 发回回答
    });
    
    // 3. 等待用户回答
    match answer_rx.await {
        Ok(answer) => {
            // 4. 将回答写入 claude 进程的 stdin
            if let Some(ref mut stdin) = child_stdin {
                let _ = stdin.write_all(answer.as_bytes());
                let _ = stdin.write_all(b"\n");
                let _ = stdin.flush();
            }
        }
        Err(_) => {
            // 用户取消了回答，返回
            return Ok("用户取消了操作".to_string());
        }
    }
}
```

**注意**：这要求 coding agent 分支能访问到 child_stdin。需要调整代码结构，让 child_stdin 在 event loop 的生命周期内可用。

---

### 3. `src/agents/core/orchestrator.rs` — `OrchestratorEvent` 枚举

新增事件变体：

```rust
pub enum OrchestratorEvent {
    // ... 已有变体 ...
    
    /// Sub-agent 向用户提问，等待回答
    SubAgentQuestion {
        agent_id: String,
        prompt: String,
        options: Vec<String>,
        answer_tx: tokio::sync::oneshot::Sender<String>,
    },
}
```

---

### 4. `src/runtime/job_manager.rs` — 处理 `SubAgentQuestion` 事件

在 orchestrator 事件处理循环中新增分支：

```rust
OrchestratorEvent::SubAgentQuestion { agent_id, prompt, options, answer_tx } => {
    // 存储 answer_tx，等待用户回答
    this.job_manager.pending_subagent_answer = Some((
        agent_id,
        prompt.clone(),
        options.clone(),
        answer_tx,
    ));
    
    // 设置 UI 提示
    this.job_manager.request_status_text = Some(format!("问题：{}", prompt));
    
    // 存储问题到 pending_claude_question（兼容现有 UI）
    this.job_manager.pending_claude_question = Some(crate::app_state::PendingClaudeQuestion {
        prompt,
        options,
        source_run_id: 0,
        session_id: None,
    });
}
```

并在 `cancel_current_run` 中清理 `pending_subagent_answer`。

**新增：`continue_subagent_with_answer` 方法**

```rust
pub(crate) fn continue_subagent_with_answer(&mut self, answer: String) {
    if let Some((_agent_id, _prompt, _options, answer_tx)) = 
        self.job_manager.pending_subagent_answer.take() 
    {
        let _ = answer_tx.send(answer);
        self.job_manager.pending_claude_question = None;
    }
}
```

---

### 5. `src/ui/chat.rs` — 发送按钮增加 pending_question 判断

```rust
.on_mouse_down(..., cx.listener(move |this, ...| {
    // ── 有子代理问题待回答 ────────────────────────
    if this.job_manager.pending_subagent_answer.is_some() {
        if let Some(editor) = weak_composer.upgrade() {
            let text = editor.read_with(cx, |editor, cx| editor.text(cx)).trim().to_string();
            if !text.is_empty() {
                editor.update(cx, |editor, cx| {
                    editor.set_text("", _window, cx);
                });
                this.continue_subagent_with_answer(text);
            }
        }
        return;
    }
    
    // ── 有 Claude Code 问题待回答 ────────────────
    if this.job_manager.pending_claude_question.is_some() {
        if let Some(editor) = weak_composer.upgrade() {
            let text = editor.read_with(cx, |editor, cx| editor.text(cx)).trim().to_string();
            if !text.is_empty() {
                editor.update(cx, |editor, cx| {
                    editor.set_text("", _window, cx);
                });
                this.continue_claude_with_answer(text, cx);
            }
        }
        return;
    }
    
    // ── 运行中 → 停止 ───────────────────────────
    if this.job_manager.request_in_flight {
        this.cancel_current_run(cx);
        return;
    }
    
    // ── 正常发送 ────────────────────────────────
    ...
}))
```

按钮标签也相应调整：

```rust
let send_label = if this.job_manager.pending_subagent_answer.is_some() {
    "回答"
} else if this.job_manager.pending_claude_question.is_some() {
    "回答"
} else if request_in_flight {
    "停止"
} else {
    t(lang, Translations::SEND)
};
```

---

### 6. 选项按钮 UI（当 options 不为空时）

在 `render_composer` 中，`pending_claude_question.options` 不为空时，在输入框上方渲染选项按钮：

```rust
if let Some(ref question) = self.job_manager.pending_claude_question {
    if !question.options.is_empty() {
        // 渲染选项按钮
        for opt in &question.options {
            div()
                .px_3()
                .py_1()
                .rounded_md()
                .bg(SURFACE_ELEVATED())
                .cursor_pointer()
                .on_mouse_down(gpui::MouseButton::Left, cx.listener(move |this, ...| {
                    this.continue_subagent_with_answer(opt.clone());
                }))
                .child(opt)
        }
    }
}
```

---

## 改动汇总

| # | 文件 | 改动 | 复杂度 |
|---|------|------|--------|
| 1 | `src/agents/claude_code.rs` | stdin 改为 pipe；`execute_instruction_stream` 在 `Started` 后将 stdin 通过 channel 传出 | 中 |
| 2 | `src/agents/core/orchestrator.rs` | 新增 `OrchestratorEvent::SubAgentQuestion`；coding 分支检测 `AskUserQuestion` 时暂停等待回答，写入 stdin | 高 |
| 3 | `src/runtime/job_manager.rs` | 新增 `pending_subagent_answer` 字段；新增 `continue_subagent_with_answer()`；`cancel_current_run` 清理新增字段；处理 `SubAgentQuestion` 事件 | 中 |
| 4 | `src/ui/chat.rs` | 发送按钮增加 pending_question 判断；按钮标签按状态变化；options 不为空时渲染选项按钮 | 中 |

---

## 风险

- **stdin pipe 的写入时机**：Claude Code 的 `AskUserQuestion` 意味着它正在等待 stdin 输入。写入后 claude 进程会继续处理回答。但 claude 可能期望特定的交互格式（如选择编号、输入文本等）。需要按 claude CLI 的交互方式写入。
- **`execute_instruction_stream` 在 `AskUserQuestion` 后继续**：当用户回答写入 stdin 后，claude 进程会输出新的内容（`AssistantText`、`ToolUse` 等）。coding agent 的 event loop 继续读取这些事件，正常处理。
- **oneshot channel 的 await**：在 coding agent 的 async event loop 中 await oneshot receiver 是安全的，不会阻塞 tokio runtime。