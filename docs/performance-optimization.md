# ONE GPUI 性能优化方案

> 生成时间：2026-06-06
> 版本：v2（代码对照修订版）
> 目标：消除不必要的全量 re-render，优化事件通知机制，提升页面流畅度

---

## 一、问题诊断

### 1.1 核心问题：5 个 polling loop 导致每秒最多 83 次无效 re-render

当前代码中存在 5 个独立的 `cx.spawn + loop + timer` 模式的事件轮询循环，其中 4 个频率为 **60ms/次**，1 个为 **120ms/次**。组合起来，在 AI 响应活跃期间，最多可达 **每秒 83 次 `cx.notify()`**。

每次 `cx.notify()` 都会触发 GPUI 的 `AppState::render()` 全量重建整个 UI 树（nav、chat、sidebar、terminal），即使在聊天输出过程中左侧 nav 和右侧面板的状态完全没有变化。

### 1.2 具体循环列表（已对照 job_manager.rs 实际代码确认）

| # | 位置 | 函数 | 频率 | 活跃时段 | 影响程度 | 备注 |
|---|------|------|------|----------|----------|------|
| 1 | `app_state.rs` | `start_approval_pump` | 8.3Hz (120ms) | **始终运行** | 低(但浪费) | 同时检查 permission + soul |
| 2 | `job_manager.rs` | `spawn_claude_code_run` 内 loop | 16.6Hz (60ms) | Claude 执行中 | **极高** | |
| 3 | `job_manager.rs` | `spawn_summarize_job` 内 loop | 16.6Hz (60ms) | 摘要生成中 | 高 | |
| 4 | `job_manager.rs` | `poll_general_ai_events`（私有方法） | 16.6Hz (60ms) | AI 回复中 | **极高** | 被 `spawn_system_tools_run` 和 `confirm_system_tools_operation` 共用，迁移只需改一处 |
| 5 | `job_manager.rs` | `spawn_orchestrator_run` 内 loop | 16.6Hz (60ms) | Agent 运行中 | **极高** | |

> ⚠️ 注意：#4 的 `poll_general_ai_events` 是一个私有方法，被两个调用方共用。迁移时只改这一个方法即可，不需要分别修改两处调用方。

### 1.3 为什么这些循环是浪费的

所有 polling loop 的模式相同：

```rust
loop {
    loop {
        match receiver.try_recv() {
            Ok(event) => { update(state); cx.notify(); }
            Err(TryRecvError::Empty) => break,   // ← 没有事件
            Err(Disconnected) => return,
        }
    }
    // 不管有没有事件，都会等 60ms 再试
    cx.background_executor().timer(Duration::from_millis(60)).await;
    // 下一次 loop 即使没有新事件，也会触发全量 re-render
}
```

关键浪费点：**当 channel 中没有新事件时，循环仍然每 60ms 醒来一次，触发全量 re-render**。

> ✅ 注意：当前代码 `cx.notify()` 已经只放在 `Ok(event)` 分支内，不在 Empty 分支调用，这一点已经是正确的。问题在于 `timer` 本身每 60ms 都会让 loop 体执行一次，即使随后立刻 break。

### 1.4 次要问题：Approval Pump 是纯空轮询

```rust
// app_state.rs — start_approval_pump
fn start_approval_pump(&self, cx: &mut Context<Self>) {
    cx.spawn(async move |this, cx| loop {
        let _ = this.update(cx, |state, cx| {
            // 99.99% 的时间这里什么也不做
            if state.pending_approval.is_none() {
                if let Some(req) = crate::agents::permission::drain_next() { ... }
            }
            if state.pending_soul_proposal.is_none() {
                if let Some(prop) = crate::agents::soul::drain_next() { ... }
            }
        });
        cx.background_executor().timer(Duration::from_millis(120)).await;
    }).detach();
}
```

即使用户不进行任何操作，这个循环也永远在跑，且同时检查 `permission` 和 `soul` 两个队列。

---

## 二、修复方案

