# 记忆系统改造方案

> 生成时间：2026-06-06
> 版本：v2（基于完整代码审查重写）
> 相关代码：`src/memory/`、`src/agents/core/main_agent.rs`、`src/agents/core/orchestrator.rs`、`src/runtime/job_manager.rs`

---

## 一、现状盘点

### 1.1 已实现的能力（可直接用）

| 模块 | 文件 | 状态 | 说明 |
|------|------|------|------|
| Workspace 事实存储 | `memory/profile.rs` | ✅ 已接通 | `save_fact` / `get_all_facts`，按 workspace 隔离 |
| RememberTool | `agents/core/main_agent.rs` | ✅ 已接通 | MainAgent 工具，底层调 `memory::profile` |
| RecallTool | `agents/core/main_agent.rs` | ✅ 已接通 | MainAgent 工具，返回当前 workspace 全量事实 |
| Task 消息存储 | `memory/storage.rs` | ✅ 已接通 | `save_task_memory_async`，对话结束后写磁盘 |
| TF-IDF 检索 | `memory/search.rs` | ⚠️ 已实现，未接入 | `tfidf_search` 完整可用，从未被调用 |
| 对话快照生成 | `memory/snapshot.rs` | ⚠️ 已实现，未接入 | `generate_snapshot_sync` 用 LLM 蒸馏摘要，从未被调用 |
| L3 Chunks 索引 | `memory/search.rs` | ⚠️ 已实现，未接入 | `upsert_task_chunks` 在 `save_task_memory_async` 里调用，但 `tfidf_search` 没人用 |

### 1.2 已删除（本次清理）

- `MemoryAgent`（`agents/core/memory_agent.rs`）：功能完全被 `MainAgent` 内置的 `RememberTool`/`RecallTool` 覆盖，已清除
- `MemoryTool`（`agents/core/tools/mod.rs`）：只有 `MemoryAgent` 在用，随之删除

### 1.3 当前最大的问题

**记忆注入是被动的**：现在依赖 LLM 主动调 `recall` 工具，但 LLM 不是每次都会调——简单问候、日常对话基本不会触发 `recall`。结果就是：记忆存进去了，但 AI 大部分时间根本不知道这些记忆的存在。

**没有全局记忆**：用户在 workspace A 说"我叫小明"，切到 workspace B 后 AI 完全不认识这个人。

**L3 检索和快照能力闲置**：`tfidf_search`、`generate_snapshot_sync`、`build_memory_context` 都已实现，但从没被 `orchestrator.rs` 或 `job_manager.rs` 调用过。

---

## 二、目标

| 层级 | 范围 | 存储 | 示例 |
|------|------|------|------|
| Global（新增） | 所有 workspace 共享 | `memory/global/profile.json` | 用户姓名、语言偏好、常用工具 |
| Workspace（已有） | 同 workspace 跨 task | `memory/<workspace>/profile.json` | 项目技术栈、代码规范 |
| Task（已有） | 单次对话内 | 对话上下文 + L3 chunks | 对话历史 |

---

## 三、改造方案

### Phase 1：全局记忆层（存储 + 工具改造）

**改动量小，优先级最高。**

#### 3.1 `memory/storage.rs` — 新增全局路径函数

```rust
/// 全局记忆目录：<memory_base>/global/
pub fn get_global_memory_dir() -> PathBuf {
    get_memory_base_path().join("global")
}
```

#### 3.2 `memory/profile.rs` — 新增全局读写函数

```rust
/// 保存事实到全局记忆
pub fn save_global_fact(fact: &str) -> anyhow::Result<()> {
    // 复用 save_fact 逻辑，workspace_name = "global"
    let dir = get_global_memory_dir();
    fs::create_dir_all(&dir)?;
    // ...同 save_fact 逻辑
}

/// 获取全局记忆中所有事实
pub fn get_global_facts() -> Vec<String> {
    // 读 global/profile.json 的 key_facts
}
```

#### 3.3 `main_agent.rs` — `RememberTool` 加 `scope` 参数

```rust
// parameters_schema 新增 scope 字段
"scope": {
    "type": "string",
    "enum": ["global", "workspace", "both"],
    "description": "存储范围。global：跨 workspace 的个人信息（姓名、偏好）；workspace：仅限当前项目（技术栈、规范）；both：不确定时同时存。默认 both。"
}
```

