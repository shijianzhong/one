 当前项目方向是成立的：用 Rust + GPUI 做一个轻量原生 AI 工作台，核心不是“聊天 App”，而是以 Workspace/Task 为组织单位，把聊天、编码 Agent、终端、系统工具、记忆和技能市场统一成一个入口。这个方向比 Electron/VSCode 二开更有差异化。

  但当前实现已经进入“功能快速堆叠期”：很多产品主线还没固化，多 Agent、系统工具、Claude Code、记忆、UI 状态都混在 src/main.rs:1157 里。继续这样加功能，会很快变成难测、难改、行为不可预测的桌面壳。

  当前实现概览
  产品目前实际有这些模块：

  - GPUI 桌面主应用：Chat、导航、Workspace/Task、模型配置、终端、Claude Code 面板主要都在 src/main.rs:1759。
  - SQLite 任务与消息持久化：src/task_db.rs:349。
  - OpenAI-compatible API 流式调用：src/services/api.rs:72。
  - 快速意图路由：src/agents/intent_router.rs:58。
  - 多 Agent 编排雏形：src/agents/core/orchestrator.rs:23。
  - Claude Code CLI 适配：src/agents/claude_code.rs:207。
  - 本地记忆：文件快照、TF-IDF 检索、profile facts：src/memory/snapshot.rs:10、src/memory/storage.rs:7。
  - ACP 协议库雏形：acpx/，测试覆盖相对最好。

  已经暴露的实际问题
  我跑了 cargo test --workspace，结果失败：

  agents::intent_router::tests::test_system_keywords FAILED
  assertion failed: router.needs_llm_intent("查看进程")

  原因很明确：代码里系统类请求会直接返回 SystemTools，但测试期待 needs_llm_intent() 为 true。也就是说路由语义已经不一致：src/agents/intent_router.rs:96 和测试 src/agents/intent_router.rs:119 对“是否需要 LLM 意图分析”的理解相反。

  另外还有 21 个 one crate 警告，包括未使用的 keyword_classifier、未使用的 intent agent 状态、未使用的 spawn_intent_agent_run。这说明旧路由方案和新 orchestrator 方案并存，但没有完成收敛。

  主要架构问题

  1. 路由策略重复且冲突
     现在有 IntentRouter、KeywordClassifier、IntentAgent、Orchestrator 四套路由/分类逻辑。主流程实际是：能 quick route 就直接处理，否则进 orchestrator：src/main.rs:1166。但 KeywordClassifier 和 LLM IntentAgent 已经基本游离。建议只
     保留一条主链路：规则预判高置信任务，其他进入 planner/orchestrator。

  2. main.rs 职责过重
     main.rs 超过 6000 行，里面同时处理 UI、数据库、API 调用、进程、Agent 事件、终端命令和预览状态。建议拆成：
     app_state.rs、ui/、routes.rs、runtime/jobs.rs、chat/session.rs、workspace.rs、terminal.rs、agent_panel.rs。

  3. 记忆架构和 README 描述不完全一致
     README 说 L3 是 embedding 语义层，但当前 L3 是 l3_chunks.json + TF-IDF：src/memory/snapshot.rs:49。这适合 MVP，但不应继续叫“生产级三层记忆”。更好的方案是先统一 MemoryStore 接口，当前 TF-IDF 作为 fallback，后面替换为 sqlite-
     vss/Qdrant。

  4. Claude Code 权限风险过高
     Claude CLI 调用使用 --permission-mode bypassPermissions：src/agents/claude_code.rs:209。这对个人实验方便，但产品化必须改成可配置权限模型，至少按 workspace/task sandbox 隔离，并把危险操作显式确认。

  5. ShellTool 也缺权限边界
     多 Agent 的 ShellTool 直接 sh -c 执行模型给出的命令：src/agents/core/tools/mod.rs:82。这比 system tools 的危险确认更危险，因为它绕过了结构化工具的安全层。建议 CodingAgent 只委托 Claude Code/ACP，不直接暴露裸 shell，或者所有
     shell 命令走统一 approval/sandbox runtime。

  6. 数据模型还不够支撑 Agent 产品
     SQLite 有 workspaces/tasks/messages，但 agent tables 多数是 CRUD 雏形，主流程没有真正围绕 agent instance/session 落库。消息也只保存 role/content，tool calls、run events、artifact、agent trace 基本在旁路文件或 UI 状态里。后续
     要做“任务可恢复”和“执行可审计”，需要统一 event log。

  7. 依赖本机绝对路径
     Cargo.toml:11 依赖 /Users/shijianzhong/sking/zed/...，协作、CI、打包都会受阻。短期可接受，下一步应改成 git submodule、workspace sibling 环境变量，或明确 dev bootstrap 脚本。

  更好的方案
  我建议产品主线收敛为：

  1. ONE = Task-centric Agent Workspace
     每个 Task 是一个可恢复工作单元，包含 chat、agent runs、terminal output、artifacts、memory snapshot。

  2. 路由只保留两级
     一级 deterministic router：明显系统查询、明显编码任务、普通闲聊。
     二级 orchestrator：复杂或不确定任务。
     删除或合并 KeywordClassifier 和未使用的 IntentAgent 状态。

  3. Agent Runtime 统一事件模型
     定义 RunEvent：started、message_delta、tool_call、tool_result、approval_required、artifact_created、finished、failed。UI 只消费事件，不直接知道 Claude/system/general 的内部细节。

  4. 安全层前置
     所有 destructive/file/shell 操作统一走 PermissionPolicy。Claude Code 和 ShellTool 不能各搞一套权限。

  5. 记忆先做实用版，不急着上向量库
     先把 MemoryStore 抽象出来：save_turns、save_snapshot、search_related、save_fact。默认 SQLite + FTS5/TF-IDF，等产品验证后再接 embedding。

  优先级建议
  P0：修复当前测试失败，收敛路由语义。
  P1：把 main.rs 中的路由/Agent job/Claude run 拆出去。
  P1：移除或接入未使用的 KeywordClassifier、IntentAgent，避免双轨逻辑。
  P1：替换 bypassPermissions 和裸 sh -c 为统一权限确认。
  P2：建立 task event log，让 Agent 执行可恢复、可审计。
  P2：把 memory 存储从当前目录 memory/ 移到 app config/data 目录，避免不同启动目录导致记忆分裂。
  P3：整理 Cargo 依赖路径，准备 CI 和可复现构建。
