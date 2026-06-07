# Telegram + GPUUI Task 整合方案（最终版）

>基于全部关键代码阅读的综合分析，2026-06-07

---

## 一、代码全局架构图

```
┌──────────────────────────────────────────────────────────────────────┐
│                        同一进程: ONE.exe                            │
│                                                                  │
│  ┌─────────────────────────┐    ┌──────────────────────────────┐  │
│  │     GPUUI 线程          │    │    Telegram 线程 (tokio)    │  │
│  │                         │    │                              │  │
│  │  AppState {            │    │  TelegramTrigger {            │  │
│  │    db: Database       │    │    current_workspace_id     │  │
│  │    messages: Vec       │    │    current_task_id          │  │
│  │    job_manager: {...}  │    │  }                          │  │
│  │    workspaces: [...]   │    │                              │  │
│  │  }                     │    │  每个函数内部各自打开          │  │
│  │                        │    │  sqlez::Connection          │  │
│  │  OrchestratorBridge    │    │  （非 AppState.db.conn）    │  │
│  │  (GPUUI Executor 调度)  │    │                              │  │
│  └────────┬──────────────┘    └──────────────┬───────────────┘  │
│            │                                  │                  │
│            │        共享同一个 SQLite 文件     │                  │
│            │        ~/.one/one.db            │                  │
│            │   messages / tasks / workspaces  │                  │
│            └──────────────────────────────────┘                  │
└──────────────────────────────────────────────────────────────────────┘
```

---

## 二、全部确认的 Bug（必须先修）

### B1: `AwaitingUserInput` 不落 DB

**文件**：`src/runtime/job_manager.rs:598-611`

```rust
OrchestratorEvent::AwaitingUserInput { reply } => {
    this.messages.push(ChatMessage::new("assistant", &reply));
    // ❌ 缺少：task_db::insert_message()
    this.job_manager.request_in_flight = false;
    // ...
    cx.notify();
}
```

**影响**：Orchestrator 多轮交互中，用户回复后 AI 的 `AwaitingUserInput` 回复只写在内存。切换 task 时这条消息丢失。

---

### B2: `Finished` handler 用 `this.active_task_id` 而非 captured 值（竞态）

**文件**：`src/runtime/job_manager.rs:515`（capture） 和 `656`（使用）

```rust
// line 515: spawn 时 capture
let active_task_id = self.active_task_id;

// line 656: ❌ 用 this.active_task_id（当前值，用户可能已切换 task）
if let Some(task_id) = this.active_task_id {
    task_db::insert_message(&this.db.conn, task_id, "assistant", &result).ok();
}
```

**应改为**：
```rust
if let Some(task_id) = active_task_id {  // 用 captured 值
    task_db::insert_message(&this.db.conn, task_id, "assistant", &result).ok();
}
```

**影响**：Orchestrator 运行中切换 task，AI 最终回复写到**当前 task** 的 DB，而非原来启动的那个 task。

**对比参考**：同一文件中 `apply_general_ai_stream_event` 的 `GeneralAiStreamEvent::Finished` 用 `run_task_id`（等于 `self.job_manager.general_ai_task_id`，在 spawn 时设置）写 DB，是正确的模式。B2 的修复应参照这个模式。

---

### B3: `Failed` handler 不落 DB

**文件**：`src/runtime/job_manager.rs:671-692`

```rust
OrchestratorWrapperEvent::Failed(error) => {
    // ...
    this.messages.push(ChatMessage::new("assistant", &format!("Orchestrator failed: {}", error)));
    this.needs_auto_scroll = true;
    // ❌ 缺少：task_db::insert_message()
}
```

**影响**：Orchestrator 出错时，错误消息不写 DB，切换 task 后丢失。

**注意**：`Failed` handler 中 `this.mark_task_inactive(active_task_id)` 使用了 captured 的 `active_task_id`（✅ 正确），不存在 B2 类型的 task_id 错误。但因为没有写 DB，所以 B3 本身仍需修复。

---

### B4: `AwaitingUserInput` 后用户回复写 DB 用 `this.active_task_id`（B2 变体）

**文件**：`src/routing.rs:10-13`

```rust
pub(crate) fn route_message(&mut self, message: String, cx: &mut Context<Self>) {
    self.messages.push(ChatMessage::new("user", &message));
    if let Some(task_id) = self.active_task_id {  // ❌ 当前值，用户可能已切换 task
        task_db::insert_message(&self.db.conn, task_id, "user", &message).ok();
    }
```