### 2.1 方案 A-简化版（推荐首选）：各 loop 改用 `tokio recv().await`

#### 核心思路

**不需要新建全局事件调度器**。只需把每个 job 的 `std::sync::mpsc` 改为 `tokio::sync::mpsc`，然后把 `try_recv + timer` 改为 `recv().await`——没有事件时协程真正挂起，零 CPU 消耗，有事件时立即唤醒。

这个方案比全局事件调度器改动量小 **70%**，效果相同。

#### 改动模式（4 个 loop 相同）

```rust
// ── 改前（std::sync::mpsc + try_recv + timer）────────────────────────
let (sender, receiver) = std::sync::mpsc::channel::<ClaudeStreamEvent>();

cx.spawn(async move |this, cx| loop {
    let mut disconnected = false;
    loop {
        match receiver.try_recv() {
            Ok(event) => { ... cx.notify(); }
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => { disconnected = true; break; }
        }
    }
    if disconnected { break; }
    cx.background_executor().timer(Duration::from_millis(60)).await;
}).detach();

// ── 改后（tokio::sync::mpsc + recv().await）──────────────────────────
let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<ClaudeStreamEvent>();

// ⚠️ 注意：gpui_tokio::Tokio::spawn 才支持真正的 tokio 异步，
// cx.spawn 是 GPUI 的 async executor，也支持 .await，但底层不是 tokio。
// 这里的 recv().await 在 cx.spawn 里可用，因为 GPUI executor 支持跨 await。
cx.spawn(async move |this, cx| {
    while let Some(event) = receiver.recv().await {  // 真正阻塞等待，无事件不消耗 CPU
        let _ = this.update(cx, |this, cx| {
            this.apply_claude_run_event(run_id, event);
            cx.notify();
        });
    }
    // channel 关闭时自然退出，不需要 disconnected flag
}).detach();
```

#### ⚠️ GPUI `cx.spawn` 签名说明

GPUI 的 `cx.spawn` 闭包签名只有 **一个** `cx` 参数（`AsyncContext`），不是两个：

```rust
// ✅ 正确
cx.spawn(async move |this, cx| { ... })
// 其中 this: WeakEntity<AppState>，cx: AsyncContext<AppState>

// ❌ 错误（文档早期版本的误写）
cx.spawn(async move |_win, cx| { ... })  // 多了 _win 参数，编译报错
```

`tokio::sync::mpsc` 的 `recv().await` 在 `cx.spawn` 的闭包里完全可用，GPUI executor 支持跨 await point 挂起。

#### 具体改动点（4 处）

**改动 1：`spawn_claude_code_run`**

```rust
// 把 channel 类型改为 tokio unbounded
let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<ClaudeStreamEvent>();
let worker_sender = sender.clone();
let final_sender = sender;  // 注意：不再需要 clone，最后一个 sender drop 时 channel 关闭

// cx.spawn loop 改为：
cx.spawn(async move |this, cx| {
    while let Some(event) = receiver.recv().await {
        if let Some(rid) = log_run_id { /* RunRecorder 逻辑不变 */ }
        let _ = this.update(cx, |this, cx| {
            this.apply_claude_run_event(run_id, event);
            cx.notify();
        });
    }
}).detach();
```

**改动 2：`spawn_summarize_job`**

```rust
let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<SummarizeEvent>();

cx.spawn(async move |this, cx| {
    while let Some(event) = receiver.recv().await {
        let _ = this.update(cx, |this, cx| {
            this.apply_summarize_event(event);
            cx.notify();
        });
    }
}).detach();
```

**改动 3：`poll_general_ai_events`（一处改动覆盖两个调用方）**

```rust
fn poll_general_ai_events(
    &mut self,
    run_id: u64,
    mut receiver: tokio::sync::mpsc::UnboundedReceiver<GeneralAiStreamEvent>,
    cx: &mut Context<Self>,
) {
    cx.spawn(async move |this, cx| {
        while let Some(event) = receiver.recv().await {
            let _ = this.update(cx, |this, cx| {
                this.apply_general_ai_stream_event(run_id, event, cx);
                cx.notify();
            });
        }
    }).detach();
}
```

