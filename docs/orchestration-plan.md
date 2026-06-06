# MainAgent 与 Claude Code 联动编排及多项目支持方案（修订版）

> 生成时间：2026-06-06  
> 版本：v2（基于代码审计修订）  
> 目标：解决路径硬编码、支持一工作区多项目、实现记忆透传、增强改动感知。

---

## 一、现状评估：哪些已实现，哪些需要做

下表将原方案提案与当前代码实际情况进行对比，以确定真正的实施范围。

| # | 原方案提案 | 实际状态 | 关键代码位置 |
|:--|:---|---:|:---|
| 1 | 区分 workspace_root / storage_dir / work_dir | ✅ **基础设施已就绪**。`AppState.default_work_dir`、`get_work_dir()`、`Workspace.path` 均已存在 | `app_state.rs:46,409-419`、`workspace.rs:43-54` |
| 2 | Memory 注入（profile facts → agent context） | ✅ **已完全实现**。global + workspace facts 在 `run_task` 时注入 system prompt，L3 TF-IDF 检索也已集成 | `orchestrator.rs:53-73`、`memory/profile.rs:93-108`、`memory/snapshot.rs:10-79` |
| 3 | Orchestrator 分发任务到 sub-agent | ✅ **已完全实现**。`run_sub_agent` 分发到 coding / system agent | `orchestrator.rs:322-418`、`factory.rs` |
| 4 | MainAgent 的 remember / recall 工具 | ✅ **已完全实现**。支持 global/workspace/both 三层作用域 | `main_agent.rs:148-211` |
| 5 | 任务结束后生成 memory snapshot | ✅ **已实现 L2 快照生成**，但 L3 chunk 从未被写入 | `job_manager.rs:1067-1086`、`storage.rs:66-89`（未调用） |
| 6 | 路径纠偏：Orchestrator 内 hardcoded `"."` | ❌ **存在严重 BUG**。`orchestrator.rs:345` 硬编码 `"."`，与 `job_manager.rs:221-226` 的 task-specific 目录策略不一致 | `orchestrator.rs:345` vs `job_manager.rs:221-226` |
| 7 | Claude Code 输出解析 `modified_files` | ❌ **未实现**。`parse_stream_line` 不提取文件变更信息；`ClaudeStreamEvent::Finished` 仅有 `result: String` | `claude_code.rs:12-36,83-192` |
| 8 | Session ID 隔离 | ⚠️ **部分实现**。`job_manager.rs` 有 session_id 通道但传 `None`；orchestrator 分支也传 `None` | `job_manager.rs:199,217,241`、`orchestrator.rs:346` |
| 9 | 多项目目录切换工具 `UpdateWorkDirTool` | ❌ **未实现**。MainAgent 无此工具 | — |
| 10 | L3 chunk 数据写入 | ❌ **代码存在但未集成**。`save_task_memory_async` 和 `upsert_task_chunks` 已定义但从未被调用 | `storage.rs:66-89`、`search.rs:33` |

---

## 二、核心架构（修订）

### 2.1 三大路径概念（保留，更新引用）

| 路径类型 | 变量名 | 说明 | 现有实现 |
|:---|---:|:---|---:|
| **Workspace Root** | `workspace_path` | 整个工作区的物理根目录，用户添加工作区时选定 | `Workspace.path`（`workspace.rs`） |
| **Storage Dir** | `storage_path` | 任务元数据存储目录（快照、session） | `get_task_dir_for_ids()`（`workspace.rs:43`）|
| **Working Dir** | `work_dir` | **代码执行目录**。默认等于 Workspace Root 以保持全局视野，支持动态切换至子项目以适配构建工具。 | `AppState.default_work_dir` + `get_work_dir()`（`app_state.rs:46,409`）|

### 2.2 混合路径策略（Hybrid Strategy）

本方案不强制单一目录，而是采用“广度优先，深度按需”的原则：
1. **默认启动**：Orchestrator 默认在 `Workspace Root` 启动 Claude Code。这保证了 AI 能处理跨项目的重构和全局搜索。
2. **环境适配**：当任务涉及具体的构建或测试命令（如 `cargo test`, `npm run build`）时，由于这些工具依赖特定的 CWD，LLM 可通过工具切换至子项目目录。
3. **记忆对齐**：切换后的目录状态将反馈至后续的子任务中，确保上下文连贯。

### 2.3 代理协作链路（更新）

