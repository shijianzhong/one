# 持久 Coding Agent 会话实施任务

本文跟踪 `docs/persistent-coding-agent-sessions.md` 的落地状态。

最后更新：2026-06-20

## 总体状态

- [x] 废弃一次性 `claude -p` 编码执行路径。
- [x] 新增 MainAgent 托管的持久 coding CLI session runtime。
- [x] coding CLI cwd 固定使用 `workspace.path`。
- [x] task 只作为对话、日志、metadata、session 归属单元，不作为默认代码目录。
- [x] GUI 和 Telegram 共享同一个进程级 session manager。
- [x] task/workspace 切换不会停止后台 session。
- [x] 终端显示按 active task attached session 切换。
- [x] 同 workspace 同一时间最多一个 write-active session。
- [x] 删除旧两阶段 `src/runtime/coding_workflow.rs`，避免继续误读 `claude -p` 路线。
- [x] 全量自测通过：`ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test`，107 passed。

## Phase 0：现状确认与边界锁定

- [x] 梳理当前 `claude -p` 调用点，并标记为需要替换或删除。
- [x] 梳理当前 `terminal_output` / `terminal_emulator` / `terminal_work_dir` 使用点。
- [x] 梳理 Telegram 非命令消息如何进入 Orchestrator。
- [x] 确认 workspace root 与 task storage dir 的现有生成逻辑。
- [x] 写下需要迁移到 persistent session 的现有行为清单。
- [x] 在本任务文档补充“现状确认”小结。

现状确认小结：

- 旧编码路径位于 `src/runtime/coding_workflow.rs`，通过 `claude -p ... --output-format stream-json --verbose` 一次性执行，已删除。
- 旧 terminal 是全局 `terminal_output` / `terminal_emulator`，不能表达多 task、多 workspace session。现已改为持久 coding session 自带 PTY terminal，UI 根据 active task attach。
- Telegram 非命令消息原本直接跑 Orchestrator 且丢弃 runtime event。现已处理 coding session event，并新增 `/agent ...` 命令。
- `workspace.path` 是项目根目录；`workspace/tasks/<task>` 仍保留为 task 存储目录，不再作为 coding CLI cwd。

验证：

- [x] `rg -n "claude -p|start_coding_workflow|CodingWorkflowRequested|coding_cancel_tx" src` 不再命中运行时代码。
- [x] `cargo check` 通过。

## Phase 1：核心模型与 SessionManager

- [x] 新增 `CodingAgentProvider`，支持从配置解析 provider id、label、command、args。
- [x] 新增 `PersistentSessionStatus`。
- [x] 新增 `PersistentCliSession` 元信息结构。
- [x] 新增 `PersistentCliSessionManager`。
- [x] 支持 `start_session`。
- [x] 支持 `send_input`。
- [x] 支持 `read_recent_output`。
- [x] 支持 `stop_session`。
- [x] 支持 `list_sessions`。
- [x] 支持 `session_for_task`。
- [x] 支持 `active_write_session_for_workspace`。
- [x] 增加基础单元测试：provider 解析、状态活跃判断、task/workspace 绑定、写锁状态流转。

边界：

- [x] 第一版 session 状态存在内存中。
- [x] 不要求 app 重启恢复运行中进程。

验证：

- [x] `ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test persistent_session`

## Phase 2：PTY 持久进程

- [x] 评估复用 `TerminalEmulator` 是否足够。
- [x] 复用 `TerminalEmulator` 作为 PTY runner，并新增 shutdown、退出状态和 screen text snapshot。
- [x] 支持以 workspace root 为 cwd 启动配置中的 provider command。
- [x] 默认内置 `claude`、`codex` 两个 provider。
- [x] 支持持续 stdin 写入。
- [x] 支持持续读取可见输出并写入 session output 视图。
- [x] 支持进程退出检测。
- [x] 支持 shutdown/stop。
- [x] 支持 `output_seq`。
- [x] 最近输出读取按 terminal screen snapshot 限制行数，避免无限返回。

验收：

