# Task-Message 架构重构方案

## Context

当前 `AppState` 全局持有 `messages: Vec<ChatMessage>`, `pending_summarize: bool`, `needs_auto_scroll: bool`, `chat_scroll_handle: ScrollHandle`, `think_collapsed: HashMap<String, bool>` 等字段。所有 task 共享这些状态。切换 task 时通过 `restore_task_context()` 从数据库 reload 消息。异步回调依赖当前 `active_task_id` 判断是否更新 UI，导致：

- 切换 task 后，原 task 的 AI 回复完了但不更新 UI（消息虽然写进 DB 了但不可见）
- summarize 跑到了切换后的 task 上
- `needs_auto_scroll` 跨 task 污染

## 目标

**Task 自包含** — 每个 TaskItem 持有自己的 messages 和相关状态。切换 task 就是换一个 TaskItem 引用。

## 改动全貌（按文件分类）

### 1. `src/workspace.rs` — TaskItem 增加字段

**TaskItem 当前定义：**

```rust
pub struct TaskItem {
    pub id: usize,
    pub title: String,
    pub is_draft: bool,
}
```

**改为：**

```rust
use crate::memory::types::ChatMessage;

pub struct TaskItem {
    pub id: usize,
    pub title: String,
    pub is_draft: bool,
    pub messages: Vec<ChatMessage>,
    pub pending_summarize: bool,
    pub needs_auto_scroll: bool,
}
```

**AppState 移除字段（共 5 个）：**

```rust
// 从 AppState 中删除：
messages: Vec<ChatMessage>,              // app_state.rs:197
needs_auto_scroll: bool,                 // app_state.rs:198
pending_summarize: bool,                 // app_state.rs:199
chat_scroll_handle: ScrollHandle,        // app_state.rs:202
think_collapsed: HashMap<String, bool>,  // app_state.rs:209
```

**新增辅助方法到 AppState：**

```rust
/// 获取当前 active_task 的可变引用
pub(crate) fn active_task_mut(&mut self) -> Option<&mut TaskItem> {
    let tid = self.active_task_id?;
    self.workspaces.iter_mut()
        .flat_map(|w| &mut w.tasks)
        .find(|t| t.id == tid)
}

/// 按 task_id 获取任意 task 的可变引用
pub(crate) fn task_mut(&mut self, task_id: usize) -> Option<&mut TaskItem> {
    self.workspaces.iter_mut()
        .flat_map(|w| &mut w.tasks)
        .find(|t| t.id == task_id)
}

/// 获取当前 active_task 的不可变引用
pub(crate) fn active_task_ref(&self) -> Option<&TaskItem> {
    let tid = self.active_task_id?;
    self.workspaces.iter()
        .flat_map(|w| &w.tasks)
        .find(|t| t.id == tid)
}
```

### 2. `src/workspace.rs` — restore_task_context 改为从 DB 加载到 task.messages

```rust
pub fn restore_task_context(&mut self) {
    // ── 清理前一个 task 的运行状态，防止污染新 task ───────────
    self.job_manager.request_in_flight = false;
    self.job_manager.request_kind = None;
    self.job_manager.request_status_text = None;
    self.job_manager.general_ai_run_id = None;
    self.job_manager.general_ai_task_id = None;
    self.job_manager.general_ai_show_live_bubble = false;
    self.job_manager.general_ai_live_text.clear();
    // 注意：不要关闭 orchestrator_user_input_tx！

    if let Some((workspace_id, task_id, title)) = self.get_active_task_location() {
        let _ = self.ensure_task_storage_dir(workspace_id, task_id, &title);
        let msgs = task_db::load_messages(&self.db.conn, task_id).unwrap_or_default();
        let msg_vec: Vec<ChatMessage> = msgs
            .into_iter()
            .map(|m| ChatMessage::new(&m.role, &m.content))
            .collect();
        if let Some(task) = self.task_mut(task_id) {
            task.messages = msg_vec;
        }
    }
    // else 不用清空，因为没有 active_task 时 task_mut 返回 None
}
```

