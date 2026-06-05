# 远程触发安全确认机制 - 改造方案

> 生成时间：2026-06-04
> 版本：v11（完整代码对照修订版）

---

## 一、现状问题

当前 `RemoteScopeGuard` 机制：
- 远程触发 → Strict 模式 → Shell 直接 Deny、File/Process 必须本机弹窗确认
- 问题：没有人在电脑前时，危险操作永远卡住

---

## 二、已知实现约束（代码审查发现）

以下约束是审查现有代码后发现的，实现中必须遵守：

1. **`TelegramTrigger` 必须用 `std::sync::Mutex` 包装可变状态**
   当前 `Trigger::run(&self)` 签名不可变（来自 `#[async_trait]`），新增的 `pending` HashMap 等可变状态需要 `std::sync::Mutex` 包装。
   ⚠️ 必须用 `std::sync::Mutex` 而非 `tokio::sync::Mutex`：Telegram trigger 运行在独立的 `std::thread::spawn` + 独立 `tokio::Runtime` 里（见 `main.rs`），不是主 GPUI 运行时的 worker thread。`run()` 在单个 tokio 任务里串行执行，`.lock().unwrap()` 永远不会死锁，也不会跨 `.await` 持锁，不需要 tokio 异步锁。

2. **不修改 `Skill::execute` trait 签名**
   现有 5 个 Skill 的 `execute(&self, args: Value, source: Option<&str>)` 签名不应新增参数。workspace_id/task_id 通过 `TriggerEvent` 在外层传递，Skill 执行结果由 `telegram.rs` 的调用方追加到对应 Task。

3. **`RemoteScopeGuard` 自动升级 `source` 的隐式链路**
   `dispatcher.rs` 里 `run_skill` 调用 `skill.execute(args, None)`（source=None），同时持有 `RemoteScopeGuard`。`permission().request_async` 内部检测到 `RemoteScopeGuard::is_active()` 后自动将 `effective_source` 升级为 `Some("remote")`，进而触发 Strict 分支。`DangerLevel` 方案在 dispatcher 层加判断不影响这条链路，两者正交。

4. **新增 `bcrypt` 依赖** — Cargo.toml 当前无 bcrypt / argon2 依赖，需 `cargo add bcrypt`

5. **`dispatch()` 是纯函数** — 不持有状态，危险操作判断只返回标记（`TriggerReply` 扩展），暗号等待和超时管理在 `telegram.rs` 的 `run()` loop 里处理

6. **新文件路径** — 权限模块实际位于 `src/agents/permission.rs`，新文件应为 `src/agents/remote_auth.rs`

7. **配置路径分离**
   - `telegram_bot_token` / `telegram_chat_id` 存入现有 `~/.one/config.json`，对应 `services/config.rs` 里的 `Config` 结构体（**需要新增这两个字段并加 `#[serde(default)]`**）
   - `remote_auth.json` 只存认证状态（暗号哈希、锁定计数、default_workspace_id），避免 Token 与安全凭据混存
   - 路径由 `dirs::config_dir().join(".one")` 计算，与现有 `get_config_path()` 保持一致

8. **`PendingConfirmation` 必须实现 `Drop`** — 确保 `local_approval_tx` 在超时/取消时发送 `false` 信号，防止本机弹窗永久悬挂

9. **`SkillManifest` 新增字段需要向后兼容**
   `SkillManifest` 用 `#[derive(Serialize, Deserialize)]`，新增 `danger_level: DangerLevel` 字段时**必须加 `#[serde(default)]`**，否则已有序列化数据（如 task_db 里存的 manifest 快照）反序列化时会报错。`DangerLevel` 也需实现 `Default`（默认值为 `Normal`）。

10. **`TelegramTrigger` 需要新增 `from_config()` 构造器，`main.rs` 随之更新**
    当前 `TelegramTrigger::from_env()` 只读环境变量。绑定引导流程写入 `config.json` 后，`main.rs` 需改为优先读配置文件，fallback 到环境变量：
    ```rust
    // main.rs 改后
    TelegramTrigger::from_config(&config)
        .or_else(|| TelegramTrigger::from_env())
    ```
    Phase 2 实现时必须同步新增 `from_config(config: &Config) -> Option<Self>` 构造器。

---

## 三、最终方案：分级确认 + 暗号认证

### 核心设计