```
1. 用户 → AppState.messages
2. AppState  → JobManager.spawn_orchestrator_run()
       ↓
3. Orchestrator.run_task(task, session_id, history, workspace, task_id, work_dir ← 新增)
       ↓ 注入 memory / L3 context → 主循环
4. MainAgent.step_stream() → ToolCalls 或 Answer
       ↓
5. run_sub_agent("coding", task, session_id, work_dir ← 新增)
       ↓ 启动 Claude Code（传入 work_dir）
6. Claude Code → 实时 JSON stream（含 modified_files ← 新增）
       ↓
7. ClaudeStreamEvent::ModifiedFiles / Finished → UI 展示
8. 对话结束 → save_task_memory_async(L2+L3) ← 修复缺失调用
```

---

## 三、关键 Bug：`orchestrator.rs:345` 硬编码 `"."`

这是本方案中 **优先级最高的问题**。

### 问题描述

```rust
// orchestrator.rs:340-354
if agent_id == "coding" {
    let project_dir = std::path::PathBuf::from(".");  // ← BUG: 硬编码 `.`
    // ...
    crate::agents::claude_code::ClaudeCodeAgent::execute_instruction_stream(
        &project_dir,  // ← 始终使用当前进程目录
        &task_owned,
        None,  // ← session_id 也传 None
        tx,
    )
}
```

与此对比，直接路径的 `spawn_claude_code_run`（`job_manager.rs:221-226`）：

```rust
let project_dir =
    if let Some((workspace_id, task_id, title)) = self.get_active_task_location() {
        self.ensure_task_storage_dir(workspace_id, task_id, &title)  // 正确的 task 目录
    } else {
        std::path::PathBuf::from(self.get_work_dir())
    };
```

**后果**：
- Orchestrator 启动的 Claude Code 子进程在错误目录下操作文件
- 用户可能修改了 task_dir 中的文件而不是源代码目录
- 与直接启动 Claude Code 的行为不一致，造成混淆

### 解决方案

1. 在 `Orchestrator` 结构体中增加 `work_dir: PathBuf` 字段
2. 从 `AgentFactory::create_orchestrator` 传入
3. 从 `job_manager.rs` 的 `spawn_orchestrator_run` 获取 `self.get_work_dir()` 并透传
4. `run_sub_agent("coding")` 分支使用 `self.work_dir` 替代 `"."`

---

## 四、缺失功能

### 4.1 Claude Stream 缺少 `modified_files` 解析

**现状**：`ClaudeStreamEvent` 枚举（`claude_code.rs:12-36`）不包含文件变更信息。`parse_stream_line`（第83行）仅处理 `assistant`、`ask_user_question`、`result`、`system` 等类型。

**需要新增**：
- `ClaudeStreamEvent::ModifiedFiles { files: Vec<String> }` 变体
- `parse_stream_line` 的新分支：匹配 Claude Code JSON 输出中文件变更字段（字段名需先实证确认，可能是 `modified_files` 或 `changed_files`）
- `SubagentEventEntry` 或 `ClaudeRunEvent` 中增加文件变更的 UI 展示

**前置调研**：运行 `claude -p "echo hi" --output-format stream-json --verbose 2>/dev/null | head -10`，查看实际输出的 JSON 结构中修改文件列表的字段路径。

### 4.2 L3 Chunk 从未被写入

**现状**：`search.rs:33` 的 `upsert_task_chunks` 和 `storage.rs:66` 的 `save_task_memory_async` 均已实现，但从未被调用。这意味着 `snapshot.rs:53` 的跨任务 TF-IDF 搜索永远返回空结果。

**修复**：在 `job_manager.rs` 的 orchestrator finish 回调中（第1067-1086行），在生成 L2 snapshot 后调用 `save_task_memory_async`，传入完整 messages。

### 4.3 MainAgent 缺少目录切换工具

**现状**：无 `UpdateWorkDirTool`。MainAgent 无法在对话中动态切换工作目录。

**新增**：
- `UpdateWorkDirTool`，接受 `path: String` 参数
- 将新目录存回 `AppState` 的 `default_work_dir` 或当前 task 的临时状态
- 后续 Claude Code 调用将使用新目录

---

## 五、实施步骤

### Phase 0：修复路径硬编码与目录透传（1 天）

**目标**：消除 `orchestrator.rs:345` 的 `"."`，使 Orchestrator 的 coding agent 使用与直接路径一致的 working directory。

**步骤**：

1. **Step A（0.5 天）**：`Orchestrator` 结构体增加 `work_dir: PathBuf` 字段
   - `orchestrator.rs:23-27` 增加字段
   - `Orchestrator::new()` 增加参数
   - 修改所有调用 `Orchestrator::new()` 的地方（主要是 `factory.rs:38`）