### 3. `src/app_state.rs` — 所有 TaskItem 创建处增加 messages 和 pending_summarize

**3a. app_state.rs ~L152-159 — 初始化加载 tasks**

```rust
let tasks = task_db::load_tasks(conn, w.id)
    .unwrap_or_default()
    .into_iter()
    .map(|t| TaskItem {
        id: t.id,
        title: t.title,
        is_draft: t.is_draft,
        messages: vec![],       // + 新增
        pending_summarize: false,
        needs_auto_scroll: false,
    })
    .collect();
```

**3b. app_state.rs ~L469-476 — ensure_workspace_draft_task 重新加载**

```rust
self.workspaces[ws_index].tasks = rows
    .into_iter()
    .map(|t| TaskItem {
        id: t.id,
        title: t.title,
        is_draft: t.is_draft,
        messages: vec![],       // + 新增
        pending_summarize: false,
        needs_auto_scroll: false,
    })
    .collect();
```

**3c. src/ui/nav.rs ~L359-363 — 删除失败回滚**

```rust
ws.tasks = rows
    .into_iter()
    .map(|t| TaskItem {
        id: t.id,
        title: t.title,
        is_draft: t.is_draft,
        messages: vec![],       // + 新增
        pending_summarize: false,
        needs_auto_scroll: false,
    })
    .collect();
```

**3d. 从 AppState::new 的 struct literal 中删除：**

```rust
messages: vec![],              // 删除
needs_auto_scroll: false,       // 删除
pending_summarize: false,       // 删除
chat_scroll_handle: ScrollHandle::default(),  // 删除
think_collapsed: HashMap::new(),              // 删除
```

### 4. `src/ui/chat.rs` — 消息读取改为从 task.messages

**4a. render_chat (L28)**

```rust
// 删除：let scroll_handle = self.chat_scroll_handle.clone();
// 改为：clone 当前 task 的 scroll_handle？不，scroll_handle 还是 AppState 的，因为 track_scroll 绑定到同一个 dom 元素
// 实际：chat_scroll_handle 保留在 AppState，只是 messages、needs_auto_scroll、pending_summarize 移到 TaskItem
// 所以 L28 不变
```

**4b. render_chat_messages (L324)**

```rust
// 旧：
let messages = self.messages.clone();
// 新：
let messages = self.active_task_ref()
    .map(|t| t.messages.clone())
    .unwrap_or_default();
```

**4c. render_chat_messages (L336-338) — auto scroll**

```rust
// 旧：
if self.needs_auto_scroll && !messages.is_empty() {
    scroll_handle.scroll_to_bottom();
    self.needs_auto_scroll = false;
}
// 新：
let should_scroll = self.active_task_ref()
    .map(|t| t.needs_auto_scroll)
    .unwrap_or(false);
if should_scroll && !messages.is_empty() {
    scroll_handle.scroll_to_bottom();
    if let Some(task) = self.active_task_mut() {
        task.needs_auto_scroll = false;
    }
}
```

**4d. render_composer (L966-969) — 第一消息检测**

```rust
// 旧：
let is_first_message = this.messages.is_empty();
if is_first_message {
    this.pending_summarize = true;
}
// 新：
let is_first_message = this.active_task_ref()
    .map(|t| t.messages.is_empty())
    .unwrap_or(true);
if is_first_message {
    if let Some(task) = this.active_task_mut() {
        task.pending_summarize = true;
    }
}
```

**4e. render_composer (L1017-1020) — 第二处第一消息检测（同上）**

```rust
// 同上
```

**4f. render_chat_messages (L346) — task_id 获取（不变）**

```rust
let task_id = self.active_task_id.unwrap_or_default();
// 这个不变，already gets from active_task_id
```

**4g. render_chat_messages (L477-479) — think_collapsed 使用**

`think_collapsed` 也移到了 TaskItem 里，因为它是跟每个 task 的 chat messages 渲染相关的状态。

```rust
// 在 TaskItem 中新增：
pub think_collapsed: HashMap<String, bool>,
```