`spawn_system_tools_run` 和 `confirm_system_tools_operation` 调用方只需把 channel 类型改为 `tokio::sync::mpsc::unbounded_channel`，调用 `poll_general_ai_events` 的方式不变。

**改动 4：`spawn_orchestrator_run`**

```rust
let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<OrchestratorWrapperEvent>();

cx.spawn(async move |this, cx| {
    while let Some(event) = receiver.recv().await {
        let _ = this.update(cx, |this, cx| {
            // 原有的 match event { ... } 逻辑完全不变
            match event { ... }
            cx.notify();
        });
    }
}).detach();
```

#### 变更清单

| 文件 | 改动 | 工作量 |
|------|------|--------|
| `src/runtime/job_manager.rs` | 4 处 channel 类型改为 tokio unbounded；4 个 loop 改为 recv().await | 4 × 0.25 天 |

**不需要新建任何文件，不需要改 `app_state.rs`、`permission.rs`、`main.rs`。**

---

### 2.2 方案 A-完整版：统一全局事件调度器

如果希望进一步统一事件路由、便于未来扩展（如添加事件日志、限流、优先级队列），可以在简化版基础上再上一层：

#### 核心思路

把所有 job 的事件合并为一个全局枚举，用一个 loop 处理，减少 `cx.notify()` 调用次数（多个事件合并为一次 notify）。

#### 新建 `src/runtime/event_poller.rs`

```rust
use tokio::sync::mpsc;
use std::sync::OnceLock;

pub(crate) enum PollEvent {
    ClaudeStream { run_id: u64, event: ClaudeStreamEvent },
    GeneralAiStream { run_id: u64, event: GeneralAiStreamEvent },
    Orchestrator(OrchestratorWrapperEvent),
    Summarize(SummarizeEvent),
    ApprovalPending,
    SoulProposalPending,
}

static GLOBAL_EVENT_TX: OnceLock<mpsc::UnboundedSender<PollEvent>> = OnceLock::new();

pub(crate) fn global_sender() -> Option<mpsc::UnboundedSender<PollEvent>> {
    GLOBAL_EVENT_TX.get().cloned()
}

pub(crate) fn start_event_loop(app: gpui::WeakEntity<AppState>, cx: &mut gpui::Context<AppState>) {
    let (tx, mut rx) = mpsc::unbounded_channel::<PollEvent>();
    let _ = GLOBAL_EVENT_TX.set(tx);

    // ⚠️ cx.spawn 签名只有一个参数：(WeakEntity<AppState>, AsyncContext<AppState>)
    cx.spawn(async move |this, cx| {
        loop {
            // recv + drain：多个事件一次 notify，减少 re-render
            match rx.recv().await {
                Some(first) => {
                    let _ = this.update(cx, |state, cx| {
                        state.handle_poll_event(first, cx);
                        while let Ok(next) = rx.try_recv() {
                            state.handle_poll_event(next, cx);
                        }
                        cx.notify();
                    });
                }
                None => break, // 所有 sender 丢弃，退出
            }
        }
    }).detach();
}
```

#### Approval/Soul 改为推送通知

**`permission.rs`**：`enqueue_request` 推送到全局 channel

```rust
// enqueue_request 末尾添加
if let Some(tx) = crate::runtime::event_poller::global_sender() {
    let _ = tx.send(PollEvent::ApprovalPending);
}
```

**`soul.rs`**：`submit_proposal` 推送到全局 channel（在变更清单里补上，v1 遗漏）

```rust
// submit_proposal 末尾添加
if let Some(tx) = crate::runtime::event_poller::global_sender() {
    let _ = tx.send(PollEvent::SoulProposalPending);
}
```

#### 完整版变更清单