- [x] 启动 shell 测试 session 后可 send input。
- [x] 输出保留在对应 session terminal。
- [x] stop 后状态更新并释放写锁。

验证：

- [x] `shell_session_binds_task_and_releases_write_lease` 单测通过。

## Phase 3：Workspace Write Lease

- [x] 新增 workspace-level write owner 状态。
- [x] 启动 write session 前检查同 workspace 是否已有 owner。
- [x] 当前 session 是 owner 时允许继续写。
- [x] 另一个 session 是 owner 时阻止启动，并返回结构化错误。
- [x] session stopped/exited/failed 时释放 owner。
- [x] 记录 git baseline：branch、HEAD、`git status --short`。
- [x] 增加单元测试覆盖 write lease 获取、拒绝、释放。

验收：

- [x] 同 workspace Claude running 时，Codex write session 无法直接启动。
- [x] Claude stop 后 Codex 可以启动。

## Phase 4：Session-Scoped Terminal State

- [x] 从全局 `terminal_output` 分离持久 session 输出。
- [x] 新增 `task_attached_session` 映射。
- [x] 新增 `visible_terminal_session` 计算逻辑：根据 active task 找 attached session。
- [x] 当前 task 有 session 时终端显示该 session output。
- [x] 当前 task 无 session 时显示空状态和启动入口。
- [x] 后台 task 输出只进入自己的 session terminal。
- [x] 切换 task/workspace 时终端重新 attach，不停止进程。
- [x] task 列表显示 session badge：`${provider label} running`、`${provider label} stopped` 等。

验收：

- [x] Task A session 输出不会由 Task B 的 attached session 渲染。
- [x] 切回 Task A 能重新显示 Task A 的 session terminal。
- [x] Workspace 切换后终端按当前 task attached session 显示。

## Phase 5：GUI 操作入口

- [x] 在当前 task 终端区域按 `coding_agents` 配置动态渲染启动按钮。
- [x] 支持通过终端键盘输入持续写入 active session。
- [x] 增加 `Stop`。
- [x] Header 显示 session status。
- [x] 自动 attach 当前 task session。
- [x] 当前 task 无 session 时显示引导态。
- [x] 当前 workspace 有其他 write-active session 时，启动失败会明确提示。
- [x] GUI 主输入仍进入 MainAgent；MainAgent 通过 tools 驱动 session。

验收：

- [x] GUI 能启动配置中的 coding CLI session。
- [x] GUI 能发送多轮输入。
- [x] GUI 能停止 session。
- [x] GUI 能正确展示当前 task session。

## Phase 6：MainAgent Tools

- [x] 新增 `start_coding_session` tool。
- [x] 新增 `send_to_coding_session` tool。
- [x] 新增 `read_coding_session_output` tool。
- [x] 新增 `stop_coding_session` tool。
- [x] 新增 `list_coding_sessions` tool。
- [x] 新增 `get_workspace_write_status` tool。
- [x] MainAgent system prompt 增加持久 coding session 规则。
- [x] MainAgent 有 active session 时优先复用的规则写入 prompt。
- [x] MainAgent 能根据“继续 / 同意 / 选 1”转发到当前 task session。
- [x] MainAgent 能根据“看看进度”读取输出并总结。
- [x] MainAgent 在无 session 且 coding 意图明确时可启动 session。
- [x] MainAgent 在 workspace write lease 冲突时提示用户。

验收：

- [x] 用户在 GUI 与 MainAgent 对话即可驱动持久 coding CLI。
- [x] MainAgent 通过 `read_coding_session_output` 获取输出后可总结。

验证：

- [x] `ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test tool_dispatcher`

## Phase 7：Telegram 集成

- [x] 新增 `/agent start claude`。
- [x] 新增 `/agent start codex`。
- [x] 新增 `/agent send <text>`。
- [x] 新增 `/agent status`。
- [x] 新增 `/agent stop`。
- [x] 新增 `/agent attach <task>`。
- [x] 新增 `/agent sessions`。
- [x] Telegram 自然语言消息进入 MainAgent，由 MainAgent 决定是否转发 session。
- [x] Telegram Orchestrator runtime event 已接入共享 session manager。
- [x] 继续使用 chat_id allowlist。
- [x] 高危操作继续使用暗号/本机双确认。