`call()` 实现：
```rust
async fn call(&self, args: Value) -> Result<Value> {
    let fact = args["fact"].as_str().unwrap_or_default();
    let scope = args["scope"].as_str().unwrap_or("both");
    match scope {
        "global"    => save_global_fact(fact)?,
        "workspace" => save_fact(&self.workspace, fact)?,
        _           => { save_global_fact(fact)?; save_fact(&self.workspace, fact)?; }
    }
    Ok(json!({ "status": "success" }))
}
```

#### 3.4 `main_agent.rs` — `RecallTool` 合并全局 + workspace

```rust
async fn call(&self, _args: Value) -> Result<Value> {
    let mut facts = get_global_facts();            // 全局记忆
    facts.extend(get_all_facts(&self.workspace));  // workspace 记忆
    facts.dedup();                                 // 去重（同一事实可能同时存了两处）
    Ok(json!(facts))
}
```

#### 3.5 `main_agent.rs` — system_prompt 补充 scope 规则

在 system_prompt 的记忆指令部分替换为：

```
你有 remember 和 recall 两个记忆工具：
- 每次对话开始时先调 recall 查看已有信息，避免重复提问。
- 遇到关于用户个人的信息（姓名、偏好、职业、语言习惯）→ remember(scope="global")。
- 遇到关于当前项目/工作区的信息（技术栈、规范、路径、团队成员）→ remember(scope="workspace")。
- 不确定时 → remember(scope="both")，宁可多存不要漏存。
```

---

### Phase 2：主动记忆注入（解决"被动召回"问题）

**这是解决"AI 不记得用户"根本问题的关键。**

#### 3.6 `agents/core/orchestrator.rs` — `run_task` 开始时注入记忆

在 `run_task` 的 context 初始化之后、第一次 `step_stream` 之前，注入记忆：

```rust
pub async fn run_task<F>(
    &self,
    task: &str,
    session_id: String,
    history: Vec<ChatMessage>,
    workspace: &str,    // ← 新增参数
    mut on_event: F,
) -> Result<String> {
    let mut context = AgentContext::new(session_id);

    // ── 主动注入记忆（确定性，不依赖 LLM 主动召回）──────────────────────
    let mut all_facts = crate::memory::profile::get_global_facts();
    all_facts.extend(crate::memory::profile::get_all_facts(workspace));
    all_facts.dedup();

    if !all_facts.is_empty() {
        let memory_hint = format!(
            "[已知用户信息]\n{}",
            all_facts.iter().map(|f| format!("- {}", f)).collect::<Vec<_>>().join("\n")
        );
        // 作为第一条 system 补充消息注入
        context.add_message(ChatMessage::new("system", &memory_hint));
    }

    // 历史消息
    let msg_count = history.len();
    for msg in history.into_iter().take(msg_count.saturating_sub(1)) {
        context.add_message(msg);
    }
    context.add_message(ChatMessage::new("user", task));
    // ... 后续不变
```

`runtime/job_manager.rs` 的 `spawn_orchestrator_run` 调用处，传入 `workspace_name`：

```rust
orchestrator
    .run_task(&instruction_for_task, session_id, history, &workspace_name, |event| {
        ...
    })
    .await
```

---

### Phase 3：接入已有的 L3 检索和快照能力

**这部分代码已经写好，只需要接线。**

#### 3.7 对话结束后生成快照（`job_manager.rs`）

在 `OrchestratorWrapperEvent::Finished` 和 `GeneralAiStreamEvent::Finished` 处理里，任务完成后异步触发快照生成：

```rust
// Finished 处理末尾追加（不阻塞主流程）
if let Some(task_id) = this.active_task_id {
    let messages = this.messages.clone();
    let base_url = this.model_base_url.clone();
    let api_key  = this.model_api_key.clone();
    let model    = this.model_name.clone();
    let ws_name  = this.get_active_workspace()
        .map(|w| w.name.clone())
        .unwrap_or_else(|| "Default".to_string());

    std::thread::spawn(move || {
        crate::memory::snapshot::generate_snapshot_sync(
            &base_url, &api_key, &model,
            &messages, task_id, "task", &ws_name,
        );
    });
}
```

#### 3.8 `run_task` 注入 L3 相关 task 上下文