**场景**：
1. Orchestrator 在 task A 上运行，触发 `AwaitingUserInput`
2. 用户切换到 task B
3. 用户在 task B 发消息
4. `route_message` 检测 `orchestrator_user_input_tx` → 通过 channel 发送给旧 orchestrator
5. 用户消息写入 task B 的 DB（`this.active_task_id` = B），而非 task A
6. 旧 orchestrator 在 task A 的 session 里收到这条消息，但 DB 里它属于 task B

**影响**：DB 中 task A 缺少用户回复，task B 多了一条不属于它的消息。这是 B2 的路由层变体。

---

### B5: `Finished` handler 中记忆快照使用 `this.messages.clone()` 而非 DB 加载

**文件**：`src/runtime/job_manager.rs:625`

```rust
OrchestratorWrapperEvent::Finished(result) => {
    if let Some(task_id) = active_task_id {
        let messages = this.messages.clone();  // ❌ 内存中的当前值
        // ...
        std::thread::spawn(move || {
            crate::memory::snapshot::generate_snapshot_sync(
                &base_url, &api_key, &model, &messages, task_id, &task_title, &ws_name,
            );
        });
    }
```

**场景**：Orchestrator 在 task A 上运行，Finished handler 被调用时：
- 如果用户已切换到 task B → `this.messages` 是 task B 的消息 → 快照用**错误的消息**生成
- 即使修复 B2（用 `active_task_id` 写 DB），快照仍然用了错误的 `this.messages`

**应改为**：从 DB 加载消息：
```rust
let messages = task_db::load_messages(&this.db.conn, task_id)
    .unwrap_or_default()
    .into_iter()
    .map(|m| ChatMessage::new(&m.role, &m.content))
    .collect::<Vec<_>>();
```

---

### B6: `save_model_config` / `toggle_lang` / `toggle_theme` 清空 Telegram 配置（严重）

**文件**：`src/app_state.rs:613-625`、`643-657`、`664-687`

```rust
// save_model_config (line 613-625)
let config = Config {
    model_base_url: self.model_base_url.clone(),
    model_api_key: self.model_api_key.clone(),
    model_name: self.model_name.clone(),
    light_model: None,
    coding_model: None,
    system_model: None,
    lang: self.current_lang,
    theme_mode: self.theme_mode,
    telegram_bot_token: None,   // ❌ 清空了已绑定的 Telegram 配置
    telegram_chat_id: None,     // ❌
    telegram_bound_at: None,    // ❌
};
if let Err(e) = save_config(&config) { ... }
```

同样的模式出现在 `toggle_lang` 和 `toggle_theme` 中。

**影响**：每次保存 model config、切换语言、切换主题时，都会创建一个**不含 Telegram 字段**的 Config 对象并写入 `~/.one/config.json`。这会**覆盖掉之前成功绑定的 Telegram token 和 chat_id**。下次应用重启后，`TelegramTrigger::from_config()` 读取 config 发现 token 为 None → trigger 不启动 → Telegram 控制功能彻底失效。

**修复**：所有 `save_config` 调用应该先 `load_config()` 再只修改需要改的字段：
```rust
let mut config = crate::services::load_config();
config.model_name = self.model_name.clone();
config.model_base_url = self.model_base_url.clone();
config.model_api_key = self.model_api_key.clone();
save_config(&config)?;
```

---

### B7: Telegram 绑定轮询与 trigger 实例竞态（严重）

**文件**：`src/app_state.rs:802-977`（`start_telegram_bind`）和 `src/main.rs:196`

**问题流程**：
1. 应用启动 → `main.rs:196` 调用 `TelegramTrigger::spawn_in_background(&config)` → trigger 实例 A 开始长轮询（前提是已配置过 token，首次启动时 config 中 token 为 None 不会创建任何实例）
2. 用户在 GPUI 中点击绑定 Telegram → `start_telegram_bind` 自己做了 `getUpdates` 轮询（line 892-907）来寻找绑定码消息 → 消费了 update offset
3. 绑定成功后 → `spawn_in_background(&config)` 启动 trigger 实例 B（line 957）
4. 实例 B 从 offset=0 开始，会重新拉取已消费的更新，或者如果实例 A 仍在运行，两者竞争同一个 Bot 的 getUpdates