说明：

- Telegram 输出目前按命令/自然语言请求返回，不做持续自动刷屏；这等价于避免刷屏的保守策略。
- 未新增 `remote_agent_auto_start` 配置字段，避免引入配置/数据库迁移；当前通过 MainAgent 规则和 `/agent start ...` 显式启动。

验收：

- [x] GUI 启动的 session 可由 Telegram `/agent status/send/stop` 使用同一个共享 manager 控制。
- [x] Telegram 启动的 session 可在 GUI 中看到。
- [x] Telegram 不会把 Task A session 输出写入 Task B；session 绑定 task。

## Phase 8：Workspace Root 工作目录改造

- [x] 明确 coding CLI cwd 使用 `workspace.path`。
- [x] task storage dir 只用于日志、metadata、输出缓存。
- [x] 删除旧 coding workflow 中把 task_dir 当项目工作目录的路径。
- [x] 空 workspace 创建应用时直接写 workspace root。
- [x] 旧 task artifact preview 逻辑不再由 coding workflow 驱动。
- [x] 更新相关文案，避免用户误解 task 子目录是项目目录。

验收：

- [x] 空 workspace 中让 coding CLI 写应用，cwd 是 workspace root。
- [x] 已有项目 workspace 中让 coding CLI 修改，cwd 是项目根目录。

## Phase 9：彻底替换一次性 `claude -p`

- [x] 删除现有 `claude -p` planning/implementation 路径。
- [x] 不新增 `legacy_once` / `persistent_session` 双模式配置。
- [x] 将 GUI、Telegram、MainAgent、coding workflow 的编码入口全部接入 persistent session。
- [x] 删除只服务一次性 coding runner 的分支、文案和配置。
- [x] 单轮任务由 MainAgent 启动 persistent session、发送任务、读取结果、按策略 stop。

验收：

- [x] 仓库运行时代码中不存在仍会触发 `claude -p` 的编码执行路径。
- [x] 所有 coding CLI 编码入口都复用同一套 persistent session manager。
- [x] 单轮 coding 需求也通过 persistent session 完成。

说明：

- `task_db.rs` 中保留旧 `coding_workflows` 表和相关历史兼容函数，因为用户此前明确要求避免数据库层面的大改。它们不再被运行时代码调用。

## Phase 10：日志、审计与摘要

- [x] session 启动写 run log。
- [x] session input 写 run log。
- [x] session output 通过 terminal snapshot 可读取。
- [x] stop/exit/fail 写 run log。
- [x] 记录 git baseline。
- [x] 支持 MainAgent 读取最近输出并生成摘要。
- [x] 支持用户请求原始最近 N 行输出。
- [x] 支持 task 级 session 状态和输出查看。

验收：

- [x] 可以追踪某 task 的 agent session 做了什么。
- [x] 可以从 UI/Telegram 获得进度输出。

## Phase 11：测试矩阵

- [x] 单 task 启动 Claude，多轮 send：代码路径完成；真实 Claude 依赖本机 CLI。
- [x] 单 task 启动 Codex，多轮 send：代码路径完成；真实 Codex 依赖本机 CLI。
- [x] 单 task 启动配置化 provider，多轮 send：代码路径完成；真实 CLI 依赖本机命令可用。
- [x] Task A 运行时切换 Task B，Task A 后台继续：session manager 不随 task 切换 stop。
- [x] Task A 输出不污染 Task B：terminal 按 task attached session 渲染。
- [x] 切回 Task A 后显示最新输出：terminal 重新 attach。
- [x] 同 workspace Claude write-active 时阻止 Codex write-active：单测覆盖。
- [x] Claude stop 后 Codex 可接力：单测用 shell 覆盖同等 write lease 行为。
- [x] 同 workspace 任意 provider write-active 时阻止另一个 write-active provider：单测用 shell 覆盖同等 write lease 行为。
- [x] 空 workspace 直接在 root 创建应用：cwd 使用 workspace root。
- [x] 已有项目 workspace 修改 root 项目代码：cwd 使用 workspace root。
- [x] GUI 启动，Telegram attach：共享 manager 支持。
- [x] Telegram 启动，GUI attach：共享 manager 支持。
- [x] Stop 后释放 write lease：单测覆盖。
- [x] app 关闭时处理运行中 session：未找到现有 GPUI close hook 可弹提示；已在 `AppState::drop` 中统一 stop 所有持久 session 并释放写锁，避免残留 PTY。

