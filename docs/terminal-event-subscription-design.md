# 终端事件订阅机制设计方案

## 背景

当前 ONE 的右侧终端已经区分 `Shell` 与 `Coding` tab：

- `Shell` tab：执行普通 `run_in_terminal` 命令，例如 `ls`、`git status`、脚本运行。
- `Coding` tab：运行持久交互式编码 CLI，例如 Claude Code、Codex、Gemini。

用户期望进一步优化为事件订阅模型：终端输出、命令完成、Claude Code 需要确认、编码任务完成等状态，能够主动通知 MainAgent，而不是 MainAgent 或 UI 周期性扫描终端内容。

目标不是让 MainAgent 直接读取所有终端输出，而是让底层 runtime 把输出变化转成结构化事件，再由 MainAgent 订阅高价值事件。

## 当前实现状态

本方案已完成第一轮落地：

- 已新增 runtime event bus。
- `TerminalEmulator` 已将 alacritty `Wakeup/Exit/Title` 转成 `TerminalOutputChanged/TerminalExited/TerminalTitleChanged`。
- `run_in_terminal` 已发布 Shell 命令 started/finished/failed 生命周期事件。
- Coding runtime 已使用 session_id 作为 terminal id，并将 raw output changed 转成 `CodingOutputChanged`。
- AppState 已启动常驻订阅 loop，监听 `CodingOutputChanged` 后做防抖，再触发 `CodingSessionSupervisor`。
- UI 刷新循环不再高频全量调用旧的 `collect_supervision_requests()`；该旧入口已删除。

仍保留的后续优化：

- Shell 命令 stdout/stderr 当前仍是完成后一次性写入，尚未做 chunk streaming。
- 主聊天区对 Shell 命令完成结果的自动摘要还未接入 `AgentNotificationEvent`。
- 仍保留低频/被动 `refresh_all()` 作为 session exit/status 兜底。

## 当前实现梳理

### 1. TerminalEmulator 层

位置：

- `src/terminal_emulator/mod.rs`

当前基于 `alacritty_terminal` 创建真实 PTY：

- `TerminalEmulator::new/new_with_args` 创建 PTY 和 alacritty event loop。
- `TerminalListener` 实现 `EventListener`。
- 当前只把两类 alacritty 事件转成内部事件：
  - `Event::Exit` -> `TerminalEvent::Exited`
  - `Event::Title` -> `TerminalEvent::Title`
- `TerminalEmulator::process_events()` 通过 `try_recv()` 拉取这些事件，更新：
  - `exited`
  - `title`

关键点：

- 当前没有把 PTY 输出变化、屏幕刷新、scrollback 变化作为事件暴露。
- UI 和 runtime 需要调用 `screen_text_lines()` / `renderable_history_lines()` 主动读取终端屏幕。

### 2. Shell 命令链路

位置：

- `src/agents/core/tool_dispatcher.rs`
- `src/runtime/job_manager.rs`
- `src/ui/terminal.rs`

当前 `run_in_terminal` 流程：

1. MainAgent 调用 `run_in_terminal`。
2. `ToolDispatcher` 发出 `OrchestratorEvent::RunInTerminal { command, work_dir }`。
3. `JobManager` 接收事件后：
   - 切到 `Shell` tab。
   - 将命令记录到 `terminal_output`。
   - 通过 `Backend::Pty` 或 `Backend::Docker` 后台执行 `sh -c`。
   - 等命令结束后一次性拿到完整输出。
   - 更新最后一条 `TerminalLine.output`。

关键点：

- 普通命令不是实时 PTY session 输出流，而是后台 exec 完成后一次性更新。
- 当前没有明确的 `ShellCommandStarted/Output/Finished/Failed` 事件流。
- MainAgent 得到的工具返回只是“命令已发送到 Shell tab 执行”，不会自动获得结构化命令结果总结。

### 3. Coding runtime 链路

位置：

- `src/runtime/persistent_session.rs`
- `src/runtime/coding_sessions.rs`
- `src/runtime/coding_supervisor.rs`
- `src/ui/terminal.rs`

当前持久 coding runtime 流程：

1. `start_coding_terminal_runtime` 启动 Claude/Codex/Gemini 的 PTY。
2. 启动时 `wait_for_runtime_ready()` 会短周期读取 `screen_text_lines()`，等待 Claude Code welcome/ready 输出。
3. 用户任务通过 `write_interactive_prompt()` 粘贴进 TUI 并回车。
4. UI 的 `ensure_terminal_refresh_loop()` 每 50ms 执行：
   - `sessions.refresh_all()`
   - `sessions.collect_supervision_requests()`
   - 对每个 request 调用 `CodingSessionSupervisor`
   - `apply_supervision_decision()` 根据结构化结果决定是否通知主聊天区。