```
操作危险等级分类：
├── 普通操作（查看、查询）→ 直接放行，无需确认
├── 危险操作（删除、执行 Shell）→ Telegram 输入暗号确认
└── 极度危险操作（格式化、sudo rm -rf）→ 暗号 + 本机弹窗双确认
```

### 暗号设置时机

- 暗号**只能在本机 GPUI 设置页里设置**，不经过 Telegram / 网络
- 首次危险操作时，若暗号未设置，拒绝执行并通过 Telegram 回复："请先在本机 ONE 设置页配置远程暗号"
- 暗号：用户自定义短语（如"芝麻开门"、"我是主人"），灵动有趣

---

## 四、危险操作分级表

| 操作类型 | 危险等级 | 确认方式 |
|---------|---------|---------|
| 文件列表查询、搜索、状态查询 | Normal | 直接放行 |
| 删除文件、执行 Shell 命令 | Dangerous | TG 输入暗号确认 |
| 格式化磁盘、sudo rm -rf、卸载系统级应用 | Extreme | 暗号 + 本机弹窗双确认 |

---

## 五、工作流程

### 安全层级

```
Telegram 消息 → chat_id 白名单校验 → 通过后才进入危险等级判断 → 暗号认证（危险操作时）
```

### 普通操作

```
Telegram 发送指令 → 直接执行 → TG 返回结果
```

### 危险操作（Dangerous）

```
1. 用户 Telegram 发送危险指令（如删除文件）
2. 若暗号未设置 → TG 回复"请先在本机设置远程暗号" → 结束
3. Bot 回复："📐 此操作需要确认，请在 2 分钟内回复暗号"
4. TelegramTrigger 在 PendingConfirmation 里登记（skill_id + args + expires_at）
5. 用户发送暗号
6. 本机 bcrypt 验证
7. 验证通过 → 执行 → TG 反馈结果
8. 验证失败 → TG 返回"暗号错误，已拒绝"（记录失败次数）
9. 超时（2分钟未回复）→ TG 返回"确认超时，操作已取消"
```

### 极度危险操作（Extreme）- 双确认

```
1. 用户 Telegram 发送极度危险指令（如格式化）
2. 若暗号未设置 → TG 回复"请先在本机设置远程暗号" → 结束
3. Bot 回复："⚠️ 此操作需要双重确认：
   1) 请在 2 分钟内回复暗号
   2) 本机将同步弹出确认窗口"
4. TelegramTrigger 登记 PendingConfirmation（含 needs_local_approval=true）
5. 同时向 ApprovalQueue 投递本机弹窗请求
6. tokio::select!：
   ├── 等待暗号回复（2分钟超时）
   └── 等待本机 ApprovalQueue oneshot 结果
7. 暗号验证通过 AND 本机点击 Allow → 执行
8. 任一失败或超时 → 取消另一个等待，TG + 本机同步提示拒绝
```

---

## 六、远程 Workspace 机制

### 问题

当前 Telegram 命令没有 workspace 上下文：
- `dispatch()` 和 `run_skill()` 都没有 workspace 参数
- `SkillManifest` 没有 workspace 字段

但 ONE 架构里每个任务都挂在某个 workspace 下，远程操作也必须有 workspace 上下文。

### 解决方案：绑定时指定 + 动态切换

1. **绑定时指定默认 workspace**
   - Telegram 首次绑定时，从已有 workspace 列表中选择一个作为默认
   - 如果没有 workspace，自动创建一个名为 `远程作业区` 的 workspace
   - 在 `remote_auth.json` 里记录 `default_workspace_id`

2. **动态切换 workspace**
   - 新增命令：`/workspace <name>` 切换当前 workspace
   - 切换后所有远程操作（`/preview`、`/run` 等）都在该 workspace 下执行
   - 当前 workspace_id 存入 `TelegramTrigger.current_workspace_id`（`std::sync::Mutex` 包装）

### Workspace 切换流程

```
1. 用户发送：/workspace 工作区B
2. Bot 查找 workspace "工作区B"（模糊匹配）
3. 找到 → 更新 current_workspace_id
   → TG 回复："已切换到「工作区B」，后续操作将在此工作区执行"
4. 未找到 → TG 回复："未找到 workspace「工作区B」，请先在本机创建"
```

### 新增命令

| 命令 | 说明 |
|-----|------|
| `/workspace [name]` | 切换到指定 workspace；不带参数时显示当前 workspace |
| `/workspaces` | 列出所有 workspace |