**影响**：
- 如果实例 A 仍在运行（之前绑定/解绑再绑定的情况） → 两个实例同时轮询同一个 Bot → 消息可能被 A 或 B 任意消费 → 回复可能丢失或重复
- 绑定轮询吃掉了 offset → 实例 B 可能错过绑定确认消息之后的更新

**修复**：
1. 新增全局 `TRIGGER_STOP_SIGNAL: AtomicBool` + `TRIGGER_RUNNING_COUNT: AtomicUsize`
2. `TelegramTrigger::stop_all()` 设置停止信号后**立即返回，不阻塞等待**。旧实例会在完成当前 `getUpdates` poll 后检测到信号并自行退出。
3. `run()` 循环每次 poll 前检查 `TRIGGER_STOP_SIGNAL`
4. `spawn_in_background()` / `start_telegram_bind()` / `handle_telegram_unbind()` 在适当时机调用 `stop_all()`

**设计决策说明**：`stop_all()` 不等待旧实例退出的原因：
- 旧实例可能在 `getUpdates` 长轮询中阻塞（timeout=50s），等待它会卡住 UI 几十秒
- 中国用户访问 Telegram 网络不佳时，阻塞时间可能更长，UI 卡住问题更严重
- 不等待的做法：旧实例下次 poll 回到循环顶部时自行退出（最长 ~55 秒），新实例立即工作
- 新旧实例短暂共存期间，getUpdates offset 机制保证不会重复消费消息
- 正常使用场景（绑定一次后不反复绑定），此问题几乎不会触发

---

### B8: `/workspace` 切换没清空 `current_task_id`（当前已存在的 bug）

**文件**：`src/triggers/telegram.rs:421-468`

```rust
if text.starts_with("/workspace ") {
    let name = text.trim_start_matches("/workspace ").trim();
    if !name.is_empty() {
        // ... 匹配 workspace → 更新 current_workspace_id
        *self.current_workspace_id.lock().unwrap() = ws.id.to_string();
        // ❌ 但没有清空 current_task_id！
    }
    continue;
}
```

**影响**：切换 workspace 后 `current_task_id` 没有被清空，新 workspace 下会继续使用旧 workspace 的 task_id。如果两个 workspace 的 task ID 碰巧相同，消息会混入错误的 task；如果 ID 不同，后续 `ensure_remote_task()` 会因为发现已有 `current_task_id` 而不创建新 task，直接使用旧 task。

**修复**：
```rust
*self.current_workspace_id.lock().unwrap() = ws.id.to_string();
*self.current_task_id.lock().unwrap() = None;  // 清空旧 task
```

---

## 三、Telegram 侧 Dead Code

| 函数 | 位置 | 调用次数 | 问题 |
|---|---|---|---|
| `ensure_remote_task()` | `telegram.rs:67` | **0** | 定义了但从未调用 |
| `append_step_to_task()` | `telegram.rs:103` | **0** | 定义了但从未调用 |

**当前 `Chat(t)` 分支行为**（`dispatcher.rs:118-148`）：
```rust
TriggerCommand::Chat(t) => {
    call_chat_api_stream(..., |delta| { full_text.push_str(&delta); })  // 简单 API
    // ❌ 没建 task
    // ❌ 没写 DB
    // ❌ 没走 Orchestrator
    // ❌ 没有记忆系统支持（L1 profile / L3 context）
}
```

**深层问题**：`Chat(t)` 只传一条 user 消息给 `call_chat_api_stream`，不注入任何记忆上下文。而 Orchestrator 路径会注入 L1 profile facts 和 L3 cross-task context。这意味着 Telegram 用户通过 Chat 分支获得的回复质量远低于 GPUI 侧的 Orchestrator 路径。

---

## 四、Orchestrator 生命周期（关键事实）

**文件**：`orchestrator.rs:43-167`

```
orchestrator.run_task(instruction, history, ...)
    │
    ├─ AgentContext.history = history（spawn 时传入）
    ├─ context.add_message(user, instruction)  ← 初始 user 消息
    │
    └─ while max_steps > 0 {
          step_stream() → response
          ├─ Answer + 有 user_input_rx:
          │    → on_event(AwaitingUserInput)  ← 阻塞等输入
          │    → input_rx.recv() → user_msg
          │    → context.add_message(user, user_msg)  ← ⚠️ 加到 orchestrator 内部 context，不在 AppState.messages
          │    → continue
          ├─ Answer + 无 user_input_rx:
          │    → on_event(StepFinished) → return
          └─ ToolCalls:
               → execute → continue
        }
```

