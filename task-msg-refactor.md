# Task-Message 架构重构 — 进度追踪

## 任务列表

| # | 阶段 | 文件 | 描述 | 状态 |
|---|------|------|------|------|
| 1 | P1 | workspace.rs | TaskItem 扩展 + AppState 辅助方法 | ✅ |
| 1a | P1.5 | app_state.rs + nav.rs | 所有 TaskItem 构造处加新字段 | ✅ |
| 2 | P2 | app_state.rs + workspace.rs + chat.rs | 消息和滚动状态迁移到 TaskItem | ✅ |
| 3 | P3 | job_manager.rs | 所有 messages/scroll/summarize 操作迁移 | ✅ |
| 4 | P4 | routing.rs + app_state.rs | 剩余消息写入点迁移 | ✅ |
| 5 | P5 | 全项目 | cargo build 编译验证 | ✅ |

## 编译结果

```
cargo build → Finished (zero errors, 47 pre-existing warnings)
```

## 关键设计决策（实施中发现）

- `task_mut()` 参数类型定为 `Option<usize>` — 因为大多数调用方传的是 `Option`（run_task_id, active_task_id）
- `apply_general_ai_stream_event` 中提前 `let Some(run_task_id)` unwrap，内部直接用 `usize`
- 异步回调中提前 `let current_active_id = self.active_task_id` 避免借位冲突
- `chat_scroll_handle` 保留在 AppState（DOM 容器唯一）

## 改动对照

详见 `docs/architecture-refactor-task-messages.md`