---

## 七、Telegram 绑定引导流程

### 问题

用户需要手动配置环境变量来设置 chat_id 白名单，流程复杂。

### 解决方案：GPUI 设置页引导

```
1. 用户在 GPUI 设置页输入 Bot Token
       ↓
2. 点击"绑定 Telegram"
       ↓
3. 页面提示："请在 Telegram 给 Bot 发任意消息"
       ↓
4. 用户发消息 → 系统自动获取 chat_id → 写入 config.json
       ↓
5. 绑定成功，显示"已连接到 Telegram"
```

### 用户操作步骤

1. 去 Telegram 找 **@BotFather** 创建 Bot，拿 Token
2. 把 Token 粘贴到 GPUI 设置页
3. 给 Bot 发条消息
4. 完成！系统自动完成 chat_id 获取和配置

### 系统自动完成

- 自动获取 chat_id
- 将 Token + chat_id 写入 `~/.one/config.json`（**不写入 remote_auth.json**）
- `services/config.rs` 的 `Config` 结构体需新增对应字段（加 `#[serde(default)]`）
- 测试连接是否正常

### GPUI 设置页 UI

```
┌─────────────────────────────────────┐
│  Telegram 远程控制                    │
├─────────────────────────────────────┤
│  Bot Token: [________________]       │
│                                     │
│  [绑定 Telegram]  状态: 未绑定        │
│                                     │
│  绑定后可通过 Telegram 远程控制电脑    │
└─────────────────────────────────────┘

绑定后：
┌─────────────────────────────────────┐
│  Telegram 远程控制                    │
├─────────────────────────────────────┤
│  Bot Token: ********************     │
│                                     │
│  [解绑]  状态: ✅ 已连接 (chat_id: xxx)│
│                                     │
│  默认 Workspace: [下拉选择 ▼]         │
│  远程暗号: [已设置] [修改]             │
└─────────────────────────────────────┘
```

---

## 八、远程作业 Task 机制

### 设计原则

**远程 Telegram 会话对应的 Task 永远不标记为 done**。它是一个持续追加的会话记录。

### Task 生命周期

```
TG 发送消息 → 检查 current_task_id
       ↓
   ├─ 有 current_task_id → 追加到该 Task
   └─ 无 current_task_id → 创建新 Task，设置 current_task_id
       ↓
追加 Step 到 Task
       ↓
发送 /clear → current_task_id = null（老 Task 在数据库里保留）
       ↓
下次 TG 消息 → 回到步骤 1，创建新 Task
```

**原则**：
- 发送消息才创建 Task，不提前创建
- Task 永远不标记为 done，持续追加到用户主动 `/clear` 为止
- 30 分钟无活动：只更新 `last_active_at`，不做其他处理

### Task 不使用 status 字段

| 字段 | 用途 |
|-----|------|
| `status` | 仅用于 GPUI 侧"任务管理"视图，远程 Task 不使用 |
| `last_active_at` | Task 最后活动时间 |
| `created_at` | Task 创建时间 |

### Step 追加机制

每次 Skill 执行或追问都作为 Step 追加到 Task：

```rust
struct TaskStep {
    step_index: u32,           // 第几步
    step_type: StepType,       // skill_execute / user_message / system_response
    skill_id: Option<String>,  // 如果是 Skill 执行
    content: String,           // 内容摘要
    created_at: DateTime,
}
// messages 表扩展 step_index 字段
```

### 新增命令

| 命令 | 说明 |
|-----|------|
| `/status` | 显示当前远程 Task 状态和最近几步 |
| `/tasks` | 列出该 workspace 下的所有远程 Task |
| `/clear` | 手动结束当前远程 Task（不删除，仅清空 current_task_id） |

---

## 九、暗号认证架构

**文件**: `src/agents/remote_auth.rs`（新增，与 `src/agents/permission.rs` 同级）

```rust
pub struct RemoteAuth {
    config_path: PathBuf,  // ~/.one/remote_auth.json
}

const MAX_FAILED_ATTEMPTS: u32 = 3;
const LOCK_DURATION_SECS: u64 = 300; // 5 分钟，可配置

impl RemoteAuth {
    pub fn new() -> Self;
    pub fn load_cipher_hash(&self) -> Result<Option<String>>;
    pub fn set_cipher(&self, cipher: &str) -> Result<()>;    // 只在本机 GPUI 调用
    pub fn verify_cipher(&self, cipher: &str) -> Result<bool>;
    pub fn is_cipher_set(&self) -> bool;
    pub fn record_failure(&self) -> Result<()>;              // 超 3 次锁 5 分钟
    pub fn locked_for_secs(&self) -> u64;                    // 0 表示未锁定
}
```