2. **Step B（0.5 天）**：修改调用链
   - `AgentFactory::create_orchestrator()` 接受 `work_dir: PathBuf` 参数
   - `job_manager.rs` 的 `spawn_orchestrator_run` 传入 `self.get_work_dir()`
   - `orchestrator.rs:345` 改为 `self.work_dir.clone()`

3. **Step C（可选，0.25 天）**：`orchestrator.rs` 的 coding agent 分支传入 session_id
   - 将 `context.session_id` 透传到 `execute_instruction_stream` 的 session_id 参数
   - 替换当前的 `None`

### Phase 1：Memory 审计与 L3 修复（0.5 天）

**目标**：验证已有的 memory 注入逻辑正确运行，修复 L3 chunk 从未写入的问题。

**步骤**：

1. **Step A（0.25 天）**：验证现有 memory 注入
   - 确认 `orchestrator.rs:54-73` 实际工作
   - 确认 `memory/profile.rs` 的 global/workspace 事实存储正常
   - 可通过添加简短 eprintln 日志或测试用例验证

2. **Step B（0.25 天）**：修复 L3 chunk 写入
   - 在 `job_manager.rs:1080`（snapshot 生成完成后）添加：
     ```rust
     let _ = crate::memory::storage::save_task_memory_async(
         workspace_name, task_id, task_title, messages,
     );
     ```
   - 移除 `storage.rs` 顶部的 `#![allow(dead_code)]` 或改为更精确的 `#[allow(dead_code)]`

### Phase 2：文件改动反馈（1 - 2 天）

**依赖**：Phase 0 完成

**目标**：Claude Code 执行后，UI 能展示修改了哪些文件。

**步骤**：

1. **Step A（0.5 天）**：前置调研
   - 运行 `claude -p "echo hi" --output-format stream-json --verbose 2>/dev/null`
   - 确认实际输出 JSON 中包含文件修改信息的字段路径（可能是 `result.changed_files` 或顶层 `modified_files`）
   - 记录字段名和嵌套结构

2. **Step B（0.25 天）**：扩展 `ClaudeStreamEvent`
   - `claude_code.rs:12-36` 增加 `ModifiedFiles { files: Vec<String> }` 变体
   - 如果 Claude Code CLI 返回的是嵌套结构（如 `result.modified_files`），需要对应调整

3. **Step C（0.25 天）**：扩展 `parse_stream_line`
   - 在 `claude_code.rs:108-192` 的 match 分支中增加新类型处理
   - 从 JSON 提取文件名列表并生成 `ModifiedFiles` 事件

4. **Step D（0.25 天）**：事件消费
   - `job_manager.rs:362-400` 的 Claude stream 处理增加 `ModifiedFiles` 分支
   - 将文件列表存入 `ClaudeRunPanelState`（需增加字段）
   - 在 UI（`ui/subagent.rs` 或 `ui/chat.rs`）展示文件列表

### Phase 3：多项目目录切换与环境优化（0.5 天）

**目标**：赋予 MainAgent 动态调整执行环境的能力，以应对复杂的构建工具依赖。

**步骤**：

1. **Step A（0.25 天）**：新增 `UpdateWorkDirTool`
   - `main_agent.rs` 中增加新的 Tool 实现
   - 参数：`path: String`（目标子项目目录，支持相对于 Workspace Root 的路径）
   - 执行逻辑：将新目录更新至 `AppState.default_work_dir`。
   - **设计细节**：增加路径存在性校验，防止 LLM 进入不存在的目录。

2. **Step B（0.25 天）**：更新 MainAgent system prompt
   - 明确告知 LLM：默认在根目录操作以保持全局视野。
   - 引导 LLM：当需要运行 `cargo`, `npm`, `go` 等命令且在当前目录无法找到配置文件时，**必须**使用 `UpdateWorkDirTool` 切换至对应的子项目目录。
   - 示例场景：
     - "修复全局 Bug" → 保持在根目录。
     - "运行后端测试" → 切换至 `server/` 目录 → 执行 Claude Code。

### Phase 4：Session 隔离（0.25 天）

**目标**：确保不同任务的编辑状态物理隔离。

**步骤**：

1. `job_manager.rs` 中 `spawn_orchestrator_run` 生成有意义的 session_id（当前已部分实现 `format!("orchestrator-{}", run_id)`）
2. 将该 session_id 透传到 `orchestrator.rs` 的 `run_task`
3. 在 `orchestrator.rs` 的 `run_sub_agent("coding")` 中将 session_id 传给 `execute_instruction_stream`