**关键**：
1. `history` 是 spawn 时从 `self.messages.clone()` capture 的**静态快照**
2. orchestrator 内部的 `context.history` 在 `AwaitingUserInput` 时会 add 新消息，但 **AppState.messages 不会同步更新**（除了 push 一条 assistant 消息）
3. `AwaitingUserInput` 回复**没有写 DB**（B1 bug）
4. `general_ai_task_id`（在 spawn 时设置）是一个正确的 captured 值模式，可用于比对 B2 的修复

---

## 五、并发场景分析：GPUUI 切换 task + orchestrator 继续运行

```
T0: GPUUI spawn_orchestrator_run(history=[A])
T1: Orchestrator 执行中，触发 AwaitingUserInput
T2: on_event(AwaitingUserInput) → AppState.messages.push(reply_A)（没写DB）
T3: 用户切换 task → restore_task_context()
     → AppState.messages = load_messages(new_task_id)  ← reply_A 不在 DB，丢失
     → job_manager 状态清空
T4: 用户在新 task 发消息 B
     → route_message 检测 orchestrator_user_input_tx → 通过 channel 发送
     → 但 channel 属于旧 orchestrator（旧 task 的 session）
     → 旧 orchestrator 收到 → context.add_message(user, B)
     → 旧 orchestrator 继续执行
     → 旧 orchestrator 的 Finished 写 DB 时用 this.active_task_id → new_task_id（当前值）❌
     → AI 回复错误地写到 new_task 的 DB
```

**这是 B2 bug 的实际触发路径**。

**B4 的触发路径**（同场景）：
```
T4: route_message 写用户消息到 DB 时用 this.active_task_id → new_task_id ❌
    → task A 的 DB 缺少用户回复 B
    → task B 的 DB 多了一条不属于它的消息
```

---

## 六、Telegram 消息流的正确设计

### 6.1 每次 Telegram 消息 = 独立 Orchestrator Session

**原因**：Orchestrator `run_task` 是单 instruction 生命周期，`Finished` 后 session 结束，无法跨消息延续。

**设计**：
```
Telegram 消息A
  → load_messages(task_id) → history = DB中所有历史
  → insert_message_step(user, A)
  → spawn orchestrator.run_task(history=[...], instruction=A)
  → Finished → insert_message_step(assistant, result)
  → send_message(result)

Telegram 消息B（新的独立 session）
  → load_messages(task_id) → history = [A, reply_A, B]
  → insert_message_step(user, B)
  → spawn orchestrator.run_task(history=[...], instruction=B)
  → ...
```

### 6.2 简化方案：Telegram 线程内直接跑 orchestrator

不需要跨线程调度 GPUUI Executor。Telegram 在自身线程直接创建 orchestrator 并 await：

```rust
// telegram.rs - Chat 分支伪代码
TriggerCommand::Chat(t) => {
    // 1. 检查 workspace
    let ws_id = current_workspace_id.lock().unwrap().clone();
    if ws_id.is_empty() {
        send_message("请先发送 /workspace <name>")?;
        return;
    }

    // 2. 获取或创建 remote task
    let task_id = ensure_remote_task(&current_workspace_id, &current_task_id)?;
    let task_id_usize: usize = task_id.parse()?;

    // 3. 从 DB 加载历史
    let conn = open_conn();
    let db_messages = task_db::load_messages(&conn, task_id_usize)?;
    let history = db_messages.into_iter()
        .map(|m| ChatMessage::new(&m.role, &m.content))
        .collect::<Vec<_>>();

    // 4. 写用户消息到 DB
    append_step_to_task(&task_id, "user", &t, "user_message", None);

    // 5. 在 Telegram 线程跑 orchestrator
    let config = crate::services::load_config();
    let workspace_name = get_workspace_name(&ws_id)?;
    let workspace_root = get_workspace_root(&ws_id)?;
    let orchestrator = AgentFactory::create_orchestrator(&config, &workspace_name, workspace_root)?;
    let session_id = format!("telegram-task-{}", task_id);

    // 注意：user_input_rx=None → 不支持多轮追问，AI 回复需要追问时直接返回
    let result = orchestrator.run_task(
        &t, session_id, history, &workspace_name, Some(task_id_usize),
        None, None,  // cancel_flag=None（无法取消），user_input_rx=None
        |_| {},  // on_event callback（不关心中间事件）
    ).await?;

    // 6. 写 AI 回复到 DB
    append_step_to_task(&task_id, "assistant", &result, "ai_reply", None);

    // 7. 回送
    send_message(&result)?;
}
```

