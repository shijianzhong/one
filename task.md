# Solo3 GPUI 迭代计划：超级电脑智能终端 (v2)

## 产品定位
ONE 不只是一个多智能体助手，更要做"小白也能玩转电脑"的智能终端：
- 极客模式：AI 问答 / 编码 / 文档处理
- 小白模式：清理系统 / 整理桌面 / 卸载软件
- 远程模式：Telegram 等 IM 反向操控本机

## 架构总图
```
Trigger Layer (GPUI Chat | Telegram Bot | Hotkey)
           │
           ▼
       MainAgent (人格 / 决策 / 流式)
           │
   ┌───────┼───────┐
   ▼       ▼       ▼
Coding   System   Knowledge
Lane     Lane     Lane
   │       │       │
ClaudeCode Skill   Memory
(ACP)     Registry (3 layers)
           │
           ▼
   PermissionPolicy + ApprovalUI + AuditLog
```

## 当前进度（截至 2026-06-04）
- [x] MainAgent + Orchestrator 主对话内核已上线。
- [x] IntentRouter 收敛为快速路径，复杂请求落 Orchestrator。
- [x] coordinator.rs / general_agent.rs / agent_jobs.rs 已移除。
- [x] Claude Code `--dangerously-skip-permissions` 仅 Bypass 启用。
- [x] **PermissionPolicy 异步审批通道**：`request_async` + 全局 `ApprovalQueue` + `drain_next` + GPUI 弹窗 (`render_approval_dialog`)，Default 模式下 Shell/File/Process 触发用户确认；Strict 模式拒绝 Shell。
- [x] **ShellTool / ProcessListTool / FileListTool 切换到 `request_async`**，统一走授权路径。
- [x] JobManager 轻量解耦：`toggle_subagent_collapsed/toggle_subagent_events_collapsed` 物理迁入 `impl JobManager`；新增 `allocate_claude_run_id / allocate_general_ai_run_id / allocate_summarize_job_id / set_request / clear_request / clear_request_full / reset_general_ai_run` helper，AppState 不再直接读写 `next_*_id / request_in_flight / request_status_text / request_kind / general_ai_*` 字段。
- [~] Task Event Log 表结构存在，未统一收敛到 `RunRecorder`。

## M1 · 内核收敛（剩余项）
- [x] **JobManager 真解耦（轻量版）**：操作 JobManager 自身状态的方法（toggle_*）物理迁回 `impl JobManager`；spawn/apply 类方法保留在 AppState（因仍需读 `db.conn / current_lang / active_task_id / workspaces` 等），但所有字段直写收敛为 `set_request / clear_request / clear_request_full / reset_general_ai_run / allocate_*_id` helper 调用，外部模块（routing.rs / ui/subagent.rs）也已迁到同一 API。重型迁移（spawn 全部下沉至 JobManager + JobContext 借用结构）记入 M5 备选项。
- [x] **统一 RunRecorder**：新增 `RunRecorder::begin / attach`，`job_manager.rs` 里 7 处 `task_db::append_run_event` + `finish_task_run` 全部收敛走 RunRecorder；`task_db::insert_task_run / append_run_event / finish_task_run` 降为 `pub(crate)` 防止外部回流；删除孤儿 `RunEventRow` + `load_run_events`。
- [x] **memory workspace 修复**：`MainAgent::with_workspace` + `RememberTool/RecallTool` 持有 workspace；`AgentFactory::create_orchestrator` 把 `workspace_name` 真实贯穿 MainAgent / MemoryAgent，消灭 `save_fact(".", ...)` 字面量目录。`memory::storage::get_memory_base_path` 已落到 `dirs::data_dir()/one/memory`，远控所需的 app data dir 路径自动生效。
- [ ] **MemoryStore trait**（M4 预留）：把 `profile.rs` / `search.rs` / `snapshot.rs` 抽到统一接口，方便切 sqlite-vss / 远端实现。
- [x] **`update_soul` 降级**：MainAgent 工具改名 `propose_soul_update`，仅向 `agents::soul` 全局队列写入 `SoulProposal`（含 rationale + 旧/新内容）；AppState 接管草案 pump，新增 GPUI 双栏审核弹窗（左旧右新 + 拒绝/应用按钮），仅在用户点击"应用草案"时才真正写 `soul.md`。`soul.md` 行为准则同步更新。