L477-479:
```rust
// 旧：
let collapsed = self.think_collapsed.get(&key).copied().unwrap_or(complete);
// 新：
let collapsed = self.active_task_ref()
    .and_then(|t| t.think_collapsed.get(&key).copied())
    .unwrap_or(complete);
```

L501-502:
```rust
// 旧：
let next = !this.think_collapsed.get(&key).copied().unwrap_or(default_collapsed);
this.think_collapsed.insert(key.clone(), next);
// 新：
let next = !this.active_task_ref()
    .and_then(|t| t.think_collapsed.get(&key).copied())
    .unwrap_or(default_collapsed);
if let Some(task) = this.active_task_mut() {
    task.think_collapsed.insert(key.clone(), next);
}
```

L662, L690-695 (general_ai_live_message 中的 think_collapsed) — 同上模式。

### 5. `src/routing.rs` — 消息 push 改为写入 task

**Line 11:**
```rust
// 旧：
self.messages.push(ChatMessage::new("user", &message));
// 新：
if let Some(task) = self.active_task_mut() {
    task.messages.push(ChatMessage::new("user", &message));
}
```

**Line 15:**
```rust
// 旧：
self.needs_auto_scroll = true;
// 新：
if let Some(task) = self.active_task_mut() {
    task.needs_auto_scroll = true;
}
```

**Line 57 (handle_routing_decision):**
```rust
// 旧：
let last_msg = self.messages.last().map(|m| m.content.clone()).unwrap_or_default();
// 新：
let last_msg = self.active_task_ref()
    .and_then(|t| t.messages.last().map(|m| m.content.clone()))
    .unwrap_or_default();
```

### 6. `src/runtime/job_manager.rs` — 所有 messages 操作

**6a. apply_general_ai_stream_event — needs_auto_scroll (L114)**

```rust
// 旧：
self.needs_auto_scroll = run_task_id == self.active_task_id;
// 新：
if run_task_id == self.active_task_id {
    if let Some(task) = self.active_task_mut() {
        task.needs_auto_scroll = true;
    }
}
```

注意：这里语义微调。旧的写法是 `needs_auto_scroll = run_task_id == self.active_task_id`，每次 delta 都会设置。新写法只在匹配时设置为 true（因为不匹配时不需要设置为 false）。效果一致。

**6b. Finished — CONFIRM_REQUIRED (L130-132)**

```rust
// 旧：
if run_task_id == self.active_task_id {
    self.messages.push(ChatMessage::new("assistant", &dangerous_msg));
    self.needs_auto_scroll = true;
}
// 新：
if run_task_id == self.active_task_id {
    if let Some(task) = self.task_mut(run_task_id) {
        task.messages.push(ChatMessage::new("assistant", &dangerous_msg));
        task.needs_auto_scroll = true;
    }
}
```

**6c. Finished — 正常完成 (L145-148)**

```rust
// 旧：
if run_task_id == self.active_task_id {
    self.messages.push(ChatMessage::new("assistant", &content));
    self.needs_auto_scroll = true;
}
// 新：
if let Some(task) = self.task_mut(run_task_id) {
    task.messages.push(ChatMessage::new("assistant", &content));
    if run_task_id == self.active_task_id {
        task.needs_auto_scroll = true;
    }
}
```

注意：消息始终写入对应 task（即使不是 active），但 needs_auto_scroll 只在当前显示这个 task 时才设置。

**6d. Finished — pending_summarize (L153-156)**

```rust
// 旧：
if self.pending_summarize && run_task_id == self.active_task_id {
    self.pending_summarize = false;
    self.spawn_summarize_job(cx);
}
// 新：
if let Some(task) = self.task_mut(run_task_id) {
    if task.pending_summarize {
        task.pending_summarize = false;
        self.spawn_summarize_job(run_task_id, cx);
    }
}
```

**6e. Failed (L167-169)**

```rust
// 旧：
if run_task_id == self.active_task_id {
    self.messages.push(ChatMessage::new("assistant", &error_message));
    self.needs_auto_scroll = true;
}
// 新：
if let Some(task) = self.task_mut(run_task_id) {
    task.messages.push(ChatMessage::new("assistant", &error_message));
    if run_task_id == self.active_task_id {
        task.needs_auto_scroll = true;
    }
}
```

