# 持久 Coding Agent 会话需求与设计方案

## 1. 背景

当前项目中的 Claude Code 编码链路主要是一次性命令模式：

```text
claude -p <prompt> --output-format stream-json --verbose
```

这种模式会导致每次调用都重新开始，无法承载远程持续控制、多轮确认、跨 GUI/Telegram attach、task 切换后继续运行等目标。因此本方案要求彻底废弃一次性 Claude/Codex 编码调用，统一改为由 MainAgent 托管的持久 Claude Code / Codex 命令行会话。Claude/Codex 像用户自己在终端中启动一样持续运行，保留上下文、当前目录、交互状态和输出；MainAgent 负责把用户意图转发给对应会话，并把会话输出整理后反馈给用户。

本方案只描述需求和设计，不包含代码改动。

## 2. 目标

- 将 Claude Code / Codex 从“一次性命令工具”彻底改造为 MainAgent 托管的持久 Coding Agent 会话。
- 删除或停用现有 `claude -p` 一次性 coding runner，避免后续维护两套行为。
- 用户通过 GUI 或 Telegram 与 MainAgent 对话，MainAgent 决定是否启动、复用、读取、转发、停止 Claude/Codex 会话。
- Claude/Codex 会话绑定 task 和 workspace，切换 GUI 当前 task/workspace 不会停止后台会话。
- 每个持久会话的工作目录是 workspace root，而不是 task 子目录。
- 同一个 workspace 下允许 Claude 和 Codex 串行协作同一个项目，但不并发写。
- 页面切换 workspace/task 时，终端区域必须正确显示当前 task 对应的会话状态和输出。
- Telegram 和 GUI 能 attach 到同一个会话：GUI 启动的会话可由 Telegram 继续控制，Telegram 启动的会话可在 GUI 中查看和继续操作。

## 3. 非目标

- 第一阶段不实现 Claude/Codex 同时写同一个 workspace。
- 第一阶段不做 app 关闭后仍保持会话的独立 daemon。
- 第一阶段不做完整 ACP/MCP end-to-end。
- 第一阶段不自动解决多个 agent 修改冲突。
- 不保留 `claude -p` 一次性编码路径作为长期兼容模式。

## 4. 核心用户场景

### 4.1 GUI 启动 Claude Code 写应用

1. 用户选择一个 workspace，workspace path 是项目根目录。
2. 如果 workspace 是空目录，用户对 MainAgent 说：“写一个应用。”
3. MainAgent 判断这是 coding 任务，启动持久 Claude Code session。
4. Claude Code 的 cwd 是 workspace root。
5. Claude Code 直接在 workspace root 创建应用文件，而不是创建 task 子目录。
6. 输出显示在当前 task 的终端区域。

### 4.2 切换 task 后后台继续运行

1. 用户在 Task A 启动 Claude Code。
2. 用户切换到 Task B。
3. Task A 的 Claude Code 不停止，后台继续运行。
4. 当前终端区域切换为 Task B 的会话状态。
5. 用户切回 Task A 时，终端重新 attach 到 Task A 的 Claude Code 输出。

### 4.3 Telegram 远程继续控制

1. 用户在 GUI 启动 Claude Code 后离开电脑。
2. 用户通过 Telegram 发送：“看看进度。”
3. MainAgent 读取 Task A 的 Claude Code 最近输出，总结后通过 Telegram 返回。
4. 用户发送：“同意，继续。”
5. MainAgent 将该回复写入 Task A 的 Claude Code stdin。

### 4.4 Claude 和 Codex 串行协作

1. Claude Code 在 workspace root 完成实现。
2. Claude session 完成、停止或进入空闲。
3. 用户让 Codex review 并修复问题。
4. MainAgent 检查同 workspace 当前没有其他 write-active session。
5. MainAgent 启动 Codex session，cwd 仍是同一个 workspace root。
6. Codex 可读写项目代码，但与 Claude 不并发写。

## 5. 核心概念

### 5.1 Workspace Root

`workspace.path` 是项目根目录，也是 Claude/Codex 的运行目录。

规则：

- 已有项目：Claude/Codex 直接修改 workspace root 内的项目代码。
- 空目录：Claude/Codex 直接在 workspace root 初始化应用。
- 只有用户明确要求创建子目录时，才在 workspace root 下创建子目录。

### 5.2 Task

Task 是 ONE 的工作记录单元，不是项目代码目录。

Task 保存：

- 用户与 MainAgent 的对话。
- 持久会话元信息。
- 会话输出 buffer。
- run log。
- 审批记录。
- 摘要和快照。

Task 不应默认成为 Claude/Codex 的 cwd。

### 5.3 Persistent Coding Session

