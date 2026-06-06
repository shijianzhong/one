# MainAgent 与 Claude Code 联动编排及多项目支持方案

> 生成时间：2026-06-06
> 版本：v1
> 目标：解决路径硬编码、支持一工作区多项目、实现记忆透传、增强改动感知。

---

## 一、 核心架构重构

### 1.1 明确三大路径概念
为了支持一个 Workspace 下存在多个工程项目，我们需要明确区分以下路径：

| 路径类型 | 变量名 | 说明 | 示例 |
| :--- | :--- | :--- | :--- |
| **Workspace Root** | `workspace_path` | 整个工作区的物理根目录，用户添加工作区时选定的路径。 | `/Users/dev/my_monorepo` |
| **Storage Dir** | `storage_path` | 任务元数据存储目录（快照、日志、session）。位于 workspace 下的 `tasks/`。 | `workspace_path/tasks/<task_id>/` |
| **Working Dir** | `work_dir` | **真实的代码操作执行目录**。默认等于 Workspace Root，但可被任务动态修改。 | `workspace_path/apps/web-server/` |

### 1.2 代理协作链路
1.  **用户**：发起任务（Task）。
2.  **MainAgent**：分析意图，确定执行目录 `work_dir`。
3.  **Orchestrator**：
    *   从 `Memory System` 获取当前 Workspace 事实。
    *   将事实注入 `Claude Code` 指令。
    *   在指定的 `work_dir` 下启动 `Claude Code`。
4.  **Claude Code**：执行文件操作。
5.  **Feedback**：返回执行摘要 + 修改的文件列表给 `MainAgent`。

---

## 二、 关键功能实现

### 2.1 任务感知的工作目录 (Working Dir)
在 `MainAgent` 中增加 `set_working_directory` 工具，允许其根据对话上下文动态调整执行路径。

*   **逻辑**：
    *   启动任务时，默认 `work_dir = workspace_root`。
    *   如果用户说“处理 `client/` 目录”，MainAgent 发现该路径存在，则调用工具更新当前任务的 `work_dir`。
    *   后续所有子代理（Claude Code, Shell）均在该目录下执行。

### 2.2 记忆透传 (Memory Passthrough)
打破大脑（MainAgent）与执行手（Claude Code）的信息鸿沟。

*   **注入逻辑**：
    *   Orchestrator 在拦截 `run_claude_code` 调用时，从 `profile.rs` 读取该 Workspace 的关键事实（编码规范、技术栈偏好）。
    *   将这些事实以 `[Context: Project Guidelines]` 的形式拼接在给 Claude Code 的 `instruction` 头部。
    *   **示例注入**：`"instruction": "[Context: 本项目强制使用 Rust anyhow 处理错误]\n 任务：实现 main.rs 中的错误处理。"`

### 2.3 状态与 Session 隔离
利用 Claude Code CLI 的 `--session-id` 参数，将不同任务的编辑状态物理隔离。

*   **方案**：
    *   将 `--session-id` 指向 `Storage Dir` 下的特定文件。
    *   这样即使用户在同一个 Workspace 切换不同 Task，各自的撤销历史、临时状态都不会互相干扰。

---

## 三、 文件改动清单

### 3.1 `src/agents/core/orchestrator.rs`
*   **修改方案**：`run_task` 接口新增 `work_dir: PathBuf` 参数。
*   **修改方案**：在 `run_sub_agent` 逻辑中，实现记忆注入逻辑。
*   **消除硬编码**：将 `project_dir` 从 `.` 改为传入的 `work_dir`。

### 3.2 `src/agents/core/main_agent.rs`
*   **新增工具**：`UpdateWorkDirTool`。允许 LLM 发现子项目并切换路径。
*   **逻辑更新**：在 `remember` 时增加对“项目结构”的识别。

