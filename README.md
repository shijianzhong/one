# ONE — 轻量级 AI 智能体桌面应用

**ONE** 是一个基于 Rust + [GPUI](https://github.com/zed-industries/zed) 构建的本地 AI 智能体。不依赖 Electron，从零拥抱 Rust，给你一个真正轻快、无负担的 AI 助手体验。

---

## 核心理念

**ONE = 一 = 连接万物的那个点。**

- **一个入口** — 通过 ONE 连接一切，而不是在无数 AI 应用间切换
- **万物归一** — 工具、记忆、技能都围绕同一个主 Agent 运转
- **轻装上阵** — 原生 GPU 渲染，非 WebView 套壳，内存占用 50-100MB

---

## 与主流方案对比

| 维度 | Electron/VSCode 方案 | ONE |
|------|---------------------|-----|
| 技术栈 | JavaScript + Node.js | Rust + GPUI（原生） |
| 内存占用 | 300MB – 1GB+ | 预计 50–100MB |
| 启动速度 | 5–15 秒 | < 1 秒 |
| 包体积 | 数百 MB | ~10MB |
| UI 渲染 | WebView + CSS | GPU 加速原生渲染 |

---

## 整体架构

```
┌──────────────────────────────────────────────────────────────────┐
│                         GPUI 渲染层                               │
│  Nav │ Chat（主对话） │ Sidebar（产物/预览） │ Terminal（终端）    │
└──────────────────────────────────────────────────────────────────┘
              │                    │
              ▼                    ▼
┌─────────────────────┐   ┌──────────────────────┐
│     路由层           │   │     运行时层          │
│  IntentRouter        │   │  JobManager           │
│  (关键词快速分流)     │   │  (任务生命周期管理)   │
└──────┬──────────────┘   └──────────────────────┘
       │
  system 关键词 ──► spawn_system_tools_run
  其他所有消息 ──► spawn_orchestrator_run
       │
       ▼
┌─────────────────────────────────────────────────────────────────┐
│                        Agent 层                                  │
│                                                                  │
│   Orchestrator（调度外壳，最多 15 步循环）                        │
│       │                                                          │
│       ▼                                                          │
│   MainAgent（唯一 Agent，有人格、有工具、流式对话）               │
│       │                                                          │
│       ├── remember / recall        → 记忆系统                    │
│       ├── run_system_task          → Skill Registry（技能分发）  │
│       ├── analyze_disk / clean_disk → 磁盘工具                   │
│       ├── propose_soul_update      → Soul 草案审核队列            │
│       └── update_work_dir          → 工作目录切换                │
└─────────────────────────────────────────────────────────────────┘
       │
       ▼
┌────────────────────────────────────┐
│           Skill Registry           │
│  system.cleaner   磁盘清理          │
│  desktop.organizer  桌面整理        │
│  app.uninstaller  应用卸载          │
│  doc.summarizer   文档摘要          │
│  media.dedup      媒体去重          │
│  （未来通过 Skill Market 扩展）      │
└────────────────────────────────────┘
       │
       ▼
┌────────────────────────────────────┐
│           记忆系统（三层）           │
│  L1  当前会话上下文（Context）       │
│  L2  用户画像 profile.json          │
│       ├─ global/profile.json（跨ws）│
│       └─ <ws>/profile.json（项目级）│
│  L3  历史 Task 快照 + TF-IDF 检索   │
└────────────────────────────────────┘
```

---

## 消息处理流程

```
用户发送消息
    │
    ▼
route_message()
    │
    ├─ 已有 Orchestrator 等待用户输入？
    │       └─ 直接发给 orchestrator_user_input_tx ─► Orchestrator 继续
    │
    ├─ IntentRouter 命中 system 关键词？
    │       └─ spawn_system_tools_run（OS 操作快速路径）
    │
    └─ 其他（默认）
            └─ spawn_orchestrator_run
                    │
                    ▼
              Orchestrator.run_task()
                    │ 注入记忆（global + workspace facts + L3 上下文）
                    │ 注入历史消息
                    │
                    ▼
              MainAgent.step_stream()  ──► 流式 delta → UI 实时显示
                    │
                    ├─ AgentResponse::Answer ──► 多轮等待 or 返回结果
                    │
                    └─ AgentResponse::ToolCalls
                            │
                            ├─ run_system_task(skill_id) ──► Skill.preview/execute
                            ├─ run_system_task(task)     ──► system_tools 直调
                            ├─ remember/recall           ──► memory::profile
                            ├─ analyze_disk/clean_disk   ──► 磁盘操作
                            ├─ update_work_dir           ──► 修改工作目录
                            └─ propose_soul_update       ──► Soul 审核队列
```

---

## 项目结构

```
solo3_gpui/
├── src/
│   ├── main.rs                  # 应用入口、AppState、GPUI 初始化
│   ├── app_state.rs             # 全局应用状态（UI 数据、配置）
│   ├── routing.rs               # 消息路由（IntentRouter → JobManager）
│   ├── workspace.rs             # Workspace / Task 数据结构
│   ├── task_db.rs               # SQLite 持久化（sqlez）
│   ├── run_log.rs               # 运行日志记录（RunRecorder）
│   ├── i18n.rs                  # 国际化（中英双语）
│   ├── ui_theme.rs              # 语义化主题颜色系统
│   ├── util.rs                  # 工具函数
│   │
│   ├── agents/                  # Agent 体系
│   │   ├── core/
│   │   │   ├── mod.rs           # Agent / Tool trait、BaseAgent、AgentContext
│   │   │   ├── main_agent.rs    # MainAgent（唯一 Agent，内置 7 个工具）
│   │   │   ├── orchestrator.rs  # Orchestrator（外层调度循环 + 工具分发）
│   │   │   ├── factory.rs       # AgentFactory（创建 Orchestrator 入口）
│   │   │   └── tools/           # （工具实现，已迁移到 main_agent.rs）
│   │   ├── intent_router.rs     # 关键词快速路由
│   │   ├── permission.rs        # 权限策略（Allow/Deny/Ask + RemoteScopeGuard）
│   │   ├── remote_auth.rs       # 远程暗号认证（bcrypt）
│   │   ├── soul.rs              # Soul 人格草案审核队列
│   │   └── types.rs             # 共享类型（RoutingDecision、RequestKind 等）
│   │
│   ├── memory/                  # 三层记忆系统
│   │   ├── profile.rs           # 用户事实读写（global + workspace 两级）
│   │   ├── storage.rs           # Task 消息持久化、路径工具函数
│   │   ├── search.rs            # TF-IDF 检索（L3 语义召回）
│   │   ├── snapshot.rs          # 对话快照生成（LLM 摘要 → L2）
│   │   └── types.rs             # ChatMessage 等基础类型
│   │
│   ├── runtime/                 # 任务运行时
│   │   ├── job_manager.rs       # JobManager（所有异步任务生命周期）
│   │   └── events.rs            # 运行时事件枚举
│   │
│   ├── services/                # 外部服务
│   │   ├── api.rs               # LLM API 调用（OpenAI-compatible 流式）
│   │   └── config.rs            # 配置加载/保存（~/.one/config.json）
│   │
│   ├── skills/                  # 技能系统（Skill Market）
│   │   ├── mod.rs               # Skill trait、SkillRegistry、SkillManifest
│   │   ├── system_cleaner.rs    # 磁盘清理
│   │   ├── desktop_organizer.rs # 桌面整理
│   │   ├── app_uninstaller.rs   # 应用卸载
│   │   ├── doc_summarizer.rs    # 文档摘要
│   │   └── media_dedup.rs       # 媒体去重
│   │
│   ├── triggers/                # 远程触发器
│   │   ├── mod.rs               # Trigger trait、TriggerEvent
│   │   ├── telegram.rs          # Telegram Bot（long-poll + 白名单）
│   │   └── dispatcher.rs        # 命令解析与 Skill 分发
│   │
│   ├── ui/                      # UI 层
│   │   ├── mod.rs               # 主渲染入口（AppState::render）
│   │   ├── chat.rs              # 聊天界面（Composer、消息流）
│   │   ├── nav.rs               # 左侧导航栏（Workspace/Task 列表）
│   │   ├── sidebar.rs           # 右侧侧边栏（Artifacts、Preview、References）
│   │   ├── subagent.rs          # Subagent 消息卡片渲染
│   │   ├── dialogs.rs           # 弹窗（权限、导出、Workspace 菜单）
│   │   ├── terminal.rs          # 内嵌终端
│   │   └── components.rs        # 通用组件（代码块、进程表等）
│   │
│   └── sandbox/                 # 终端后端
│       └── backend.rs           # Pty（alacritty）+ Docker 可选沙箱
│
├── skills/
│   └── system_tools/            # 独立 crate：OS 工具函数库
│       └── src/tools/           # disk、file、process 工具
│
├── components/                  # 独立 crate：可复用 GPUI 组件
│   └── src/                     # TextInput、Button、Checkbox 等
│
├── docs/                        # 设计文档
│   ├── memory-plan.md           # 记忆系统改造方案
│   ├── remote-auth-plan.md      # 远程触发安全认证方案
│   └── performance-optimization.md  # 性能优化方案
│
├── vendor/zed/                  # git submodule：GPUI 源码
├── patches/                     # vendor/zed 本地补丁
└── scripts/                     # 构建脚本
    └── apply-zed-patches.sh
```

---

## 核心模块详解

### Agent 层

**设计原则**：一个 MainAgent，通过工具调用一切，没有多余的中间 Agent。

| 组件 | 职责 |
|------|------|
| `Orchestrator` | 外层调度循环（最多 15 步），拦截特殊工具调用，注入记忆上下文 |
| `MainAgent` | 唯一 Agent，加载 soul.md 人格，内置 7 个工具，流式对话 |
| `AgentFactory` | 创建 Orchestrator + MainAgent 的工厂函数 |
| `IntentRouter` | 同步关键词匹配，system 类操作走快速路径 |

**MainAgent 内置工具**

| 工具 | 说明 |
|------|------|
| `run_system_task` | 分发到 Skill Registry（skill_id）或 system_tools（自然语言 task） |
| `analyze_disk` | 磁盘空间分析，列出占用大的目录 |
| `clean_disk` | 清理废纸篓、下载、缓存或自定义路径 |
| `remember` | 写入用户事实（global / workspace / both 三个范围） |
| `recall` | 读取全局 + 当前 workspace 所有事实并去重 |
| `propose_soul_update` | 提交 soul.md 修订草案，须用户在 UI 确认后生效 |
| `update_work_dir` | 切换工作目录，下次 Skill 调用时生效 |

### 记忆系统（三层）

```
L1  当前会话（Context window）
      ↓ 对话结束异步生成
L2  结构化快照（profile.json + task snapshot .md）
      全局：~/.one/memory/global/profile.json        ← 跨 workspace 用户事实
      项目：~/.one/memory/<workspace>/profile.json   ← 当前项目上下文
      ↓ 同时写入
L3  历史 Task chunks（TF-IDF 索引，memory/tasks/<ws>/）
      每次 run_task 开始时，通过 build_memory_context 召回相关片段
```

新会话开始时，Orchestrator 自动注入：全局事实 + workspace 事实 + L3 相关历史，无需 LLM 主动调 `recall`。

### Skill 系统

每个 Skill 实现 `preview()` + `execute()` 两步接口：
- `preview(args)` — 只读分析，返回预计影响范围，无副作用
- `execute(args, source)` — 实际执行，走 `PermissionPolicy` 权限校验

危险等级通过 `SkillManifest.danger_level` 声明（`Normal` / `Dangerous` / `Extreme`），影响远程触发时的确认流程。

### 权限系统

```
PermissionMode::Default   → 本机操作：Ask 弹窗确认
PermissionMode::Strict    → 远程触发：Shell 直接 Deny，File/Process 弹窗确认
RemoteScopeGuard::enter() → 自动将当前线程升级为 Strict 模式
```

### 远程触发（Telegram）

通过环境变量或设置页配置 Bot Token + chat_id 白名单，支持命令：

| 命令 | 说明 |
|------|------|
| `/help` | 帮助 |
| `/skills` | 列出已安装 Skill |
| `/preview <id> [json]` | Skill 预览（只读） |
| `/run <id> [json]` | Skill 执行（需暗号确认） |
| `/audit [n]` | 查看最近 N 条运行日志 |

---

## 构建与运行

> ONE 把 zed 仓库以 git submodule 形式 vendor 在 `vendor/zed/`，并通过 `patches/` 维护本地补丁。

```bash
# 1. 克隆仓库（带子模块）
git clone --recursive <repo-url> solo3_gpui
cd solo3_gpui

# 2. 应用本地补丁（幂等，重复执行安全）
bash scripts/apply-zed-patches.sh

# 3. 构建运行
cargo build
cargo run

# 运行测试
cargo test
cargo test --manifest-path skills/system_tools/Cargo.toml

# 构建独立 crate
cargo build --manifest-path components/Cargo.toml
cargo build --manifest-path skills/system_tools/Cargo.toml
```

### 环境配置

配置文件位于 `~/.one/config.json`，支持通过 UI 设置页修改：

```json
{
  "model_name": "claude-sonnet-4-5",
  "model_base_url": "https://api.anthropic.com",
  "model_api_key": "sk-...",
  "language": "zh",
  "telegram_bot_token": "...",
  "telegram_chat_id": "..."
}
```

人格设定位于 `~/.one/soul.md`。仓库根目录的 `soul.md` 仅作为首次启动的初始化模板；如果本机 `~/.one/soul.md` 不存在，应用会自动从模板创建。后续可在应用内通过 `propose_soul_update` 工具发起修改草案，需用户在 UI 确认后生效。

---

## 技术选型

| 组件 | 技术 |
|------|------|
| GUI 框架 | GPUI（Zed 编辑器同款） |
| 语言 | Rust 1.95 |
| 数据库 | SQLite（via sqlez） |
| 终端 | alacritty_terminal + portable-pty |
| HTTP | reqwest 0.12 |
| 异步运行时 | tokio + gpui_tokio |
| LLM API | OpenAI-compatible（流式 SSE） |
| 记忆检索 | TF-IDF（自研，无向量数据库依赖） |
| 认证加密 | bcrypt（远程暗号） |

---

## Roadmap

### 已完成
- [x] Chat 界面与流式消息
- [x] Workspace / Task 管理（SQLite）
- [x] MainAgent + Orchestrator 架构
- [x] 三层记忆系统（L1/L2/L3 + 主动注入）
- [x] Skill 系统（5 个内置 Skill + Registry）
- [x] 权限系统（PermissionPolicy + RemoteScopeGuard）
- [x] Telegram 远程触发（M3）
- [x] Soul 人格系统（草案审核机制）
- [x] 内嵌终端（alacritty）
- [x] 国际化（中/英）
- [x] 深浅色主题

### 进行中
- [ ] 远程安全认证（暗号 + 双确认，M3 收尾）
- [ ] 记忆全局层 + 主动注入完整落地（memory-plan Phase 1-3）
- [ ] 性能优化：polling loop 改为 recv().await（performance-optimization Phase 1-2）

### 规划中
- [ ] Skill Market（外部 Skill 安装/管理）
- [ ] Coding Skill（通过 claude CLI，走 Skill Market 安装）
- [ ] Windows 支持
- [ ] 向量数据库集成（L3 升级）
- [ ] 多模型分级（轻量模型处理简单对话）

---

## 为什么不是单 workspace？

zed 的所有子 crate 用 `*.workspace = true` 继承自 `vendor/zed/Cargo.toml`。Cargo 不支持嵌套 workspace，外层声明 `[workspace]` 会"接管" `vendor/zed/crates/...` 的继承解析（`exclude` 对 path dep 无效）。因此 ONE 主仓库作为单 crate，把各子模块以 `path = "..."` 拉入；`vendor/zed` 保留自己的 workspace。

---

## License

MIT