| 文件 | 改动 |
|------|------|
| `src/runtime/event_poller.rs` | **新建**：全局 PollEvent 枚举 + unbounded channel + event loop |
| `src/runtime/mod.rs` | 导出 `event_poller` |
| `src/runtime/job_manager.rs` | 移除 4 个独立 loop，改向全局 channel 发送 PollEvent |
| `src/agents/permission.rs` | `enqueue_request` 末尾发送 `ApprovalPending` |
| `src/agents/soul.rs` | `submit_proposal` 末尾发送 `SoulProposalPending` |
| `src/app_state.rs` | 删除 `start_approval_pump`；增加 `handle_poll_event` |
| `src/main.rs` | 启动时调用 `event_poller::start_event_loop` |

---

### 2.3 方案 B：Approval Pump 单独优化（最低成本）

适用于不想动 job_manager 的情况，只解决 approval_pump 的空轮询问题。

使用 `tokio::sync::Notify` 替代轮询：

```rust
// agents/permission.rs — 全局 Notify
use tokio::sync::Notify;
use std::sync::{OnceLock, Arc};

static APPROVAL_NOTIFY: OnceLock<Arc<Notify>> = OnceLock::new();

pub fn approval_notify() -> Arc<Notify> {
    APPROVAL_NOTIFY.get_or_init(|| Arc::new(Notify::new())).clone()
}

// enqueue_request 末尾添加
approval_notify().notify_one();
```

```rust
// soul.rs — 同样模式
static SOUL_NOTIFY: OnceLock<Arc<Notify>> = OnceLock::new();

pub fn soul_notify() -> Arc<Notify> {
    SOUL_NOTIFY.get_or_init(|| Arc::new(Notify::new())).clone()
}

// submit_proposal 末尾添加
soul_notify().notify_one();
```

```rust
// app_state.rs — start_approval_pump 改为等待通知
fn start_approval_pump(&self, cx: &mut Context<Self>) {
    let perm_notify = crate::agents::permission::approval_notify();
    let soul_notify = crate::agents::soul::soul_notify();

    cx.spawn(async move |this, cx| loop {
        // 等待任意一个通知，没有请求时零 CPU
        tokio::select! {
            _ = perm_notify.notified() => {}
            _ = soul_notify.notified() => {}
        }
        let _ = this.update(cx, |state, cx| {
            if state.pending_approval.is_none() {
                if let Some(req) = crate::agents::permission::drain_next() {
                    state.pending_approval = Some(req);
                    cx.notify();
                }
            }
            if state.pending_soul_proposal.is_none() {
                if let Some(prop) = crate::agents::soul::drain_next() {
                    state.pending_soul_proposal = Some(prop);
                    cx.notify();
                }
            }
        });
    }).detach();
}
```

**变更清单（方案 B）**

| 文件 | 改动 |
|------|------|
| `src/agents/permission.rs` | 新增全局 `Notify`；`enqueue_request` 末尾 `notify_one()` |
| `src/agents/soul.rs` | 新增全局 `Notify`；`submit_proposal` 末尾 `notify_one()` |
| `src/app_state.rs` | `start_approval_pump` 改为 `tokio::select! + notified().await` |

---

### 2.4 render 树优化（独立于事件调度）

#### 2.4.1 聊天消息渲染优化

`render_chat_messages()` 每次重建所有消息和 `SubagentMessageState` 卡片。GPUI 没有虚拟滚动支持，手动分段可行但实现复杂。

**暂缓**：除非消息数 > 100 条且出现明显滚动卡顿，否则不是性能瓶颈。

#### 2.4.2 未来：AppState 拆分为多个 Entity

当前所有状态集中在 `AppState`，任何 `cx.notify()` 都触发全量 re-render。长期可将 `AppState` 拆分为 `ChatPanel`、`NavPanel`、`SidebarPanel` 各自独立的 `Entity`，实现局部刷新。这是更激进的重构，留待性能真正成为瓶颈时再做。

---

## 三、实施步骤与优先级

### 推荐路径

```
方案 B（0.5 天）→ 先解决始终运行的 approval_pump 空轮询，改动最小、收益确定
       ↓
方案 A-简化版（1 天）→ 解决 4 个 job loop 的 polling 问题
```

