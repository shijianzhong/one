# 记忆系统方案

> 生成时间：2026-06-06
> 相关代码：`src/memory/`、`src/agents/core/main_agent.rs`

---

## 一、现状

### 当前存储结构

```
~/Library/Application Support/one/memory/
├── Default/
│   └── profile.json          ← 当前 workspace 的事实
├── 工作A/
│   └── profile.json
└── ... (其他 workspace)
```

每个 workspace 有一个独立的 `profile.json`，存该 workspace 下记录的事实。`save_fact` 和 `get_all_facts` 都只操作当前 workspace 的文件。

### 问题

1. **没有全局记忆**：用户说"我叫小明"，在不同 workspace 下都要重新告诉 AI
2. **Workspace 记忆已经是隔离的**：但 recall 只查当前 workspace，不能跨 workspace 查
3. **Task 记忆**：靠对话上下文传递，无需持久化

---

## 二、目标

| 层级 | 范围 | 存储方式 | 示例 |
|------|------|---------|------|
| 全局 | 所有 workspace 共享 | `memory/global/profile.json` | 用户姓名、语言偏好、常用工具 |
| Workspace | 同一 workspace 下跨 task | `memory/<workspace>/profile.json`（已有） | 项目代码风格、项目约定 |
| Task | 单次对话内 | 对话上下文（已有，不需改动） | 对话历史 |

### 读写规则

| 操作 | 写入位置 | 读取位置 |
|------|---------|---------|
| 用户说"我叫小明" | 全局 + 当前 workspace | — |
| "这个项目用 Rust 写的" | 当前 workspace（不写全局） | — |
| 用户问"我叫什么" | — | 先查全局，再查当前 workspace |
| "这个项目的规范是什么" | — | 查当前 workspace（不查全局） |

---

## 三、方案设计

### 1. 存储层 (`src/memory/`)

新增一个全局记忆文件：

```
~/Library/Application Support/one/memory/
├── global/
│   └── profile.json          ← 新增：全局事实
├── Default/
│   └── profile.json          ← 已有：workspace 事实（不变）
└── ...
```

`profile.json` 格式不变（已有）：
```json
{
  "key_facts": ["用户叫小明", "用户喜欢用VS Code"],
  "last_updated": 1717600000
}
```

### 2. API 层 (`src/memory/profile.rs`)

新增两个函数：

```rust
/// 保存到全局记忆
pub fn save_global_fact(fact: &str) -> Result<()>;

/// 获取所有全局事实
pub fn get_global_facts() -> Vec<String>;
```

已有函数不变：
```rust
pub fn save_fact(workspace_name: &str, fact: &str) -> Result<()>;      // 不变
pub fn get_all_facts(workspace_name: &str) -> Vec<String>;              // 不变
```

### 3. Agent 层 (`src/agents/core/main_agent.rs`)

#### `remember` 工具改造

目前的 `remember` 只存到 workspace：改为同时存到全局，但 LLM 可以选择：

```rust
// remember 工具的 parameters_schema 新增：
"scope": {
    "type": "string",
    "enum": ["auto", "global", "workspace"],
    "description": "存储范围：global（全局，所有 workspace 共享）、workspace（仅当前 workspace）。默认 auto——与当前 workspace 相关的存 workspace，与用户个人相关的存 global。"
}
```

#### `recall` 工具改造

目前的 `recall` 只查当前 workspace。改为先查全局 + 再查当前 workspace：

```rust
async fn call(&self, _args: Value) -> Result<Value> {
    let mut all_facts = crate::memory::profile::get_global_facts();
    all_facts.extend(crate::memory::profile::get_all_facts(&self.workspace));
    Ok(json!(all_facts))
}
```

### 4. System Prompt 调整

在已有的记忆指令中补充 scope 说明：

```
你有 remember 和 recall 两个记忆工具：
- 回答前先调 recall 查看已有信息。
- 获取到关于用户的个人信息（姓名、偏好等），调 remember 保存（scope=global）。
- 获取到关于当前工作区/项目的信息（代码规范、项目结构等），调 remember 保存（scope=workspace）。
- 由你自行判断信息应该存到哪个范围。
```

---

## 四、文件改动清单

| 文件 | 改动 |
|------|------|
| `src/memory/storage.rs` | 新增 `get_global_memory_dir()` 函数 |
| `src/memory/profile.rs` | 新增 `save_global_fact()` 和 `get_global_facts()` 函数 |
| `src/agents/core/main_agent.rs` | `remember` 工具新增 scope 参数；`recall` 工具改为查全局+workspace；system prompt 更新 |

---

## 五、不变的部分

- 对话上下文：走 orchestrator history，不纳入记忆系统
- Task 级别：不持久化，靠对话上下文传递
- `profile.json` 格式：不变
- workspace 隔离：不变
- IntentRouter：不变
- Task DB：不变

---

## 六、后续可扩展

1. **记忆搜索**：当前是精确匹配去重，后续可按关键词搜索
2. **记忆清理/过期**：长时间未使用的事实自动清理
3. **记忆重要性排序**：重要事实优先展示
4. **跨 workspace 查询**：允许用户问"我在其他项目里是怎么配置的"