### 依赖变更

```toml
# Cargo.toml 新增
bcrypt = "0.16"
```

### Telegram 状态机

⚠️ **`Trigger::run(&self)` 签名不可变（`#[async_trait]`），所有新增可变状态必须用 `std::sync::Mutex` 包装**。
Telegram trigger 运行在独立 `std::thread` + 独立 `tokio::Runtime` 里（`main.rs`），`run()` 串行执行，不会跨 `.await` 持锁，`std::sync::Mutex` 的 `.lock().unwrap()` 永远安全，不需要 `tokio::sync::Mutex`：

```rust
pub struct TelegramTrigger {
    token: String,
    api_base: String,
    allowed_chats: HashSet<i64>,
    client: reqwest::Client,
    // 新增可变状态（std::sync::Mutex）
    pending: std::sync::Mutex<HashMap<i64, PendingConfirmation>>,
    current_workspace_id: std::sync::Mutex<String>,
    current_task_id: std::sync::Mutex<Option<String>>,
}
```

```rust
struct PendingConfirmation {
    skill_id: String,
    args: serde_json::Value,
    workspace_id: String,
    task_id: String,
    danger_level: DangerLevel,
    needs_local_approval: bool,
    local_approval_tx: Option<oneshot::Sender<bool>>,
    expires_at: std::time::Instant,  // 2 分钟后自动取消
}

impl Drop for PendingConfirmation {
    fn drop(&mut self) {
        // 确保本机弹窗在超时/取消时被撤销，防止永久悬挂
        if let Some(tx) = self.local_approval_tx.take() {
            let _ = tx.send(false);
        }
    }
}
```

### 极度危险双确认并发协调

```rust
// Extreme 操作的并发等待（伪代码）
async fn wait_for_extreme_confirm(local_rx, cipher_rx, timeout) -> bool {
    tokio::select! {
        _ = tokio::time::sleep(timeout) => false,
        r = local_rx => r.unwrap_or(false),
        cipher = cipher_rx => {
            let cipher_ok = verify_cipher(cipher);
            let local_ok = local_rx.await.unwrap_or(false);
            cipher_ok && local_ok
        }
    }
}

// 执行侧：Skill::execute 签名不变（source=None，由 RemoteScopeGuard 自动升级）
// workspace_id/task_id 在调用方层面传递，执行结果追加到 Task
let result = skill.execute(args, None).await;
append_step_to_task(workspace_id, task_id, &result);
```

---

## 十、配置文件格式

### `~/.one/config.json`（现有文件，`Config` 结构体新增 Telegram 字段）

⚠️ **`services/config.rs` 的 `Config` 结构体需新增以下字段并加 `#[serde(default)]`**，否则旧版 `config.json` 反序列化时报错：

```rust
// services/config.rs Config 结构体新增
#[serde(default)]
pub telegram_bot_token: Option<String>,
#[serde(default)]
pub telegram_chat_id: Option<String>,
#[serde(default)]
pub telegram_bound_at: Option<String>,
```

对应 JSON：
```json
{
  "...现有字段...",
  "telegram_bot_token": "123456789:ABCdef...",
  "telegram_chat_id": "123456789",
  "telegram_bound_at": "2026-06-04T10:00:00Z"
}
```

### `~/.one/remote_auth.json`（新增，仅存认证状态）

```json
{
  "cipher_hash": "$2b$12$...",
  "created_at": "2026-06-04T10:00:00Z",
  "failed_attempts": 0,
  "locked_until": null,
  "default_workspace_id": "ws_12345",
  "default_workspace_name": "工作区A",
  "max_failed_attempts": 3,
  "lock_duration_secs": 300
}
```

**Token 与暗号哈希分文件存储，避免单文件泄露导致双重风险。**

---

## 十一、DangerLevel 与 ToolKind 的关系

两个维度分别挂在不同层级，互不替代：

- **`DangerLevel`**：挂在 `SkillManifest` 上，表示 Skill 整体危险程度，由 Skill 作者声明
- **`ToolKind`**：挂在具体工具调用上（Shell/File/Process），表示单次操作类型