验证命令：

- [x] `cargo fmt`
- [x] `cargo check`
- [x] `ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test`

## Phase 12：文档与用户说明

- [x] 更新用户使用说明。
- [x] 说明 workspace root 与 task 的区别。
- [x] 说明 GUI 与 Telegram 如何 attach 同一个 session。
- [x] 说明同 workspace 不并发写策略。
- [x] 说明一次性 `claude -p` 编码路径已被 persistent session 替代。
- [x] 说明远程安全确认策略。
- [x] 说明 coding CLI provider 可配置。

用户说明：

- GUI：打开某个 workspace/task 后，右侧终端无 runtime 时会按 `coding_agents` 配置显示手动启动按钮，例如 `Start Claude`、`Start Codex`、`Start Gemini`；有 runtime 时终端显示当前 task 绑定的真实 CLI 运行状态，可直接键盘输入，`Stop` 停止并释放写锁。
- MainAgent：主聊天框仍是主要入口。编码需求先通过 `detect_coding_clis` 检查本机 CLI；未安装时询问用户是否安装，用户确认后可调用 `install_coding_cli`；选定 CLI 后通过 `start_coding_terminal_runtime` 打开右侧终端 runtime；“继续/同意/选 1”等通过 `send_to_coding_terminal_runtime` 写入同一个终端 runtime；“看看进度”优先通过 `inspect_coding_terminal_runtime` 判断状态，需要原始输出时通过 `read_coding_terminal_output` 读取。
- Telegram：先 `/workspace <name>`，再使用 `/agent start <provider_id> [任务]`、`/agent send <内容>`、`/agent status`、`/agent stop`、`/agent sessions`、`/agent attach <task_id>`。`provider_id` 来自 `coding_agents` 配置；默认内置 `claude` 和 `codex`。
- cwd：coding CLI 一律在 `workspace.path` 启动。空 workspace 会直接在 root 创建应用；已有项目会直接修改 root 项目代码。task 目录只用于 ONE 的 task storage。
- 写锁：同一个 workspace 同时最多一个 write-active session。所有 provider 都可写，但必须串行。
- 安全：Telegram 继续使用 chat_id allowlist；原有暗号/本机双确认路径保留。

配置示例：