一个持久 Coding Agent 会话代表一个长期运行的 CLI 子进程，比如 `claude` 或 `codex`。

会话绑定：

- `session_id`
- `workspace_id`
- `task_id`
- `agent_kind`: `claude` / `codex`
- `cwd`: workspace root
- `status`
- `write_mode`
- PTY 进程和 stdin/stdout 状态
- 输出 buffer

### 5.4 Workspace Write Lease

为了避免同一个项目被多个 agent 同时写，workspace 需要一个简单写锁：

```text
workspace_write_owner = session_id | None
```

规则：

- 同一个 workspace 同时最多一个 write-active session。
- Codex 和 Claude 都可以有写权限。
- 如果 Claude 正在写，用户又要求 Codex 写，MainAgent 先提示用户等待、停止 Claude 或只查看状态。
- session 完成、停止、失败或退出时释放 write lease。

## 6. 产品行为

### 6.1 GUI 入口

GUI 应支持：

- `Start Claude`
- `Start Codex`
- `Send`
- `Status`
- `Stop`
- `Attach`

主输入框仍然是用户与 MainAgent 的入口。MainAgent 决定是否：

- 启动持久 session。
- 复用当前 task 的 active session。
- 将用户输入转发给 Claude/Codex。
- 读取输出并总结。
- 要求用户确认危险操作。

### 6.2 Telegram 入口

Telegram 不直接控制 Claude/Codex，而是通过 MainAgent。

建议命令：

```text
/agent start claude
/agent start codex
/agent send <text>
/agent status
/agent stop
/agent attach <task>
/agent sessions
```

自然语言也应支持：

- “用 Claude Code 帮我改这个项目”
- “继续”
- “同意”
- “看看进度”
- “让 Codex review 一下”

推荐配置：

```text
remote_agent_auto_start = ask | always | never
```

初期建议默认 `ask`：远程自动启动写入型 session 前先确认一次。

### 6.3 MainAgent 行为规则

MainAgent 是 session supervisor，不是简单转发器。

规则：

- 当前 task 有 active session 时，优先复用。
- 用户说“继续 / 同意 / 选 1”时，转发给当前 task 的 active session。
- 用户问“现在到哪了”时，读取最近输出并总结。
- 用户明确说“启动 Claude/Codex”时，启动对应 session。
- 用户请求 coding 任务且无 active session 时，可自动建议或启动 session。
- 同 workspace 已有 write-active session 时，不启动第二个写 session。
- 远程高危操作继续走暗号或本机二次确认。

## 7. 终端显示需求

终端输出必须 session-scoped，不能继续使用单一全局输出承载持久会话。

设计原则：

```text
Session output belongs to session.
Session belongs to task.
Task belongs to workspace.
Visible terminal = active task's attached session.
```

行为：

- 切换 task 不停止 session。
- 当前终端区域 attach 到 active task 的 session。
- 后台 task 的 session 输出继续进入自己的 buffer，不污染当前终端。
- 当前 task 没有 session 时，终端区域显示空状态和启动入口。
- 侧边栏 task 可显示状态 badge：`Claude running`、`Codex waiting`、`done`、`error`、`new output`。

现有全局字段如 `terminal_output`、`terminal_emulator` 如果只服务一次性 Claude/Codex 编码路径，应随本次改造删除或降级为非 coding 场景使用。持久 Claude/Codex 会话必须迁移到 session-scoped terminal state。

## 8. 技术设计

### 8.1 数据结构草案

```rust
enum CodingAgentKind {
    Claude,
    Codex,
}

enum PersistentSessionStatus {
    Starting,
    Running,
    WaitingInput,
    Idle,
    Exited,
    Failed,
    Stopped,
}

struct PersistentCliSession {
    session_id: String,
    workspace_id: usize,
    task_id: usize,
    agent_kind: CodingAgentKind,
    cwd: PathBuf,
    status: PersistentSessionStatus,
    write_mode: bool,
    started_at: String,
    last_active_at: String,
    output_seq: u64,
    output_buffer: Vec<TerminalLine>,
}
```

Runtime-only fields:

```rust
struct RunningCliProcess {
    pty: TerminalEmulator or lower-level PTY handle,
    stdin_writer: ...,
    output_reader_task: ...,
}
```

### 8.2 Session Manager API

```rust
start_session(task_id, workspace_id, agent_kind, cwd, write_mode) -> session_id
send_input(session_id, text)
read_recent_output(session_id, cursor, limit)
stop_session(session_id)
list_sessions()
session_for_task(task_id)
active_write_session_for_workspace(workspace_id)
attach_task_session(task_id, session_id)
```

### 8.3 MainAgent Tools