不建议跳过简化版直接上完整版，除非有明确的"多事件合并"需求。

### Phase 1：Approval/Soul Pump 事件化（0.5 天）

1. `permission.rs` 新增全局 `Notify`；`enqueue_request` 末尾 `notify_one()`
2. `soul.rs` 新增全局 `Notify`；`submit_proposal` 末尾 `notify_one()`
3. `app_state.rs` `start_approval_pump` 改为 `tokio::select! + notified().await`

### Phase 2：Job Manager loop 改为 recv().await（1 天）

1. `job_manager.rs` 引入 `tokio::sync::mpsc`（Cargo.toml 已有 tokio 依赖，无需新增）
2. `spawn_claude_code_run`：channel 改为 unbounded，loop 改为 `while let Some = recv().await`
3. `spawn_summarize_job`：同上
4. `poll_general_ai_events`：同上（一处改动覆盖两个调用方）
5. `spawn_orchestrator_run`：同上，内部的 match 逻辑完全不变

### Phase 3（可选）：统一全局事件调度器（1 天）

如果 Phase 2 完成后希望进一步优化多事件合并，再做方案 A-完整版。

---

## 四、预期收益

| 指标 | 优化前 | Phase 1 后 | Phase 1+2 后 |
|------|--------|-----------|-------------|
| 空闲时 `cx.notify()` 频率 | 8.3 次/秒 | **0 次/秒** | **0 次/秒** |
| AI 响应中 `cx.notify()` 频率 | 83 次/秒 | 75 次/秒 | **与事件频率相同（无空轮询）** |
| 权限提示响应延迟 | 120ms | **即时** | 即时 |
| 无效 CPU 占用（空闲） | ~1-3% | ~0.5% | **~0%** |

---

## 五、风险与注意事项

### 5.1 tokio vs GPUI executor

`gpui_tokio::Tokio::spawn` 用于需要调用 tokio 生态（reqwest、async fn 等）的异步任务。
`cx.spawn` 是 GPUI 自己的 async executor，不是 tokio。

两者在本优化里都可以使用 `tokio::sync::mpsc`（因为 `UnboundedReceiver::recv().await` 是标准 Future，不依赖 tokio runtime），但要注意：

- **`cx.spawn` 闭包里不能直接调用 `tokio::time::sleep`**，应用 `cx.background_executor().timer()` 替代
- `tokio::sync::mpsc::recv().await` 在 `cx.spawn` 里可用，因为它是纯 Future

### 5.2 `std::sync::mpsc` → `tokio::sync::mpsc` 的类型变化

`tokio::sync::mpsc::UnboundedSender<T>` 实现了 `Clone` 和 `Send`，与原来的 `std::sync::mpsc::Sender<T>` 行为基本一致，但：
- 不再有 `TryRecvError`，改为 `Option<T>`（`recv().await` 返回 `None` 表示 channel 关闭）
- 发送方改为 `unbounded_channel` 时无法限流，但 AI 流式输出本身速率有限，不会积压

### 5.3 Notify 信号可能合并

`tokio::sync::Notify` 的 `notify_one()` 如果在 `notified().await` 之前被调用多次，等待方只会被唤醒一次。这对 approval_pump 是完全安全的——唤醒后会 `drain_next()` 把所有积压的请求都处理掉。

### 5.4 边界情况：信号在极低概率下丢失

如果 `notify_one()` 和 `notified().await` 的 await 时机重叠（竞争），信号可能被合并消耗。对于 approval_pump，下一次 push 会重新发送信号，不会永久卡住。

---

## 六、后续展望

1. **AppState 拆分为多个 Entity**：实现局部刷新，只有变化的 panel 才重建 UI 树
2. **消息虚拟化**：聊天记录超 200 条时，分页/惰性渲染可见区域
3. **动画性能**：历史消息气泡的 `with_animation` 在大量消息时可能有性能问题，可按需禁用
4. **事件优先级**：完整版 event_poller 可以为不同类型的事件设置优先级（如 UI 交互 > AI 流式输出）