**6f. spawn_summarize_job (L179-225) — 改为接收 task_id 参数**

```rust
// 旧签名：
fn spawn_summarize_job(&mut self, cx: &mut Context<Self>)
// 新签名：
fn spawn_summarize_job(&mut self, task_id: usize, cx: &mut Context<Self>)
```

内部：
```rust
// 旧：
let task_id = self.active_task_id;
let all_messages = self.messages.clone();
let Some(tid) = task_id else { return; };
// 新：
// task_id 直接参数传入（一定是 Some）
let all_messages = self.task_mut(task_id)
    .map(|t| t.messages.clone())
    .unwrap_or_default();
```

**6g. confirm_system_tools_operation — 取消 (L365)**

```rust
// 旧：
self.messages.push(ChatMessage::new("assistant", "操作已取消。"));
// 新：
if let Some(task) = self.active_task_mut() {
    task.messages.push(ChatMessage::new("assistant", "操作已取消。"));
}
```

**6h. confirm_system_tools_operation — 无法解析 (L375-376)**

```rust
// 旧：
self.messages.push(ChatMessage::new("assistant", "无法解析操作指令。"));
// 新：
if let Some(task) = self.active_task_mut() {
    task.messages.push(ChatMessage::new("assistant", "无法解析操作指令。"));
}
```

**6i. spawn_orchestrator_run — orchestrator 创建失败 (L467-469)**

```rust
// 旧：
self.messages.push(ChatMessage::new(
    "assistant",
    &format!("Failed to create orchestrator: {}", e),
));
// 新：
if let Some(task) = self.active_task_mut() {
    task.messages.push(ChatMessage::new(
        "assistant",
        &format!("Failed to create orchestrator: {}", e),
    ));
}
```

**6j. spawn_orchestrator_run — history (L511)**

```rust
// 旧：
let history = self.messages.clone();
// 新：
let history = self.active_task_ref()
    .map(|t| t.messages.clone())
    .unwrap_or_default();
```

**6k. AwaitingUserInput (L603-607)**

```rust
// 旧：
if this.active_task_id == active_task_id {
    this.messages.push(ChatMessage::new("assistant", &reply));
}
// 新：
if let Some(task) = this.task_mut(active_task_id) {
    task.messages.push(ChatMessage::new("assistant", &reply));
    if this.active_task_id == active_task_id {
        task.needs_auto_scroll = true;
    }
}
```

**6l. Finished — orchestrator 完成 (L679-680)**

```rust
// 旧：
if this.active_task_id == active_task_id {
    this.messages.push(ChatMessage::new("assistant", &result));
}
// 新：
if let Some(task) = this.task_mut(active_task_id) {
    task.messages.push(ChatMessage::new("assistant", &result));
    if this.active_task_id == active_task_id {
        task.needs_auto_scroll = true;
    }
}
```

**6m. Finished — pending_summarize (L692-695)**

```rust
// 旧：
if this.pending_summarize && this.active_task_id.is_some() {
    this.pending_summarize = false;
    this.spawn_summarize_job(cx);
}
// 新：
if let Some(task) = this.task_mut(active_task_id) {
    if task.pending_summarize {
        task.pending_summarize = false;
        this.spawn_summarize_job(active_task_id, cx);
    }
}
```

**6n. Finished — needs_auto_scroll (L696)**

```rust
// 旧：
this.needs_auto_scroll = true;
// 新：
if let Some(task) = this.task_mut(active_task_id) {
    if this.active_task_id == active_task_id {
        task.needs_auto_scroll = true;
    }
}
```

**6o. Failed — orchestrator (L715-719)**

```rust
// 旧：
if this.active_task_id == active_task_id {
    this.messages.push(ChatMessage::new("assistant", &format!(...)));
}
// 新：
if let Some(task) = this.task_mut(active_task_id) {
    task.messages.push(ChatMessage::new("assistant", &format!(...)));
    if this.active_task_id == active_task_id {
        task.needs_auto_scroll = true;
    }
}
```

