# 记忆系统改造方案

> 生成时间：2026-06-06
> 版本：v3（基于代码审查与架构优化建议重写）
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

### 1.2 已发现的问题

1.  **被动召回（核心痛点）**：AI 只有主动调 `recall` 才能看到记忆。如果它不记得用户，就不会触发 `recall`。
2.  **缺乏全局记忆**：无法跨 Workspace 共享用户信息（如姓名、偏好）。
3.  **线程不安全**：`profile.rs` 缺乏锁保护，且非原子写入（先写一半可能崩溃导致 JSON 损坏）。
4.  **去重逻辑简陋**：仅支持精确字符串匹配，无法识别“用户叫小明”和“用户名字是小明”的语义重复。
5.  **缺乏元数据**：事实（facts）没有时间戳和来源，难以处理过时信息或冲突。

---

## 二、目标架构

| 层级 | 范围 | 存储路径 | 示例 |
|------|------|------|------|
| **Global** | 跨 Workspace 共享 | `memory/global/profile.json` | 用户姓名、习惯、技术栈偏好 |
| **Workspace** | 同 Workspace 跨 Task | `memory/<ws>/profile.json` | 项目代码规范、特定库路径、团队约定 |
| **Task (L3)** | 单个对话内 | 历史 Snapshot + TF-IDF | 该任务之前的尝试、具体的代码片段 |

---

## 三、改造方案

### Phase 1：存储层加固（原子性与元数据）

#### 1.1 数据结构升级 (`profile.rs`)
引入时间戳和来源，从 `Vec<String>` 升级为 `Vec<FactEntry>`。

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactEntry {
    pub content: String,
    pub timestamp: i64,
    pub source_task_id: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UserProfile {
    pub key_facts: Vec<FactEntry>,
    pub last_updated: i64,
}
```

#### 1.2 语义初步去重与原子写入
修改 `save_fact` 逻辑：
- **包含性检查**：如果新事实是旧事实的子集，不存；如果新事实包含旧事实，替换旧事实。
- **原子替换**：先写 `.json.tmp` 再 `rename`，防止写入中断损坏文件。

```rust
pub fn save_fact(workspace_name: &str, fact: &str, task_id: Option<usize>) -> anyhow::Result<()> {
    // ... load profile ...
    
    // 语义包含性检查 (简易版)
    if profile.key_facts.iter().any(|f| f.content.contains(fact)) {
        return Ok(()); // 已有更详细的事实
    }
    profile.key_facts.retain(|f| !fact.contains(&f.content)); // 移除被新事实覆盖的旧信息
    
    profile.key_facts.push(FactEntry {
        content: fact.to_string(),
        timestamp: now(),
        source_task_id: task_id,
    });
    
    // ... 原子写入逻辑 ...
}
```

---

### Phase 2：主动记忆注入（感知力提升）

#### 2.1 结构化 Context 注入 (`orchestrator.rs`)
在 `run_task` 开始时，将 Global 和 Workspace 事实作为 **System Hint** 注入。

```rust
// 注入模板
let mut memory_parts = vec![];

if !global_facts.is_empty() {
    memory_parts.push(format!("### User Profile (Global)\n{}", format_facts(global_facts)));
}
if !ws_facts.is_empty() {
    memory_parts.push(format!("### Project Context ({})\n{}", ws_name, format_facts(ws_facts)));
}

if !memory_parts.is_empty() {
    let hint = format!("[System Memory]\n{}", memory_parts.join("\n\n"));
    context.add_message(ChatMessage::new("system", &hint));
}
```

#### 2.2 工具改造 (`main_agent.rs`)
- **`RememberTool`**：增加 `scope` 参数 (`global` | `workspace` | `both`)。
- **`RecallTool`**：保留，但作为“深度搜索”手段。当注入的事实过多（如 > 15条）时，提示 LLM 使用工具查看完整列表。

---

### Phase 3：长任务记忆（快照与检索）

#### 3.1 异步快照生成
对话结束或阶段性任务完成时，触发 `generate_snapshot_sync`。
- **优化**：对于超长对话（如 > 10 轮），每隔 N 轮自动生成一次中间快照。

#### 3.2 L3 上下文关联
利用已有的 `tfidf_search`，根据当前用户的 Query，检索其他 Task 的历史片段，作为补充上下文注入。

---

## 四、改动清单

| 文件 | 改动点 |
|------|------|
| `memory/profile.rs` | 数据结构升级、语义去重逻辑、原子写入实现。 |
| `memory/storage.rs` | 新增 `get_global_memory_dir()`，优化 `save` 接口。 |
| `agents/core/main_agent.rs` | 升级 `RememberTool` 参数，更新 System Prompt 引导 LLM 正确分类存储。 |
| `agents/core/orchestrator.rs` | 实现 `run_task` 时刻的主动结构化注入。 |
| `runtime/job_manager.rs` | 接入对话结束后的 Snapshot 异步触发逻辑。 |

---

## 五、去重策略与冲突处理

1.  **精确匹配**：基于 `HashSet` 或 `Vec::contains`。
2.  **子串覆盖**：新旧事实互相包含时的替换逻辑。
3.  **时间优先**：当 Global 和 Workspace 信息冲突时（如全局偏好 Java 但本项目使用 Rust），Workspace 信息优先级更高。
4.  **LLM 维护**：未来可引入定时任务，由 LLM 对 `profile.json` 进行整体压缩和矛盾清理。

---

## 六、实施计划

1.  **第一阶段 (Day 1)**：底层加固。升级数据结构，实现原子写入和全局路径支持。
2.  **第二阶段 (Day 1.5)**：感知注入。在 `orchestrator` 中实现结构化 Hint 注入，更新工具协议。
3.  **第三阶段 (Day 2)**：长记忆接入。接通 Snapshot 生成逻辑，优化超长对话的性能。
4.  **第四阶段 (Day 2.5)**：测试与调优。验证跨 Workspace 记忆流转和去重效果。