**设计限制**：`user_input_rx=None` 意味着 Telegram 的 Orchestrator session 不支持多轮交互。如果 AI 回复包含追问（`AwaitingUserInput`），因为没有 input channel，Orchestrator 会直接 `StepFinished` 返回。这是合理的简化，但需要在帮助文档中说明。

### 6.3 DB 连接问题

**发现**：`telegram.rs` 中 `ensure_remote_task` 和 `append_step_to_task` 各自打开独立的 `sqlez::Connection`，而 GPUUI 用的是 `AppState.db.conn`。

**SQLite 行为**：多连接并发读写 SQLite 会通过文件锁序列化，`BEGIN TRANSACTION`/`COMMIT` 保证原子性。GPUUI 和 Telegram 各自用独立连接不会破坏数据，但可能有时序问题（一个连接写的变更，另一个连接可能不会立即读到，取决于隔离级别）。

**建议**：保持各自独立连接（改动最小），依靠 SQLite 的并发控制。

**注意**：`sqlez` 是 Zed 的 SQLite 绑定层，与 `rusqlite` 行为可能有差异。应验证 `sqlez` 在多连接场景下 WAL 模式是否默认启用。如果未启用 WAL，长写操作会阻塞其他连接的读操作。

### 6.4 `append_step_to_task` 是 Dead Code 未被验证

伪代码依赖 `append_step_to_task()` 和 `ensure_remote_task()`，但这两个函数目前从未被任何路径调用过，也没有被测试验证过。在修复计划中应先验证这两个函数的正确性，特别是：
- `insert_message_step` 的 `step_index` 参数是否正确（当前用 `count_messages` + 1）
- `insert_remote_task` 是否正确创建 task 并返回 ID

---

## 七、Telegram 消息并发与 cipher 状态机冲突

**文件**：`src/triggers/telegram.rs:401-416`

```rust
// telegram.rs run() 循环中
if self.pending.lock().unwrap().contains_key(&chat_id) {
    match self.handle_cipher_reply(chat_id, &text).await { ... }
    continue;
}
```

**问题**：如果同一个 chat_id 在短时间内发送两条消息，Telegram long-poll 可能在一个 `getUpdates` 响应中返回两个 update。当前代码逐条处理，但如果第一条消息触发了 cipher 状态机（`/run` dangerous skill），第二条消息（可能是普通聊天文本）会被误判为暗号回复。

**影响**：用户在等待暗号确认期间发送的任何消息都会被当作暗号验证尝试，导致暗号验证失败或误执行危险操作。

**修复思路**：
1. 在 cipher 状态机激活时，对每条消息先检查是否看起来像暗号（长度短、无特殊字符等），不像的则告知用户"正在等待暗号确认，请回复暗号或发送 /cancel 取消"
2. 或者在触发 cipher 时立即回复用户，让用户知道后续消息只接受暗号

---

## 八、完整执行计划

```
阶段 1：Bug Fix（P0 — 必须先修）
  [1] B1: job_manager.rs ~598：AwaitingUserInput 落 DB
  [2] B2: job_manager.rs ~656：Finished handler 改用 captured active_task_id
  [3] B3: job_manager.rs ~691：Failed handler 落 DB（用 captured active_task_id）
  [4] B4: routing.rs ~12：route_message 写 DB 时改用 captured task_id（需要调整架构，在 route_message 入口处 capture）
  [5] B5: job_manager.rs ~625：Finished handler 记忆快照改用 DB load_messages 而非 this.messages.clone()
  [6] B6: app_state.rs：save_model_config/toggle_lang/toggle_theme 先 load_config() 再修改字段，保留 telegram 配置
  [7] B8: telegram.rs ~444：/workspace 切换后清空 current_task_id

阶段 2：Telegram Orchestrator 集成
  [8] telegram.rs：Chat 分支改造（load_history + orchestrator.run_task + 写 DB）
  [9] telegram.rs：先验证 ensure_remote_task / append_step_to_task 的正确性
  [10] dispatcher.rs：Chat 分支增加 L1/L3 记忆注入（或通过 orchestrator 自动注入）

阶段 3：竞态与并发修复
  [11] B7: app_state.rs：Telegram 绑定轮询与 trigger 实例竞态修复
      - 绑定前先通知旧 trigger 停止（需要全局停止信号）
      - 绑定轮询结束后传递 offset 给新 trigger
  [12] telegram.rs：cipher 状态机并发消息处理（过滤非暗号消息）

阶段 4：GPUUI 刷新（无需改动）
  [13] restore_task_context 已正确从 DB 加载
```