`dispatcher::dispatch()` 查 `Skill.manifest().danger_level` 决定远程确认流程。`dispatch()` 是纯函数，不持有 `PendingConfirmation` 状态，只返回"需要确认"标记，等待逻辑全在 `telegram.rs` 的 `run()` loop 里。

`PermissionPolicy.evaluate()` 查 `ToolKind` 决定本机是否弹窗。两者在 Extreme 操作时同时生效，形成双确认。

```rust
// src/agents/permission.rs 新增
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum DangerLevel {
    #[default]
    Normal,    // 直接放行
    Dangerous, // 暗号确认
    Extreme,   // 暗号 + 本机弹窗
}

// src/skills/mod.rs SkillManifest 新增字段
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillManifest {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: SkillCategory,
    #[serde(default)]               // ⚠️ 必须加，保证旧数据反序列化不崩溃
    pub danger_level: DangerLevel,  // 默认 Normal
}
```

---

## 十二、安全考虑

1. **暗号不经过网络**：暗号只在本机 GPUI 设置，Telegram 里只做提交验证
2. **哈希存储**：bcrypt 哈希，不可逆
3. **错误限流**：连续 3 次错误，锁定 5 分钟（可配置）
4. **会话隔离**：每次危险操作独立验证，不复用 token
5. **确认超时**：`PendingConfirmation` 2 分钟过期自动取消，防止悬挂
6. **双确认任一失败即拒绝**：Extreme 操作暗号 OR 本机 Deny 均触发取消，另一侧同步撤销
7. **Drop 守卫**：`PendingConfirmation` 实现 `Drop`，确保 `local_approval_tx` 不永久悬挂
8. **配置分离**：Token 在 `config.json`，暗号哈希在 `remote_auth.json`，单文件泄露不导致双重风险
9. **运行时隔离**：Telegram trigger 在独立 `std::thread` + 独立 `tokio::Runtime` 里运行，与主 GPUI 运行时完全隔离，`RemoteScopeGuard` 的 `thread_local!` 天然不会污染 GPUI 线程

---

## 十三、文件改动清单

| 文件 | 改动类型 | 说明 |
|-----|---------|------|
| `src/agents/remote_auth.rs` | 新增 | 暗号验证模块（bcrypt + 锁定） |
| `src/agents/permission.rs` | 修改 | 新增 `DangerLevel` 枚举（含 `Default` impl） |
| `src/triggers/dispatcher.rs` | 修改 | 危险等级判断 + workspace/task 命令处理 |
| `src/triggers/telegram.rs` | 修改 | `PendingConfirmation` 状态机；`std::sync::Mutex` 包装可变状态；新增 `from_config()` 构造器 |
| `src/skills/mod.rs` | 修改 | `SkillManifest` 新增 `danger_level`（`#[serde(default)]`）；**不修改 `Skill::execute` 签名** |
| `src/services/config.rs` | 修改 | `Config` 结构体新增 `telegram_bot_token` / `telegram_chat_id` / `telegram_bound_at`（均加 `#[serde(default)]`） |
| `src/main.rs` | 修改 | 启动逻辑改为 `TelegramTrigger::from_config(&config).or_else(\|\| TelegramTrigger::from_env())` |
| `src/ui/dialogs.rs` | 修改 | 暗号设置 UI + Telegram 绑定引导 UI |
| `src/task_db.rs` | 修改 | Task Step 扩展字段；活跃 Task 查询接口 |

---

## 十四、实现步骤

### Phase 1：暗号设置 UI + 核心验证模块（1 天）

0. **前置**：`cargo add bcrypt`
1. 新增本机 GPUI 设置页暗号配置入口（输入 → bcrypt hash → 写 `remote_auth.json`；支持修改、清除）
2. 新增 `RemoteAuth` 模块（`src/agents/remote_auth.rs`）：bcrypt 存储/验证、失败计数、锁定
3. 定义 `DangerLevel` 枚举（加 `Default` impl），挂到 `SkillManifest`（加 `#[serde(default)]`），为现有 5 个 Skill 标注危险等级

### Phase 2：Telegram 绑定引导 + 状态机（1.5 天）