```text
start_coding_session(agent_kind, write_mode)
send_to_coding_session(session_id, text)
read_coding_session_output(session_id, limit)
stop_coding_session(session_id)
list_coding_sessions()
get_workspace_write_status()
```

工具应返回结构化结果，供 MainAgent 总结给用户。

### 8.4 PTY 启动

持久 session 需要 PTY，而不是简单 stdout/stderr pipe。

原因：

- `claude` / `codex` 交互式模式可能依赖 TTY。
- 需要持续 stdin。
- 需要保留会话状态。
- 输出可能包含交互提示、选择项、确认提示。

启动策略：

```text
program = claude | codex
cwd = workspace.path
stdin = persistent
output = session output buffer
```

也可以先启动 shell，再写入 `claude\n` / `codex\n`，但 MVP 建议直接以 program 启动。

### 8.5 输出桥接

不要把 Claude/Codex 每行输出都推给用户。

需要 `Conversation Bridge`：

- 聚合最近输出。
- 识别等待输入、确认、错误、完成等状态。
- MainAgent 读取后总结。
- 用户要求原始输出时，返回最近 N 行。
- Telegram 回推需要 debounce，避免刷屏。

## 9. 持久化策略

第一阶段可以 runtime 内存为主，DB 记录关键元信息。

建议持久化：

- session_id
- workspace_id
- task_id
- agent_kind
- cwd
- status
- write_mode
- started_at / finished_at
- last_error
- run log id

输出 buffer 可先内存保存，同时持续写 run log。后续再考虑完整恢复。

第一阶段不承诺 app 重启后恢复正在运行的 CLI 进程。

## 10. 安全策略

- Telegram 继续使用 chat_id allowlist。
- 远程启动 write session 默认需要确认或配置允许。
- 高危操作继续走暗号或本机双确认。
- 启动 write session 前记录 git baseline：
  - branch
  - HEAD commit
  - `git status --short`
- 同 workspace 已有 write-active session 时阻止第二个写 session。
- 删除 task/workspace 前必须停止或确认 detach 对应 session。

## 11. 与现有系统关系

### 11.1 替换 `claude -p`

本次改造不采用长期并存策略。所有 Claude/Codex 编码入口统一收敛到 persistent session runtime。

替换策略：

- 找出所有 `claude -p` 一次性编码调用点。
- 用 persistent session runtime 替换这些调用点。
- 删除只服务一次性 coding runner 的配置、分支和文案。
- 不新增 `legacy_once` / `persistent_session` 双模式配置。
- 如存在非交互式分析场景，也应通过持久 session 执行单轮任务后由 MainAgent stop，而不是重新引入一次性 runner。

### 11.2 与 Workflow / Capability

本功能不是 workflow graph 的替代。

关系：

- Persistent coding session 是外部 coding agent runtime。
- Workflow capability 未来可以调用 persistent session tools。
- MainAgent 可同时调度 capabilities、skills、system tools、coding sessions。

### 11.3 与 Terminal

持久 coding session 的终端输出需要 session-scoped。

现有全局 terminal output 不能继续承载 Claude/Codex 编码输出。持久 coding session 的输出必须归属到对应 session/task/workspace。

## 12. MVP 范围

MVP 必须完成：

- 支持 `claude` 和 `codex` 两种持久 PTY session。
- cwd 固定为 `workspace.path`。
- 每个 task 最多一个默认 attached session。
- 每个 workspace 最多一个 write-active session。
- GUI 可以 start/send/read/stop/attach。
- MainAgent tools 可以 start/send/read/stop/list。
- Telegram 可以 start/send/status/stop/attach。
- 切换 task/workspace 时终端显示正确 session。
- 输出进入对应 task 的 buffer，不串 task。

MVP 不做：

- app 重启后恢复运行中的进程。
- 多 agent 并发写。
- 自动合并冲突。
- 完整 daemon。
- 完整 ACP/MCP。

## 13. 验收标准

- 在 workspace A 的 task 1 启动 Claude，切换到 task 2 后 Claude 仍在后台运行。
- task 2 终端不显示 task 1 的 Claude 输出。
- 切回 task 1 后能看到 Claude 最新输出。
- workspace 是空目录时，Claude/Codex 在 workspace root 创建应用，而不是 task 子目录。
- 同 workspace Claude 正在 write-active 时，Codex write session 启动被阻止或要求确认处理。
- Claude 完成/停止后，Codex 可以接力写同一个 workspace。
- GUI 启动的 session 可通过 Telegram 查询状态和发送输入。
- Telegram 启动的 session 可在 GUI 中查看输出。
- MainAgent 能总结 session 最近输出，而不是只原样转发。