在 Phase 2 的注入逻辑后，再追加相关历史 task 的 TF-IDF 摘要：

```rust
// 追加 L3 相关 task 上下文（利用已实现的 tfidf_search）
let l3_context = crate::memory::snapshot::build_memory_context(workspace, 0, task);
if !l3_context.is_empty() {
    context.add_message(ChatMessage::new("system", &l3_context));
}
```

---

## 四、文件改动清单

| 文件 | 改动类型 | 说明 |
|------|---------|------|
| `src/memory/storage.rs` | 修改 | 新增 `get_global_memory_dir()` |
| `src/memory/profile.rs` | 修改 | 新增 `save_global_fact()` / `get_global_facts()`；新增单元测试 |
| `src/agents/core/main_agent.rs` | 修改 | `RememberTool` 加 scope 参数；`RecallTool` 合并全局+workspace；system_prompt 更新 |
| `src/agents/core/orchestrator.rs` | 修改 | `run_task` 加 `workspace` 参数；开始时注入全局+workspace 记忆；注入 L3 上下文 |
| `src/runtime/job_manager.rs` | 修改 | `spawn_orchestrator_run` 传 workspace_name；Finished 时异步触发 snapshot |

---

## 五、不变的部分

- `memory/profile.rs` 的 `save_fact` / `get_all_facts`：不变，workspace 隔离逻辑保留
- `memory/storage.rs` 的 `save_task_memory_async`：已在调用，保持不变
- `memory/search.rs` 的 `tfidf_search` / `upsert_task_chunks`：已实现，Phase 3 接入时直接用
- `memory/snapshot.rs` 的 `generate_snapshot_sync` / `build_memory_context`：已实现，Phase 3 接入时直接用
- `profile.json` 格式：`{ key_facts: [...], last_updated: ... }` 不变
- Task DB 结构：不变

---

## 六、去重策略说明

当前 `save_fact` 的去重是精确字符串匹配。已知限制：
- "用户叫小明" 和 "用户名字是小明" 会存两条
- Global 层会被多个 workspace 的对话不断追加，长期会有语义重复的事实堆积

**当前阶段**：维持精确匹配，在 system_prompt 里要求 LLM "用标准格式写事实（如'用户姓名：小明'）"，降低语义重复率。

**后续可扩展**：在 `save_fact` 里加包含关系检测（新事实是否是已有事实的子串或超集），或定期用 LLM 合并冗余事实。

---

## 七、实现步骤

### Phase 1（1 天）：全局记忆存储 + 工具改造
1. `storage.rs` 新增 `get_global_memory_dir()`
2. `profile.rs` 新增 `save_global_fact()` / `get_global_facts()` + 单元测试
3. `main_agent.rs` `RememberTool` 加 scope；`RecallTool` 合并全局；system_prompt 更新

### Phase 2（0.5 天）：主动记忆注入
4. `orchestrator.rs` `run_task` 加 workspace 参数 + 开始时注入全局+workspace 记忆
5. `job_manager.rs` `spawn_orchestrator_run` 传 workspace_name

### Phase 3（0.5 天）：接入已有的快照 + L3 检索
6. `job_manager.rs` Finished 时异步触发 `generate_snapshot_sync`
7. `orchestrator.rs` `run_task` 追加 `build_memory_context` L3 上下文注入

### 测试（0.5 天）
8. 单元测试：global/workspace facts 存取、dedup
9. 手动验收：跨 workspace 记住用户姓名；同 workspace 跨 task 记住项目规范

---

## 八、预计工时

| Phase | 内容 | 工时 |
|-------|------|------|
| Phase 1 | 全局记忆层 + 工具改造 | 1 天 |
| Phase 2 | 主动记忆注入 | 0.5 天 |
| Phase 3 | 接入快照 + L3 检索 | 0.5 天 |
| 测试 | | 0.5 天 |
| **总计** | | **约 2.5 人天** |

---

## 九、后续可扩展

1. **去重升级**：LLM 定期合并冗余事实（每 N 次对话触发一次）
2. **记忆重要性分级**：高频引用的事实排前面，低频/过期的沉底
3. **记忆清理 UI**：在 GPUI 设置页展示全局记忆，用户可手动删除或编辑
4. **跨 workspace 查询**：允许用户问"我在其他项目里是怎么配置 CI 的"，检索所有 workspace 的 L3 chunks