4. `Config` 结构体新增 `telegram_bot_token` / `telegram_chat_id`（加 `#[serde(default)]`）
5. 新增 `TelegramTrigger::from_config(config: &Config) -> Option<Self>` 构造器；更新 `main.rs` 启动逻辑优先读配置文件
6. GPUI 设置页 Telegram 绑定引导：输入 Token → 调 Telegram API 获取 chat_id → 写 `config.json`
7. `TelegramTrigger` 新增 `PendingConfirmation` HashMap（`std::sync::Mutex` 包装）；消息 loop 区分普通命令和暗号回复；`expires_at` 超时检查
8. `dispatcher.rs` 新增危险等级判断：`Normal` 直接执行；`Dangerous` 返回 `needs_cipher` 标记；`Extreme` 同上 + 投递本机 `ApprovalQueue`

### Phase 3：极度危险双确认协调（0.5 天）

9. `tokio::select!` 协调暗号 + 本机弹窗两个 oneshot；任一失败/超时取消另一侧；`PendingConfirmation` Drop 守卫

### Phase 4：Workspace + Task 机制（0.5 天）

10. Workspace 切换命令（`/workspace`、`/workspaces`）
11. Task 创建和 Step 追加（workspace_id/task_id 通过外层传递，不修改 `Skill::execute`）

### Phase 5：测试（0.5 天）

12. 单元测试：`RemoteAuth` verify / lock 逻辑；`DangerLevel` serde 向后兼容
13. 集成测试：dispatcher 危险等级路由；`from_config` 构造器
14. 手动验收：Telegram → 暗号 → 执行全链路

---

## 十五、预计工时

| Phase | 内容 | 工时 |
|-------|------|------|
| Phase 1 | 暗号设置 UI + 核心验证模块 + DangerLevel | 1 天 |
| Phase 2 | 绑定引导 + Config 结构体 + from_config + 状态机 + dispatcher | 1.5 天 |
| Phase 3 | 极度危险双确认协调 | 0.5 天 |
| Phase 4 | Workspace + Task 机制 | 0.5 天 |
| Phase 5 | 测试 | 0.5 天 |
| **总计** | | **约 4 人天** |

---

## 十六、暗号示例

| 类型 | 暗号示例 |
|-----|---------|
| 中文俗语 | "芝麻开门"、"天王盖地虎" |
| 简短英文 | "open sesame"、"I am home" |
| 自创短语 | "咖啡不要糖"、"周末愉快" |
| 数字组合 | "5201314"、"2026" |

---

## 十七、后续可扩展

1. **生物识别**：macOS Face ID / Touch ID 替代暗号
2. **硬件密钥**：YubiKey 支持
3. **操作审批历史**：记录谁在什么时间确认了什么操作（RunRecorder 扩展）
4. **白名单机制**：某些 Skill 可以设置"信任的远程来源"直接放行

---

## 十八、缺失的关键点（代码审查补充）

以下关键点是在全面审查代码后发现的，文档原稿未覆盖或需要明确决策。

### 1. `dispatcher.rs` 的 `skill.execute()` 调用缺少 `source` 参数

**问题**：`dispatcher.rs:155` 的调用是：
```rust
let result = skill.execute(args).await;
```
但 `Skill::execute` 的 trait 签名（`skills/mod.rs:73`）是：
```rust
async fn execute(&self, args: serde_json::Value, source: Option<&str>) -> anyhow::Result<SkillExecution>;
```
**实际代码中必须传参**，即应改为：
```rust
let result = skill.execute(args, None).await;
```
利用 `RemoteScopeGuard` 自动升级 source 的隐式链路（见 §二.3）。

**方案修正**：所有涉及 `skill.execute()` 调用的伪代码和实现必须统一为 `skill.execute(args, None).await`。

### 2. `TriggerReply` 是否需要扩展以携带结构化的"需要确认"标记

**问题**：当前 `TriggerReply`（`triggers/mod.rs:33`）只有 text 字段：
```rust
pub struct TriggerReply {
    pub text: String,
}
```
方案说 dispatcher 返回"需要确认的标记"，但 dispatcher 是纯函数（§二.5），不保持状态。有两种方案：