**6p. Failed — needs_auto_scroll (L731)**

```rust
// 旧：
this.needs_auto_scroll = true;
// 新：
if let Some(task) = this.task_mut(active_task_id) {
    if this.active_task_id == active_task_id {
        task.needs_auto_scroll = true;
    }
}
```

### 7. `src/app_state.rs` — soul/deny messages

**7a. approve_soul_proposal (L312-315, L324)**

```rust
// 旧：
self.messages.push(ChatMessage::new("assistant", "..."));
// ...
self.needs_auto_scroll = true;
// 新：
if let Some(task) = self.active_task_mut() {
    task.messages.push(ChatMessage::new("assistant", "..."));
    // ...
    task.needs_auto_scroll = true;
}
```

**7b. deny_soul_proposal (L331-335)**

```rust
// 同上模式
```

**7c. launch_skill_card (L347-349, L351)**

```rust
// 同上模式
```

### 8. `src/main.rs` — 删除 messages/pending_summarize 等导出

检查 `src/main.rs` 是否有 `pub(crate) use app_state::{..., messages, ...}`。看起来没有，messages 没被直接导出到 main 的公共 API。

### 9. 其他文件检查

`src/ui/mod.rs` — 在 Render impl 中没有直接引用 messages、pending_summarize、needs_auto_scroll（scroll_handle 在 chat.rs 中通过 self 访问）。不需要改。

`src/ui/nav.rs` — 之前已经改为用 push_toast，没有 references to messages/pending_summarize。不需要改。

`src/memory/snapshot.rs` — 不引用 self.messages，通过参数传入消息列表。不需要改。

## 实现步骤

### Phase 1: TaskItem 扩展

1. `src/workspace.rs` — TaskItem 增加 5 个字段 + AppState 辅助方法
2. `src/app_state.rs` — 3 处 TaskItem 构造加新字段
3. `src/ui/nav.rs` — 1 处 TaskItem 构造加新字段

> 这一步改完可以 `cargo build` 确认编译通过

### Phase 2: 迁移消息和 scroll 状态

1. AppState struct literal 中删除 `messages`, `needs_auto_scroll`, `pending_summarize`, `chat_scroll_handle`, `think_collapsed`
2. restore_task_context 改为写入 task.messages
3. chat.rs 中 messages 读取、needs_auto_scroll、pending_summarize、think_collapsed 全部改为从 active_task 读取

> 这一步改完可以 `cargo build`

### Phase 3: 迁移 job_manager 中的 messages 写入

按 6a-6p 逐个替换所有 `this.messages.push(...)` 和 `this.needs_auto_scroll = ...` 和 `this.pending_summarize` 操作

> 这一步改完可以 `cargo build`

### Phase 4: 迁移 routing 和 app_state

1. routing.rs — 3 处 messages 操作
2. app_state.rs — soul/card 的 messages 和 needs_auto_scroll

> 最终 `cargo build` 确认全项目编译通过

## 不变的点

- DB schema（messages 表不变）
- ChatMessage 结构体不变
- services/api.rs 所有 API 调用签名不变（它们通过参数接收 `&[ChatMessage]`）
- memory/snapshot.rs 的 `generate_snapshot_sync` 签名不变
- task_db.rs 所有函数不动
- ui/mod.rs Render 结构不变
- ui/nav.rs 不变

## 验证

1. `cargo build` 零 warning
2. 新建 Task A，输入消息 → AI 回复 → 消息显示在聊天区 → 切到 Task B → 切回 A → 消息完整，标题已 summarize
3. 新建 Task B，输入消息 → AI 回复期间切回 Task A → 再切回 B → B 的消息和标题不受影响
4. 多个 task 同时有 AI 在跑 → 各自 messages 互不污染
5. 删除 task → 不影响其他 task 状态
6. 点击 think 折叠/展开 → 只影响当前 task 的渲染状态
7. 每次切 task → 滚动到最新消息