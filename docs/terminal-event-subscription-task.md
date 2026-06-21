# 终端事件订阅迭代任务

## 目标

把右侧终端从“UI 刷新循环高频扫描并顺便触发监督”逐步改成“终端/runtime 主动发布事件，MainAgent/主聊天区订阅语义事件”的架构。

本轮目标先完成稳定基础：

- 建立 runtime event bus。
- Shell 命令发布生命周期事件。
- Coding runtime 的 PTY 输出变化发布事件。
- Coding supervisor 从固定 50ms 扫描触发，改为输出变化事件触发并防抖。
- 保留低频/状态刷新兜底，避免事件丢失导致状态卡住。

## 设计边界

- MainAgent 不直接订阅 raw terminal output。
- UI 可以消费 raw/runtime event 刷新显示。
- Supervisor 消费 coding output changed 事件，读取最近 transcript + workspace delta 做语义判断。
- 主聊天区只接收 waiting/completed/failed 这类语义通知。
- 事件订阅不改变 Shell/Coding tab 的用户可见模型。

## Phase 1：事件类型与事件总线

- [x] 新增 `src/runtime/terminal_events.rs`。
- [x] 定义 `RuntimeEvent`：
  - [x] `TerminalOutputChanged`
  - [x] `TerminalExited`
  - [x] `TerminalTitleChanged`
  - [x] `ShellCommandStarted`
  - [x] `ShellCommandFinished`
  - [x] `ShellCommandFailed`
  - [x] `CodingOutputChanged`
- [x] 定义 `TerminalEventBus`，支持 `publish()` / `subscribe()`。
- [x] 提供全局 `global_terminal_event_bus()`。
- [x] 在 `runtime/mod.rs` 导出事件总线。
- [x] 添加基础单元测试。

## Phase 2：TerminalEmulator 发布 raw 事件

- [x] `TerminalListener` 增加 `terminal_id` 和 event bus。
- [x] `Event::Wakeup` 发布 `TerminalOutputChanged`。
- [x] `Event::Exit` / `Event::ChildExit` 发布 `TerminalExited`。
- [x] `Event::Title` 发布 `TerminalTitleChanged`。
- [x] `TerminalEmulator` 支持带 terminal id 创建。
- [x] Shell tab 和 Coding runtime 分别传入稳定 terminal id。

## Phase 3：Shell 命令生命周期事件

- [x] `run_in_terminal` 为每次命令分配 `command_id`。
- [x] 命令开始发布 `ShellCommandStarted`。
- [x] 命令成功发布 `ShellCommandFinished`。
- [x] 命令失败发布 `ShellCommandFailed`。
- [x] 保持现有 UI 输出行为不变。
- [x] 更新 `tool_dispatcher` 相关测试。

## Phase 4：Coding runtime 事件触发 supervision

- [x] `PersistentCliSessionManager` 维护 terminal output seq。
- [x] 收到 `TerminalOutputChanged` 后转换为 `CodingOutputChanged`。
- [x] 新增/调整 app 级订阅 loop，监听 `CodingOutputChanged`。
- [x] 对同一 session 做 debounce/throttle。
- [x] 触发 `collect_supervision_request_for_session()`，不再全量扫描所有 session。
- [x] 保留低频 `refresh_all()` 状态兜底。
- [x] UI 刷新循环移除高频 `collect_supervision_requests()` 全量扫描。

## Phase 5：验证

- [x] `cargo fmt`
- [x] `cargo check`
- [x] `ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test terminal_events`
- [x] `ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test tool_dispatcher`
- [x] `ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test persistent_session`
- [x] `ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test`，125 passed。

## Phase 6：事件链路诊断与监督误判保护

- [x] 新增 runtime 链路日志，写入 `~/.one/logs/terminal-events.log`。
- [x] 记录 raw terminal output、AppState 事件转换、supervision debounce、请求采集、模型返回、决策应用、保护触发等关键节点。
- [x] `apply_supervision_decision()` 在接受 `failed` / `completed` 前复查当前终端状态。
- [x] 如果终端本地识别为 `choice_required` / `permission_required` / `auth_required` / `trust_required`，拒绝把 session 标为 failed/completed，改为 `waiting_user` 通知主聊天区。
- [x] 增加回归测试：当 Claude Code 正在询问是否创建文件而 supervisor 误判 failed 时，系统转为用户确认，不再把 session 标失败。
- [x] `cargo fmt`
- [x] `cargo check`
- [x] `ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test persistent_session`
- [x] `ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test terminal_events`
- [x] `ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test`，126 passed。