**方案 A（推荐）**：扩展 `TriggerReply`：
```rust
pub struct TriggerReply {
    pub text: String,
    /// 是否需要远程暗号确认
    pub needs_cipher: bool,
    /// 危险等级（Normal 时直接放行）
    pub danger_level: DangerLevel,
}
```
`dispatch()` 通过查 `SkillManifest::danger_level` 设置这些字段。`telegram.rs` 的 `run()` loop 根据 `needs_cipher` 决定是否进入 `PendingConfirmation` 状态机。
- ✅ 结构清晰，不依赖文本匹配
- ✅ `dispatch()` 保持纯函数（只做判断和标记，不做等待）
- ⚠️ 需要修改 `triggers/mod.rs` 的 `TriggerReply` 定义

**方案 B**：dispatcher 返回文本提示，`telegram.rs` 收到回复后**自己查** `registry().find(id)` 判断 danger_level。
- ❌ 需要 `telegram.rs` 引入 `skills::registry` 依赖
- ❌ 文本匹配脆弱（如"/run system.cleaner"和"你确定要运行 system.cleaner 吗？需输入暗号"这种文本耦合）
- ✅ 不改 `TriggerReply`

**决策**：采用方案 A。需要修改文件：`triggers/mod.rs` + `triggers/dispatcher.rs` + `triggers/telegram.rs`。

### 3. Extreme 双确认与 `ApprovalQueue` 的接口缺失

**问题**：`permission.rs` 的 `ApprovalQueue`（`permission.rs:180-206`）当前只提供一个接口 `enqueue_request()`，它会投递请求并 **await oneshot**，即阻塞直到用户确认。但 Extreme 操作的**双确认**流程需要：

1. 先投递本机弹窗请求（**不等待**）
2. 同时等待暗号回复
3. 暗号验证通过后，再去 await 本机弹窗的 oneshot

当前 `ApprovalQueue` 没有"投递但不等待"的接口。

**方案**：新增 API：

```rust
// permission.rs 新增
/// 投递本机审批请求但不等待结果。返回 oneshot Receiver，暗号验证通过后再 await。
pub fn enqueue_detached(kind: ToolKind, detail: String) -> Option<oneshot::Receiver<bool>> {
    let (tx, rx) = oneshot::channel();
    {
        let mut q = queue().lock().ok()?;
        q.next_id = q.next_id.wrapping_add(1);
        let id = q.next_id;
        q.pending.push(ApprovalRequest {
            id,
            kind,
            detail,
            responder: tx,
        });
    }
    Some(rx)
}
```

`telegram.rs` 的 Extreme 流程使用方式：
```rust
let local_rx = enqueue_detached(ToolKind::Shell, detail);
// 此时用户电脑上已经弹出审批窗口
// telegram.rs 等待暗号回复
// 暗号验证通过后：
let local_ok = local_rx.await.unwrap_or(false);
```

### 4. 绑定引导流程获取 chat_id 的竞态风险

**问题**：方案 §七 说"用户发消息 → 系统自动获取 chat_id"，但 `getUpdates` 返回所有未读消息。如果绑定过程中有其他用户向 bot 发消息，可能拿到错误的 chat_id。

**方案**：绑定引导中调用 `getUpdates` 获取 chat_id 时，增加消息校验：
```rust
// 绑定流程中，调 Telegram API getUpdates，筛选 chat_id
// 只接受来自"当前本机用户"的消息（通过 msg.text 里包含绑定验证码来确认）
// 或者忽略 offset=0 之前的所有历史消息，只取 offset 之后的新消息
```

**具体做法**：
1. GPUI 生成一个随机绑定码（如 `ONE_BIND_20260605_abc123`）
2. 用户需要发消息给 Bot，**消息内容必须包含该绑定码**
3. Bot 在 `getUpdates` 结果中搜索包含该绑定码的消息，提取对应的 chat_id
4. 验证通过后写入 config.json

### 5. 现有 5 个 Skill 的 DangerLevel 分级表

方案 Phase 1 说"为现有 5 个 Skill 标注危险等级"，但未给出具体分级。基于代码审查：

| Skill | 文件 | 实际行为 | 推荐 DangerLevel | 理由 |
|-------|------|---------|-----------------|------|
| `system.cleaner` | `skills/system_cleaner.rs` | 读取 + 删除缓存目录文件 | **Dangerous** | 删除文件，不可恢复 |
| `desktop_organizer` | `skills/desktop_organizer.rs` | 整理桌面文件（移动/分类） | **Dangerous** | 移动/可能删除文件 |
| `app_uninstaller` | `skills/app_uninstaller.rs` | 卸载应用 | **Dangerous** | 卸载系统应用，破坏性操作 |
| `doc_summarizer` | `skills/doc_summarizer.rs` | 文档摘要 | **Normal** | 只读操作，不修改系统 |
| `media_dedup` | `skills/media_dedup.rs` | 媒体去重 | **Dangerous** | 可能删除重复文件 |