**优先级排序**：
- **P0**：B6（最严重，每次保存都会覆盖 Telegram 配置）、B1/B2/B3（数据丢失）
- **P1**：B7（消息丢失/重复）、B4/B5（数据错乱）、B8（逻辑错误）
- **P2**：Telegram Orchestrator 集成、cipher 并发处理

---

## 九、关键设计决策

| # | 决策 | 理由 |
|---|---|---|
| D1 | Telegram 每次消息是**独立 Orchestrator session** | Orchestrator.run_task() 生命周期只支持单 instruction，session 在 Finished 后结束 |
| D2 | Telegram **不跨线程调度** GPUUI Executor | 在 Telegram 自身线程 await orchestrator.run_task()，通过 DB 共享历史 |
| D3 | 并发控制用"拒绝"策略 | request_in_flight=true 时另一端提示等待；AwaitingUserInput 时 request_in_flight=false，所以 GPUUI 可以往 orchestrator_user_input_tx 发消息 |
| D4 | B1/B3 修复后，GPUUI 的 AwaitingUserInput 多轮消息会正确持久化 | 写 DB 后切换 task 时能从 DB 正确加载 |
| D5 | Telegram 消息用 `insert_message_step`，GPUUI 消息用 `insert_message` | 两者写入同一个 messages 表，load_messages 按 created_at 排序可合并 |
| D6 | 各自独立 SQLite 连接 | 保持最小改动；SQLite 文件锁保证并发安全（需验证 WAL 模式） |
| D7 | `save_config` 必须先 `load_config` 再修改 | 防止覆盖 Telegram 配置（B6） |
| D8 | 记忆快照从 DB 加载而非内存 | 防止切换 task 后快照用错误的消息（B5） |

---

## 十、已知限制

| 限制 | 说明 |
|---|---|
| **L1** | Telegram 每次消息是独立 session，max_steps=15 会快速消耗。Orchestrator 历史靠 DB 加载，但中间 AwaitingUserInput 时的 orchestrator 内部 context（`context.history`）和 GPUUI 的 `self.messages` 短暂不一致 |
| **L2** | GPUUI 正在流式输出（Delta）时切换 task，实时内容消失，不会出现在历史中（因为没有写 DB） |
| **L3** | GPUUI AwaitingUserInput 后切换 task，旧 orchestrator 继续运行但 GPUUI 已不在监听，Finished/Bug 写到错误的 task（B2 bug 的实际表现） |
| **L4** | Telegram 看不到 GPUUI 正在流式输出的实时内容，只能在 task 切换后看到最终结果 |
| **L5** | Telegram Chat 分支无多轮追问能力（user_input_rx=None），AI 需要追问时直接返回 |
| **L6** | Telegram Chat 分支无记忆系统支持（L1 profile / L3 context），回复质量低于 GPUUI 侧 Orchestrator 路径 |
| **L7** | `ensure_remote_task` / `append_step_to_task` 是 dead code，未经过实际调用验证 |

---

## 十一、Bug 影响矩阵

| Bug | GPUUI 侧影响 | Telegram 侧影响 | 修复难度 |
|---|---|---|---|
| **B1** | 切换 task 后 AwaitingUserInput 回复丢失 | 不涉及（Telegram Chat 不走 Orchestrator） | 低（加一行 insert_message） |
| **B2** | AI 回复写到错误 task 的 DB | 不涉及 | 低（改用 captured 值） |
| **B3** | Orchestrator 错误消息不落 DB | 不涉及 | 低（加一行 insert_message） |
| **B4** | 用户回复写到错误 task 的 DB | 不涉及 | 中（route_message 需要捕获 task_id） |
| **B5** | 记忆快照用错误的消息生成 | 不涉及 | 中（改用 DB 加载） |
| **B6** | **Telegram 配置被每次保存覆盖** | **Telegram 功能彻底失效** | 低（先 load 再改） |
| **B7** | 不涉及 | **消息丢失/重复/两个实例竞争** | 高（需要全局停止信号） |
| **B8** | 不涉及 | task 消息混入错误 workspace | 低（加一行清空） |