```json
{
  "coding_agents": [
    {
      "id": "claude",
      "label": "Claude",
      "command": "claude",
      "args": [],
      "install_command": "curl -fsSL https://claude.ai/install.sh | bash",
      "install_instructions": "Claude Code 官方安装：macOS/Linux/WSL 可运行 `curl -fsSL https://claude.ai/install.sh | bash`，或 macOS 使用 `brew install --cask claude-code`。安装后在项目目录运行 `claude` 并按提示登录。文档：https://code.claude.com/docs"
    },
    { "id": "codex", "label": "Codex", "command": "codex", "args": [] },
    { "id": "gemini", "label": "Gemini", "command": "gemini", "args": [] }
  ]
}
```

## Phase 13：Coding CLI Provider 配置化

- [x] 在 `Config` 中新增 `coding_agents`。
- [x] 默认配置保留 `claude`、`codex`，不破坏现有使用习惯。
- [x] 新增 `CodingAgentProvider` 替代固定 `CodingAgentKind`。
- [x] 支持 `command + args` 启动 provider。
- [x] GUI 启动按钮按 provider 配置动态渲染。
- [x] Telegram `/agent start` 按 provider id 解析，错误提示动态列出可用 provider。
- [x] MainAgent tool schema 去掉 `claude/codex` enum 限制。
- [x] MainAgent prompt 改为按 provider id 调用持久 coding CLI。
- [x] Orchestrator coding session event 按 provider id 解析，找不到时回落默认 provider。

验证：

- [x] `rg -n "CodingAgentKind|claude\\|codex|Start Claude|Start Codex|Claude/Codex|Claude Code 或 Codex" src` 不再命中源码。
- [x] `cargo fmt`
- [x] `cargo check`
- [x] `ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test`，107 passed。

## Phase 14：MainAgent 代理真实终端 CLI Runtime

- [x] 明确产品语义：MainAgent 不是后台 agent 管理器，而是用户与右侧终端中真实交互式 coding CLI runtime 的中间人。
- [x] 新增 `detect_coding_clis` tool，返回配置中各 CLI 的安装状态、解析路径、安装说明。
- [x] 新增 `install_coding_cli` tool，用户未确认时只返回将执行的安装命令；`confirmed=true` 才会执行安装。
- [x] 默认 Claude Code 自动安装命令使用官方 Native Install：`curl -fsSL https://claude.ai/install.sh | bash`。
- [x] 默认 Claude Code 安装说明包含官方文档与 Homebrew 方案：`brew install --cask claude-code`。
- [x] `start_coding_session` 启动前检查目标 CLI 是否存在；未安装时不启动终端 runtime，返回安装说明。
- [x] MainAgent prompt 改为：编码任务先检测 CLI；多 CLI 让用户选；没有 CLI 时询问是否安装 Claude Code；安装失败给安装说明；选定后启动右侧终端 runtime 并转发整理后的需求。
- [x] 工具描述改为“右侧终端真实 CLI runtime”，减少继续误解成后台抽象 agent。
- [x] GUI 启动成功文案显示 `command=<cli command>`，说明实际运行的是终端命令。
- [x] runtime 启动方式改为先打开默认 shell，再向 shell 输入 `claude` / `codex` / `gemini` 等命令，而不是 PTY 直接 exec CLI。
- [x] Claude Code 启动后先等待 welcome/ready 输出，再发送 MainAgent 整理后的初始需求；超时则不盲发需求，并保留终端输出给用户处理登录/确认等提示。
- [x] Claude Code 启动等待阶段识别登录、目录信任、权限确认、命令缺失等状态；遇到需要用户动作时保留终端 runtime，不发送任务正文。
- [x] 新增 `inspect_coding_terminal_runtime`，MainAgent 可读取并结构化判断 Claude Code 当前状态。
- [x] inspect 支持识别 ready、auth_required、trust_required、permission_required、command_missing、busy、not_active、unknown 等状态。
- [x] 新增 `*_coding_terminal_runtime` 工具别名，产品语义改为右侧终端 runtime；旧 `*_coding_session` 仅保留兼容。
- [x] 新增 dispatcher 单测覆盖 CLI 检测与安装确认保护。
- [x] 移除编码意图本地 fast path：主聊天区编码需求统一进入 MainAgent，由 MainAgent 理解、拆解后通过工具启动或操作 terminal runtime。
- [x] 右侧终端有 active coding runtime 时优先渲染 session PTY 输出，不再被普通命令输出覆盖。
- [x] 普通终端命令输出支持自动换行；coding runtime PTY 输出保留终端布局。
- [x] 终端 scrollback 扩展到历史行并支持向上滚动；在用户停留底部时自动跟随最新输出。
- [x] shell 启动命令与 interactive prompt 输入路径拆分：启动 `claude` / `codex` 用 shell command + Enter，转发用户需求用 bracketed paste + 单独 Enter。
- [x] 转发 prompt 时去掉末尾多余换行，避免 Claude Code TUI 把尾部换行当作继续编辑而不是提交。
- [x] 转发长 prompt 时在 bracketed paste 结束后增加二次 Enter 兜底，修复 Claude Code TUI 已粘贴但未自动提交的问题。
- [x] MainAgent 不再把 coding runtime 最近输出作为默认聊天内容反复粘贴；原始输出保留在右侧终端。
- [x] 新增 `CodingSessionSupervisor`：基于 terminal transcript、提交任务、runtime cwd 和 workspace diff 做结构化语义监督，不再依赖终端日志关键词补丁判断完成状态。
- [x] Supervisor 输出 `running | waiting_user | completed | failed | unclear` JSON 决策，并通过置信度门槛和 fingerprint 去重控制主聊天区通知。
- [x] 终端刷新循环只收集 supervision request，异步调用 supervisor；聊天区只在需要用户交互、任务完成或失败时得到中间人式反馈。
- [x] 识别 Claude Code 编号确认提示，例如 `Do you want to create index.html? 1. Yes 2. Yes, allow all edits ... 3. No`。
- [x] 对重复的登录/信任/权限/编号选择提示做 session 级 fingerprint 去重，避免聊天区重复提醒同一个问题。
- [x] MainAgent prompt 增加选项转发表达：同意/选1→`1`，全部允许/选2→`2`，拒绝/选3→`3`。
- [x] 用户在聊天区回复 Claude Code 待确认选项时，应用层优先拦截并直接转发到对应 terminal runtime，不再交给 MainAgent 自由推理。
- [x] 支持“同意/可以/允许/选1”→`1`，“全部允许/本次都允许/选2”→`2`，“拒绝/不允许/选3”→`3`。
- [x] 对“你不能替我选么/你帮我选”这类非明确授权消息，不停止 session；改为提示用户可在聊天区回复明确选项，由 ONE 代发到终端。
- [x] 权限确认文案改为“可在聊天区回复，我会帮你发送到右侧终端”，不再默认要求用户自己去终端操作。
- [x] 提升 Claude Code 编号选择识别稳定性：支持 `Do you want ...?` 与 `1. Yes` 出现在同一行的 TUI 输出。
- [x] 状态识别优先捕捉底部最新编号选择；登录/信任/权限只看最近尾部输出，避免历史登录提示覆盖当前 overwrite/edit 确认。
- [x] 登录识别改为严格匹配“明确要求登录”的提示；`Authenticated successfully`、`Read/Write/Listed`、`thinking/working` 等正常执行输出不再触发登录提醒。
- [x] 识别 Claude Code plan mode 菜单选择，例如技术栈/功能范围/Submit 的 `Enter to select` 编号菜单，并在聊天区提示用户可回复“选1/选2/选3”。
- [x] 启动或转发给 coding terminal runtime 成功后立即释放 MainAgent 聊天输入 busy 状态，用户可继续聊天、问进度或补充需求。
- [x] 删除本地低风险自动决策逻辑，不再由应用层基于规则替用户选择；需要决策时由 MainAgent/Supervisor 语义判断后与用户沟通。
- [x] Claude Code 编号选择不再走 bracketed paste；用户回复“同意/选1/选2/拒绝”会直接发送短选择按键并回车。
- [x] 右侧终端中 Claude `<think>` 输出在 UI 上弱化显示，减少对关键确认问题的干扰。
- [x] 启动/发送 coding terminal runtime 工具成功后，Orchestrator 本轮立即 handoff 结束，不再额外调用 MainAgent 模型生成收尾，避免 `error decoding response body` 污染聊天区。
- [x] 发送给 runtime 的最终任务文本强制注入当前 runtime cwd 约束：除非用户明确给出绝对路径，否则所有文件操作必须发生在当前 cwd 内。
- [x] 启动 coding session 时记录 workspace baseline，supervisor 每轮基于 workspace diff 验证产物与完成状态；完成后由主聊天区给出闭环反馈。

验证：

- [x] `cargo fmt`
- [x] `cargo check`
- [x] `ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test coding_supervisor`，2 passed。
- [x] `ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test agent_runtime`，2 passed。
- [x] `ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test persistent_session`，11 passed。
- [x] `ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test tool_dispatcher`，10 passed。
- [x] `ONE_MEMORY_DIR=/private/tmp/one-memory-test cargo test`，121 passed。

## 当前暂存任务

以下任务暂不实施，仅保留：

- [ ] 工作流完整 graph editor。
- [ ] HumanApproval 审批人权限与多级审批。
- [ ] MCP end-to-end。