`CodingSessionSupervisor` 当前输入：

- terminal transcript
- submitted task
- runtime cwd
- workspace delta

输出结构：

- `running`
- `waiting_user`
- `completed`
- `failed`
- `unclear`

关键点：

- 语义判断方向是合理的：不是简单正则，而是结合终端 transcript 和 workspace delta。
- 触发方式仍然是轮询：UI 刷新循环主动扫描所有活跃 session。
- 轮询频率较高，且终端无变化时也可能反复进入 refresh/collect 流程。
- supervisor 已有 fingerprint/in-flight 去重，但触发入口仍不是事件驱动。

## 当前实现与订阅方案的差异

| 维度 | 当前实现 | 订阅方案 |
| --- | --- | --- |
| 输出变化感知 | UI refresh loop 定时扫描 | PTY/命令运行器主动发布事件 |
| Shell 命令 | 执行完成后一次性写入 UI 状态 | started/output/finished/failed 事件流 |
| Coding runtime | 定时读取 screen transcript 并送 supervisor | 输出变化触发 debounced supervision |
| MainAgent 接收内容 | 依赖工具返回和 UI 通知 | 订阅结构化 runtime 事件 |
| 性能 | 空闲时也有轮询成本 | 空闲时基本无成本 |
| 实时性 | 受 50ms loop 与 supervisor 调度影响 | 输出到达即触发，语义分析可防抖 |
| 复杂度 | 简单、集中在 UI loop | 需要事件总线、订阅生命周期、背压和去重 |
| 可靠性 | 轮询不容易漏事件，但可能重复分析 | 事件更及时，但必须处理事件丢失/乱序/爆量 |

## 你的方案是否合理

结论：方向合理，而且符合长期架构。

原因：

1. 终端本身天然是事件源，输出、退出、标题变化、用户输入、进程状态都应该先进入 runtime event stream。
2. MainAgent 不应该主动扫终端，它应该订阅经过 runtime/supervisor 处理后的语义事件。
3. 普通 shell 命令和持久 coding runtime 都可以统一到同一个事件总线，但事件级别不同。
4. 性能上，事件驱动比固定 50ms 扫描更适合长期多 workspace、多 task、多 agent 并行。
5. 产品语义上，MainAgent 是“中间人/翻译官”，它应该收到“需要用户选择”“命令完成”“编码任务完成”这类业务事件，而不是裸终端日志。

但需要注意：不能直接把所有原始终端输出都推给 MainAgent。Claude Code 这类 TUI 输出非常频繁，直接订阅原始输出会造成噪音、token 浪费和重复判断。正确做法是分层订阅。

## 建议架构

### 总体分层

建议拆成三层事件：

1. `TerminalRawEvent`
   - 面向 UI 和 runtime。
   - 代表底层终端变化。
   - 例如 output dirty、exit、title change、resize、input written。

2. `RuntimeEvent`
   - 面向内部调度和状态机。
   - 代表 Shell/Coding runtime 的结构化状态。
   - 例如 shell command started/output/finished，coding runtime output changed/exited/ready。

3. `AgentNotificationEvent`
   - 面向 MainAgent/主聊天区。
   - 代表用户真正需要知道或参与的语义事件。
   - 例如 coding waiting_user/completed/failed，shell command finished summary。

MainAgent 只订阅第 3 层；UI 可以订阅第 1/2 层刷新显示。

### 事件类型建议

#### TerminalRawEvent

```rust
pub enum TerminalRawEvent {
    OutputChanged {
        terminal_id: String,
        seq: u64,
    },
    Exited {
        terminal_id: String,
        exit_code: Option<i32>,
    },
    TitleChanged {
        terminal_id: String,
        title: String,
    },
}
```

说明：

- `OutputChanged` 不直接携带完整输出，只通知“有变化”。
- 消费方根据需要读取最近 N 行或 screen snapshot。
- 这样可以避免大量复制终端文本。

#### RuntimeEvent

```rust
pub enum RuntimeEvent {
    ShellCommandStarted {
        task_id: Option<usize>,
        command_id: String,
        command: String,
        cwd: PathBuf,
    },
    ShellCommandOutput {
        command_id: String,
        chunk: String,
    },
    ShellCommandFinished {
        command_id: String,
        exit_code: Option<i32>,
        output_tail: String,
    },
    ShellCommandFailed {
        command_id: String,
        error: String,
    },
    CodingOutputChanged {
        session_id: String,
        task_id: usize,
        seq: u64,
    },
    CodingRuntimeExited {
        session_id: String,
        task_id: usize,
    },
}
```