### 3.3 `src/runtime/job_manager.rs`
*   **逻辑更新**：在 `spawn_orchestrator_run` 时计算初始 `work_dir`。
*   **状态同步**：确保从 UI 选中的“当前目录”能正确初始化给 Orchestrator。

### 3.4 `src/agents/claude_code.rs`
*   **功能增强**：解析 Claude Code 的 JSON 输出，提取 `modified_files` 字段，并将其包含在 `Finished` 事件中。

---

## 四、 实施步骤

### Phase 1: 路径纠偏 (1 天，拆分两步)
1.  **Step A (0.5天)**：在 `Orchestrator` 结构体中增加 `work_dir: PathBuf` 字段，从 `AgentFactory::create_orchestrator` 传入并存储。
2.  **Step B (0.5天)**：修改 `run_sub_agent` 的 coding agent 分支，使用 `self.work_dir` 而非硬编码 `"."`；同步修复 `job_manager` 中的路径计算逻辑，确保 Claude Code 运行在代码根目录而非空任务目录。

> ⚠️ **注意**：`work_dir` 需要从 `spawn_orchestrator_run` 一路透传到 `execute_instruction_stream`，改动涉及 `Orchestrator` 结构设计、`run_task`/`run_sub_agent` 调用链。建议将 `work_dir` 存入 `Orchestrator` 实例字段，而非每个方法透传参数。

### Phase 2: 记忆穿透 (0.5 天)
1.  在 `Orchestrator` 中接入 `memory::profile::get_all_facts`。
2.  在 `run_sub_agent` 的 coding agent 分支中，将 profile facts 拼接在 `instruction` 头部：
   ```rust
   let instruction_with_context = format!(
       "[Context: Project Guidelines]\n{}\n\n[Task]\n{}",
       profile_facts.join("\n"),
       task
   );
   ```

### Phase 3: 多项目支持工具 (0.5 天)
1.  为 `MainAgent` 增加目录切换工具 `UpdateWorkDirTool`。
2.  更新 System Prompt，告知其可以根据需求切换到子项目目录。

### Phase 4: 感知反馈升级 (0.5 天)
1.  升级 `ClaudeStreamEvent::Finished`，携带改动文件列表 `modified_files: Vec<String>`。
2.  `MainAgent` 接收到反馈后，将其自动存入 `ChatMessage` 供后续参考。
3.  **注意**：需先验证 Claude Code CLI 实际输出的 JSON 结构中 `modified_files` 字段的具体格式，再确定提取逻辑。当前 `parse_stream_line` 仅处理了 `type`、`message`、`session_id` 等字段。

---

## 五、 风险评估

| 阶段 | 风险等级 | 说明 |
|---|---|---|
| Phase 1 路径纠偏 | **中** | 涉及多层调用链改动，`work_dir` 透传路径较长 |
| Phase 2 记忆穿透 | 低 | 逻辑已存在，仅扩展注入点 |
| Phase 3 多项目工具 | 中 | MainAgent 需要正确判断何时切换目录 |
| Phase 4 改动反馈 | 低 | 仅扩展已有数据结构，需验证上游输出格式 |

---

## 六、 session_id 隔离说明

方案中"将 `--session-id` 指向 `Storage Dir` 下的特定文件"这一描述与 Claude Code CLI 实际行为不完全一致。Claude Code 的 `--session-id` 实际指向的是一个 session 标识符字符串，而非文件路径。

当前实现：
- `job_manager.rs:1033`：`let session_id = format!("orchestrator-{}", run_id);`
- `orchestrator.rs:346`：Claude Code 调用时 `session_id: None`

如需实现状态隔离，应依赖 session 自身的生命周期管理，而非文件路径指向。

---

## 七、 后续展望
1.  **自动目录推荐**：通过分析 `Cargo.toml` 或 `package.json` 自动向用户推荐子项目。
2.  **跨项目依赖分析**：当修改子项目 A 时，提醒用户可能会影响到子项目 B。