## M2 · Skill Registry（小白模式）
- [x] **定义 `Skill` trait**：`async fn preview / async fn execute` + `manifest()`，配套 `SkillManifest / SkillPreview / SkillExecution / SkillCategory` 数据类型，`SkillRegistry::registry()` 全局单例（`src/skills/mod.rs`）。
- [x] 首批 Skill：
    - [x] `system.cleaner`（缓存 / 回收站 / Xcode / Homebrew）—— preview 扫描尺寸，execute 走 `permission().request_async(File)`，无授权则 `denied:true` 返回。
    - [x] `desktop.organizer`（按文件类型分类）—— preview 把目标目录按图片/视频/文档… 8 类汇总，execute 经 PermissionPolicy 授权后 `std::fs::rename` 到子目录。
    - [x] `app.uninstaller`（主体 + ~/Library 残留）—— 无 args.app 时列候选；有 args.app 时定位 `.app` 主体并扫 `Application Support / Caches / Preferences / Logs / Saved Application State / Containers` 残留，execute 全部 `remove_dir_all/remove_file`。
    - [x] `doc.summarizer`（txt/md/log/csv/源码 抽取式摘要）—— 统计字符/行/词数 + 截首尾，execute 把摘要写到 `<file>.<ext>.summary.md`；PDF/DOCX 留给 M4 DocSkill。
    - [x] `media.dedup`（size + 首 64KB 哈希）—— 递归扫媒体目录，按尺寸分组再做样本哈希，输出冗余组与 keeper（`oldest|newest|shortest_path`），execute 删冗余副本。
- [x] Skill 卡片 UI（preview → confirm → result）：`SkillCardState`/`SkillCardStage` 状态机 + `launch_skill_card / approve_skill_card / cancel_skill_card` 三入口 + `render_skill_card_dialog` 模态弹窗（Previewing / PreviewReady / Executing / Done / Failed），chat header `skill` 按钮直接触发 `system.cleaner` 闭环验收。undo 暂留待后续。
- [x] `run_system_task` 工具改成"Skill Registry 检索 + 直调"：MainAgent 工具 schema 增加 `skill_id / apply / args / task` 四字段；Orchestrator 在 `skill_id` 命中时按 `apply` 走 `Skill::preview / execute`（execute 内部走 PermissionPolicy 二次确认），未命中时回落到 SystemAgent 通用路径，输出统一为带 `stage`/`summary`/`items`/`warnings` 的 JSON 让 LLM 拼下一句话。

## M3 · 远程触达
- [x] `triggers/` crate + `Trigger` trait。
- [x] `triggers/telegram`（reqwest long-poll + chat_id 白名单 `ONE_TELEGRAM_ALLOWED_CHATS`）。
- [x] 远程触发自动锁 PermissionMode = Strict；危险操作双确认。
- [x] `/audit` 远程拉取最近 N 条 RunEvent。
- [x] 远程暗号确认机制（DangerLevel + RemoteAuth + PendingConfirmation 状态机 + Extreme 双确认）。
- [x] Telegram 绑定引导 UI（GPUI 设置页 Token 输入 + getMe 验证 + 绑定码）。
- [x] 远程 Workspace 切换（`/workspace` + `/workspaces`）。
- [x] 远程 Task 生命周期（自动创建 + Step 追加 + `/status`/`/tasks`/`/clear`）。

## M4 · 内容侧（挂起）
- [ ] DocSkill 接 PDF/DOCX 解析 + 分块 + L3 嵌入（待需要时开发）
- [ ] MemoryStore 切 sqlite-vss / Qdrant 实现（低优先级）
- [ ] Skill Marketplace 接 Skill Registry，支持热装载（低优先级）
- [ ] 多模型路由（按任务类型挑模型）（已暂停，当前单模型够用）

## 当前优先级
1. **M1 已闭环**：安全侧（Permission + Soul）+ 审计侧（RunRecorder）+ 状态侧（JobManager 字段收敛）+ 数据侧（memory workspace）全部到位。
2. **M2 已闭环**：Skill trait、5 个首批 Skill（system.cleaner / desktop.organizer / app.uninstaller / doc.summarizer / media.dedup）、Skill 卡片 UI、`run_system_task` SkillRegistry 直调全部就绪；GPUI 卡片与 LLM 工具都跑同一套 `Skill::preview/execute` + PermissionPolicy。
3. **M3 已闭环**：Telegram 远程触发 + `RemoteScopeGuard` 自动 Strict + 暗号确认（DangerLevel/RemoteAuth/PendingConfirmation/Extreme 双确认）+ 绑定引导 UI + Workspace/Task 管理。`cargo build` 通过，`cargo test` 40/40 通过。
4. **M4 内容侧**：所有 4 项挂起，待后期需要时开发。