---

## 六、文件改动清单（修订版）

### 6.1 `src/agents/core/orchestrator.rs`
- **结构体变更**：`Orchestrator` 增加 `work_dir: PathBuf` 字段（替代 `"."` 硬编码）
- **接口变更**：`Orchestrator::new()` 增加 `work_dir` 参数
- **逻辑变更**：`run_task` 的 coding agent 分支使用 `self.work_dir`
- **session 透传**：coding agent 分支传入 `context.session_id` 替代 `None`

### 6.2 `src/agents/core/factory.rs`
- **接口变更**：`create_orchestrator()` 增加 `work_dir: PathBuf` 参数

### 6.3 `src/runtime/job_manager.rs`
- **逻辑变更**：`spawn_orchestrator_run` 传入 `self.get_work_dir()`
- **逻辑变更**：在 orchestrator finish 回调中添加 `save_task_memory_async` 调用（L3 chunk 写入）
- **逻辑新增**：处理 `ClaudeStreamEvent::ModifiedFiles` 事件（Phase 2 完成后）

### 6.4 `src/agents/claude_code.rs`
- **枚举扩展**：`ClaudeStreamEvent` 增加 `ModifiedFiles { files: Vec<String> }`
- **解析扩展**：`parse_stream_line` 增加文件变更信息提取分支（需先确认字段名）

### 6.5 `src/agents/core/main_agent.rs`
- **新增工具**：`UpdateWorkDirTool`，允许 LLM 切换工作目录
- **System prompt 更新**：告知 LLM 有目录切换能力

### 6.6 `src/agents/types.rs`（可选）
- **结构体扩展**：`ClaudeRunPanelState` 增加 `modified_files: Vec<String>` 字段（Phase 2）

---

## 七、风险评估（修订版）

| 阶段 | 风险等级 | 说明 |
|---|---|---|
| **Phase 0** 修复 `"."` 硬编码 | **中** | 涉及 `Orchestrator` → `Factory` → `JobManager` 调用链，`work_dir` 需多层透传。改动集中在 `orchestrator.rs`、`factory.rs`、`job_manager.rs`，改动面可控 |
| **Phase 1** Memory 审计 + L3 修复 | **低** | L3 写入是已存在函数的调用点添加，不涉及新逻辑。Memory 审计仅验证无需改代码 |
| **Phase 2** 文件改动反馈 | **中** | **前置依赖：需先确认 Claude Code CLI 实际输出的 JSON 结构**。若字段名或嵌套结构与预期不符，需要迭代。`ClaudeStreamEvent` 枚举变更是向后兼容的 |
| **Phase 3** 多项目切换 | **低** | 新增工具，不涉及现有逻辑修改 |
| **Phase 4** Session 隔离 | **低** | 仅透传已有的 session_id，改动量小 |

### 新增风险点

- **Orchestrator 与直接路径的目录不一致**（Phase 0 修复）：当前用户可能已依赖 Orchestrator 在当前目录的行为，修改后部分工作流可能受影响
- **Claude Code CLI 输出的不确定性**（Phase 2）：需要实测确认 JSON schema，无法在开发环境外预测

---

## 八、Session ID 隔离说明（保留，更新）

原方案的"将 `--session-id` 指向文件路径"描述与 Claude Code CLI 实际行为不一致。`--session-id` 接受的是标识符字符串，不是文件路径。

**当前实现**：
- `job_manager.rs` 直接路径：`session_id: None`
- `orchestrator.rs` coding agent 路径：`session_id: None`
- `job_manager.rs:1033` 的 orchestrator session：`format!("orchestrator-{}", run_id)`

**Phase 4 目标**：
- 将 `orchestrator-{run_id}` 从只存在于 orchestrator 层，透传到 coding agent 的 `execute_instruction_stream` 调用中
- Claude Code CLI 用此 session_id 保持编辑状态在同一个 task 中连续

---

## 九、后续展望（保留）

1. **自动目录推荐**：通过分析 `Cargo.toml` / `package.json` / `go.mod` 向用户推荐子项目
2. **跨项目依赖分析**：子项目 A 被修改时，提醒可能影响子项目 B
3. **文件变更 UI 增强**：修改文件列表支持点击打开、diff 查看、一键回退
4. **L3 语义搜索增强**：TF-IDF 可升级为 embedding 检索（对接向量数据库/LLM embedding API）