实现时在对应 Manifest 中标注：
```rust
SkillManifest {
    id: "system.cleaner",
    danger_level: DangerLevel::Dangerous,
    // ...其他字段
}
```

### 6. 暗号遗忘/重置机制

**问题**：方案说暗号只能在本机设置。但如果用户设置了暗号后忘记了，无法远程解锁。

**方案**：GPUI 设置页提供"重置暗号"功能，需要**本机用户手动确认**（不需要验证旧暗号）：
- 重置 → 清空 `remote_auth.json` 中的 `cipher_hash`
- 重置后远程操作提示"暗号已重置，请在本机设置新暗号"
- 也可以直接手动删除 `remote_auth.json` 的 cipher_hash 字段来重置

**不提供远程重置**：不允许通过 Telegram 重置暗号，否则双确认机制失去意义。

### 7. `tasks.status` 字段当前是死代码，不需要在远程 Task 方案中特殊处理

**发现**：审查代码后确认 `tasks.status` 字段在现有代码中完全未被使用：
- `TaskRow` 结构体（`task_db.rs:225`）不包含 status 字段
- `load_tasks()` 的 SELECT 不包含 status（`task_db.rs:258`）
- `TaskItem`（`workspace.rs:17`）不包含 status
- 全仓库没有任何 `UPDATE tasks SET status = ...` 语句
- `insert_task` 写死了 `'todo'`（`task_db.rs:313`），从未被修改

**结论**：方案 §八 说的"远程 Task 不使用 status 字段"完全正确且与现有实践一致。远程 Task 创建时默认 `'todo'` 即可，不需要新增状态值。如果要区分"哪些 task 是远程创建的"，建议用新字段 `is_remote: bool` 而非复用 status。

| 版本 | 日期 | 变更内容 |
|-----|------|---------|
| v1 | 2026-06-04 | 初稿 |
| v2 | 2026-06-04 | 增加双确认流程 |
| v3 | 2026-06-04 | 增加暗号认证架构 |
| v4 | 2026-06-04 | 评审修订：PendingConfirmation 状态机；暗号只在本机设置；双确认 tokio::select!；统一配置路径；DangerLevel 与 ToolKind 层级关系；Phase 顺序调整 |
| v5 | 2026-06-04 | 新增远程 Workspace 机制 |
| v6 | 2026-06-04 | 新增远程作业 Task 机制 |
| v7 | 2026-06-04 | 新增 Telegram 绑定引导；文件路径修正；锁定时间配置化；Drop guard；白名单校验时机 |
| v8 | 2026-06-04 | Task 生命周期简化 |
| v9 | 2026-06-04 | 代码审查更新：已知实现约束章节；Mutex 包装；bcrypt 依赖；dispatch() 纯函数约束 |
| v10 | 2026-06-04 | 结构与一致性修订：修复重复编号；伪代码修正；Token 与暗号哈希配置分离；代码块闭合修复；Phase 2 工时 1→1.5 天 |
| v11 | 2026-06-05 | 完整代码对照修订：补充约束 3（RemoteScopeGuard 自动升级 source 的隐式链路）；约束 1 补充说明 Telegram 运行在独立 Runtime 而非 worker thread，以及 std::sync::Mutex 适用原因；约束 7 补充 Config 结构体需新增字段且加 #[serde(default)]；约束 9 新增 SkillManifest 向后兼容要求（DangerLevel 加 #[serde(default)] 和 Default impl）；约束 10 新增 from_config() 构造器和 main.rs 改动要求；文件改动清单新增 services/config.rs 和 main.rs；Phase 2 步骤细化（Config 结构体 + from_config 构造器单独列出）；§十一 DangerLevel 代码示例补充 #[serde(default)] 标注；安全考虑新增第 9 条（运行时隔离） |
| v12 | 2026-06-05 | 新增§十八"缺失的关键点（代码审查补充）"：dispatcher.rs 调用参数修正、TriggerReply 扩展设计、Extreme 双确认 ApprovalQueue 接口缺失、绑定引导 chat_id 竞态保护、5个 Skill DangerLevel 分级表、暗号重置机制 |