说明：

- Shell 可以做流式输出，也可以先保留完成后一次性 output。长期建议流式。
- Coding 的原始输出变化只触发 supervisor，不直接发给 MainAgent。

#### AgentNotificationEvent

```rust
pub enum AgentNotificationEvent {
    ShellCommandCompleted {
        task_id: Option<usize>,
        command_id: String,
        summary: String,
        output_tail: String,
    },
    ShellCommandFailed {
        task_id: Option<usize>,
        command_id: String,
        summary: String,
    },
    CodingWaitingUser {
        task_id: usize,
        session_id: String,
        message: String,
        options: Vec<CodingSupervisorOption>,
    },
    CodingCompleted {
        task_id: usize,
        session_id: String,
        message: String,
    },
    CodingFailed {
        task_id: usize,
        session_id: String,
        message: String,
    },
}
```

说明：

- 这些事件才应该进入主聊天区或 MainAgent 对话上下文。
- `message` 可以继续由 ONE 模板生成，避免 supervisor 直接写聊天文案。

## 推荐落地方案

### Phase 1：建立 Runtime Event Bus

新增一个轻量事件总线，建议位置：

- `src/runtime/terminal_events.rs`

职责：

- 提供 `TerminalEventBus`。
- 内部使用 `tokio::sync::broadcast` 或 `tokio::sync::mpsc`。
- 支持发布 `RuntimeEvent` 和 `AgentNotificationEvent`。
- AppState 初始化时创建全局/应用级 event bus。

建议不要一开始引入复杂 actor 框架，先用明确的 channel。

验收：

- `run_in_terminal` 完成时能发布 `ShellCommandFinished`。
- UI 仍可正常显示 shell 输出。
- 不改变用户可见行为。

### Phase 2：Shell 命令事件化

当前 `run_in_terminal` 是后台 exec 完成后一次性更新 UI。

建议先做最小稳定改造：

- 命令开始：发布 `ShellCommandStarted`。
- 命令完成：发布 `ShellCommandFinished` 或 `ShellCommandFailed`。
- UI 收到事件后更新 `terminal_output`。
- MainAgent/聊天区可以基于 `ShellCommandFinished` 生成“已完成，结果是……”的简洁反馈。

暂时不强求 stdout/stderr 实时 chunk，因为当前 backend API 可能只返回完整 output。先把生命周期事件打通。

长期可扩展：

- 把 backend exec 改为 streaming。
- 发布 `ShellCommandOutput` chunk。
- UI 增量渲染。

验收：

- Claude/Codex 正在 Coding tab 运行时，执行 `ls` 会产生独立 Shell command events。
- Shell 命令完成后，主聊天区可以得到结果摘要，而不是只提示用户看右侧终端。

### Phase 3：Coding runtime 输出变化事件化

当前 `ensure_terminal_refresh_loop()` 每 50ms 扫描。

建议改为：

- `TerminalEmulator` 在 alacritty 事件中捕捉输出/dirty 事件。
- 如果 alacritty 无法直接提供细粒度 output event，则在 EventLoop listener 中捕捉 `Event::Wakeup` / display update 类事件；如果该事件不可用，再由 TerminalEmulator 内部用低频 watcher 兜底。
- 每次输出变化只发布 `CodingOutputChanged { seq }`。
- `CodingSessionSupervisorRunner` 订阅 `CodingOutputChanged`，做 debounce。

防抖建议：

- 输出变化后等待 300-800ms 沉默窗口再触发 supervisor。
- 如果持续输出，每 2-5 秒最多触发一次语义检查。
- 对 `waiting_user` 类候选状态可以更快触发。

验收：

- Claude 输出大量内容时，supervisor 不会被每一屏刷新疯狂触发。
- Claude 静止等待用户选择时，主聊天区能及时收到一次稳定通知。

### Phase 4：Supervisor 从轮询改为事件驱动

新增 `CodingSupervisorRunner`：

- 订阅 `CodingOutputChanged`。
- 根据 session_id 找到 active turn。
- 读取 recent transcript + workspace delta。
- 调用 `CodingSessionSupervisor`。
- 调用 `apply_supervision_decision()`。
- 发布 `AgentNotificationEvent`。

同时逐步弱化 `ensure_terminal_refresh_loop()` 的职责：