## 进度记录
- **2026-06-03**：MainAgent 上线。
- **2026-06-04**：JobManager 接线 + Claude 命令预览修正。
- **2026-06-04 PM**：基于产品定位重排里程碑（M1-M4），落地 PermissionPolicy 异步审批 + GPUI 确认弹窗（M1 第一项）。
- **2026-06-04 EOD**：Memory workspace 真贯通——MainAgent/MemoryAgent/Factory 全部接收真实 workspace 名，长期事实落到 `<data_dir>/one/memory/<workspace>/profile.json`，不再写到字面量 `.` 目录（M1 第二项收尾，MemoryStore 抽象延后到 M4）。
- **2026-06-04 Late**：RunRecorder 收口——`job_manager.rs` 全部 audit-write 走 `RunRecorder::{begin,attach,record,finish}`，task_db 写接口降到 `pub(crate)`，孤儿 `RunEventRow / load_run_events` 删除。`cargo test` 9/9 通过（M1 第三项完成）。
- **2026-06-04 Night**：`update_soul` 降级为 `propose_soul_update`——新增 `agents/soul.rs` 草案队列（rationale + 旧/新内容快照）、AppState 复用同一 pump、GPUI 双栏审核弹窗，soul.md 只能在用户点"应用草案"时被写入。`cargo test` 11/11 通过（M1 第四项完成，M1 安全侧闭环）。
- **2026-06-04 Closing**：JobManager 轻量解耦完成——toggle_subagent_* 物理迁回 `impl JobManager`；新增 `allocate_*_id / set_request / clear_request[_full] / reset_general_ai_run` 一组 helper，`job_manager.rs` + `routing.rs` + `ui/subagent.rs` 共 12 处字段直写改成 helper 调用，AppState 不再绕过 JobManager 改它的内部状态。`cargo check` 干净，`cargo test` 11/11 通过（M1 第五项完成，M1 全部收口）。
- **2026-06-04 M2-1**：M2 起步——新增 `src/skills/` 模块（`Skill` trait + `SkillManifest / SkillPreview / SkillExecution / SkillCategory` + `SkillRegistry` 全局单例），首个 Skill `system.cleaner` 落地：扫描 `~/Library/Caches / DerivedData / iOS DeviceSupport / Homebrew Caches / .Trash`，preview 不写盘只统计大小并附 warning，execute 经 `permission().request_async(ToolKind::File)` 二次确认后再逐项 `remove_dir_all/remove_file`，拒绝时返回 `denied:true`。`cargo test` 16/16 通过（含 5 个新增 Skill 测试）。
- **2026-06-04 M2-2**：Skill 卡片 UI 闭环——AppState 增加 `skill_card: Option<SkillCardState>` 与 `SkillCardStage`（Previewing / PreviewReady / Executing / Done / Failed）状态机，`launch_skill_card / approve_skill_card / cancel_skill_card` 三入口分别 `cx.spawn` 调用 `Skill::preview / execute`；`render_skill_card_dialog` 渲染 520px 模态（汇总 + 滚动条目 + 释放空间 + warning + 失败/成功列表），仅 `PreviewReady` 显示"应用"按钮；`ui/mod.rs` 在 soul-proposal 之后挂上新 dialog；`render_chat_header` 新增 `skill` 按钮（action=`skill-cleaner`）一键调起 `system.cleaner` 全链路。`cargo check` 干净，`cargo test` 16/16 通过。
- **2026-06-04 M2-3**：`run_system_task` 接入 SkillRegistry——MainAgent 工具 schema 升级为 `{ skill_id, apply, args, task }`；Orchestrator 拦截分支先 `crate::skills::registry().find(skill_id)`，命中则按 `apply=false/true` 调用 `Skill::preview / execute`，输出 `{stage, summary, estimated_bytes, items, warnings, hint}` 或 `{stage:"execute", denied, freed_bytes, success, failed}`；未命中或 `skill_id` 缺省时回落到原 `SystemAgent` 路径。LLM 现在可以"先 preview 再请求授权"，与 GPUI Skill 卡片走同一套 PermissionPolicy。`cargo test` 16/16 通过。
- **2026-06-04 M2-4**：M2 闭环——首批 4 个 Skill 全部落地：`desktop.organizer`（顶层文件按 8 类 mv 到子目录）/ `app.uninstaller`（`.app` 主体 + 6 类 ~/Library 残留）/ `doc.summarizer`（文本类抽取式摘要 + 写 `<name>.<ext>.summary.md`）/ `media.dedup`（size+首 64KB 哈希分组 + keeper 策略）。全部走 `permission().request_async(File)` 二次确认，preview 只读、execute 才写盘。`SkillRegistry::new` 注册 5 个 Skill，`RunSystemTaskTool` 描述同步刷新让 LLM 看到完整 skill_id 列表。`cargo check` 干净，`cargo test` 25/25 通过（含 9 个新增 Skill 测试）。M2 全部收口。
- **2026-06-04 M3-1**：M3 远程触达骨架——（略，见上）。M3 第一刀闭环，剩"远程来源自动锁 Strict"留给 M3-2。
- **2026-06-04 M3-2**：M3 闭环——`RemoteScopeGuard` RAII guard 落地（`permission.rs`）：thread-local `Cell<bool>` 标记当前线程是否处于远程来源，`enter()` 置位、Drop 自动还原、支持嵌套；`request_async` 在 `source=None` 但 `RemoteScopeGuard::is_active()` 时自动升级为 `source="remote"` 走 Strict 分支；`dispatcher::run_skill` 在调 `Skill::execute` 前 `let _guard = RemoteScopeGuard::enter()`，后续所有 `permission().request_async` 调用自动收紧为 Strict（Shell 直接 Deny、File/Process 必须本机点 Allow）。新增 3 个测试：guard enter/drop、嵌套、auto-strict 行为验证。M3 全部收口。