- UI refresh loop 只负责屏幕重绘和 scroll。
- 不再承担 supervisor 调度。
- `refresh_all()` 可以保留为低频健康检查，而不是 50ms 主路径。

验收：

- 主聊天区的 waiting/completed/failed 通知来自事件订阅链。
- UI 关闭终端面板时，coding runtime 仍能被监督并通知 MainAgent。

### Phase 5：MainAgent 订阅语义事件

MainAgent 不直接订阅 raw terminal output。

建议在 AppState 层做统一分发：

- `AgentNotificationEvent::CodingWaitingUser` -> append assistant message。
- `AgentNotificationEvent::CodingCompleted` -> append assistant message。
- `AgentNotificationEvent::ShellCommandCompleted` -> 如果该命令来自 MainAgent tool call，则可以作为 tool result 或后续 assistant message。

这里要区分两类 Shell 命令：

1. 用户明确要看终端实时输出：可以只展示 Shell tab。
2. MainAgent 为回答问题调用命令：应把命令结果总结给用户。

验收：

- 用户问“当前目录有哪些文件”，MainAgent 调用 shell 后，聊天区最终返回目录列表摘要。
- 用户不需要自己去右侧终端理解 `ls` 输出。

## 为什么不建议让 MainAgent 直接订阅原始终端输出

不建议的原因：

- 原始终端输出包含 TUI 控制字符、重复屏幕绘制、进度条、think 内容等。
- Claude/Codex 输出频繁，直接进 MainAgent 会造成 token 浪费。
- MainAgent 会被迫处理大量低价值状态，破坏“中间人翻译官”的角色边界。
- 语义判断需要结合 workspace delta、turn_id、submitted_task，仅看 raw output 不够。

正确边界：

- Raw output 给 UI 和 runtime 状态机。
- RuntimeEvent 给 supervisor。
- AgentNotificationEvent 给 MainAgent/聊天区。

## 方案优劣

### 优点

- 更实时：终端变化主动触发，不依赖固定扫描周期。
- 更省性能：空闲 session 不会反复扫描和分析。
- 更适合多任务：多个 workspace/task/runtime 并行时，事件按 session_id/task_id 路由。
- 更稳定：MainAgent 只处理语义事件，减少噪音。
- 更可扩展：未来可支持 Codex/Gemini、远程 Telegram、后台任务通知、命令审计。

### 缺点

- 实现复杂度提升，需要设计事件生命周期和订阅清理。
- 需要处理事件风暴，必须做 debounce/throttle/backpressure。
- 需要处理事件丢失或 UI 关闭后仍要运行的情况。
- 如果事件总线设计不清楚，容易出现重复通知或状态不同步。

## 风险与处理

### 风险 1：输出事件过于频繁

处理：

- `CodingOutputChanged` 只携带 seq，不携带全文。
- supervisor runner 做 debounce。
- 同一 session 同一 fingerprint 不重复分析。

### 风险 2：事件丢失

处理：

- 保留 session 内的 `output_seq`。
- runner 收到事件后读取当前最新 screen，而不是依赖事件 payload。
- 增加低频健康检查，例如 2-5 秒一次，仅检查 exited/active 状态。

### 风险 3：UI 关闭后 supervisor 停止

处理：

- supervisor runner 不应绑定终端面板可见性。
- 事件总线和 runner 应属于 runtime/app 层，不属于 render loop。

### 风险 4：Shell 命令结果如何回到 MainAgent

处理：

- 为每次 `run_in_terminal` 分配 `command_id`。
- 工具调用上下文记录 command_id。
- 命令完成后将结果摘要回填到当前 task。

### 风险 5：仍然需要少量轮询

处理：

- 允许低频兜底轮询存在。
- 目标不是绝对零轮询，而是把高频 50ms 扫描从主路径移除。

## 建议结论

建议采用你的事件订阅方向，但不要让 MainAgent 直接订阅原始终端输出。

推荐最终形态：

- `TerminalEmulator` 发布低层 raw events。
- `ShellCommandRunner` 发布 shell command lifecycle events。
- `CodingSupervisorRunner` 订阅 coding output events，产出语义 supervision events。
- MainAgent/聊天区只订阅 `AgentNotificationEvent`。
- UI 订阅 raw/runtime events 做显示更新。
- 保留低频健康检查作为兜底，不再用 50ms UI refresh loop 承担 supervisor 调度。

这条路线比当前轮询模型更适合长期多 agent、多 workflow、多 workspace 并行，也更符合 MainAgent 作为中间人/翻译官的产品定位。
