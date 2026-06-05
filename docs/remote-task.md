# 远程触发安全确认机制 - 改造任务追踪

> 生成时间：2026-06-05
> 主方案文档：`docs/remote-auth-plan.md`

---

## 任务总览

| # | 任务 | Phase | 状态 | 预计工时 | 实际工时 |
|---|------|-------|------|---------|---------|
| 1 | cargo add bcrypt | Phase 0 | ✅ 已完成 | 0.1h | 0.1h |
| 2 | DangerLevel 枚举 + SkillManifest 扩展 | Phase 1-1 | ✅ 已完成 | 0.5h | 1h |
| 3 | RemoteAuth 模块 (src/agents/remote_auth.rs) | Phase 1-2 | ✅ 已完成 | 1h | 1h |
| 4 | 暗号设置 UI（GPUI 设置页） | Phase 1-3 | ✅ 已完成 | 1.5h | 2h |
| 5 | Config 结构体新增 Telegram 字段 | Phase 2-1 | ✅ 已完成 | 0.3h | 0.3h |
| 6 | TelegramTrigger::from_config() + main.rs 更新 | Phase 2-2 | ✅ 已完成 | 0.5h | 0.3h |
| 7 | GPUI Telegram 绑定引导 UI | Phase 2-3 | ✅ 已完成 | 1.5h | 1.5h |
| 8 | PendingConfirmation 状态机 | Phase 2-4 | ✅ 已完成 | 2h | 2h |
| 9 | dispatcher 危险等级路由 + TriggerReply 扩展 | Phase 2-5 | ✅ 已完成 | 1h | 0.8h |
| 10 | Extreme 极度危险双确认协调 | Phase 3 | ✅ 已完成 | 1h | 1h |
| 11 | Workspace 切换命令 (/workspace /workspaces) | Phase 4-1 | ✅ 已完成 | 1h | 0.8h |
| 12 | 远程 Task 创建和 Step 追加 | Phase 4-2 | ⏳ 待开始 | 1.5h | — |
| 13 | 测试 | Phase 5 | ✅ 已完成 | 1h | 0.5h |

**总计：预计约 4 人天**

---

## 任务详细记录

### Phase 0: 基础依赖

#### [x] Task 1: cargo add bcrypt

- **文件改动**：`Cargo.toml`
- **命令**：`cargo add bcrypt`
- **说明**：bcrypt 用于暗号哈希存储，不经过网络传输
- **完成时间**：

---

### Phase 1: 暗号核心模块

#### [ ] Task 2: DangerLevel 枚举 + SkillManifest 扩展

- **涉及文件**：
  - `src/agents/permission.rs` — 新增 `DangerLevel` 枚举（`Normal | Dangerous | Extreme`），实现 `Default`
  - `src/skills/mod.rs` — `SkillManifest` 新增 `danger_level: DangerLevel`（加 `#[serde(default)]`）
  - `src/skills/system_cleaner.rs` — 标注 `DangerLevel::Dangerous`
  - `src/skills/desktop_organizer.rs` — 标注 `DangerLevel::Dangerous`
  - `src/skills/app_uninstaller.rs` — 标注 `DangerLevel::Dangerous`
  - `src/skills/doc_summarizer.rs` — 标注 `DangerLevel::Normal`
  - `src/skills/media_dedup.rs` — 标注 `DangerLevel::Dangerous`
- **验证**：`cargo build` 通过
- **完成时间**：

#### [x] Task 3: RemoteAuth 模块

- **涉及文件**：
  - `src/agents/remote_auth.rs`（新建）
  - `src/agents/mod.rs` — 注册新模块
- **核心 API**：
  - `RemoteAuth::new()` — 加载 `~/.one/remote_auth.json`
  - `set_cipher(&self, cipher: &str)` — bcrypt hash 后写入文件
  - `verify_cipher(cipher) -> Result<bool, String>` — bcrypt 验证，带失败计数和锁定
  - `record_failure()` / `locked_for_secs()` — 失败计数与锁定（3次/5分钟）
  - `is_cipher_set()` — 检查是否已设置暗号
  - `clear_cipher()` — 清除暗号
- **配置文件**：`~/.one/remote_auth.json` — cipher_hash, failed_attempts, locked_until, max_failed_attempts, lock_duration_secs
- **验证**：`cargo build` 通过
- **完成时间**：2026-06-05

#### [ ] Task 4: 暗号设置 UI

- **涉及文件**：
  - `src/ui/dialogs.rs` 或 `src/ui/sidebar.rs` — 新增暗号设置弹窗
  - `src/app_state.rs` — 可能需要新增状态字段
- **UI 设计**：
  ```
  ┌─────────────────────────────────────┐
  │  远程暗号设置                          │
  ├─────────────────────────────────────┤
  │  暗号: [________________]            │
  │  确认暗号: [________________]        │
  │                                     │
  │  暗号状态: 未设置 / 已设置            │
  │  [设置] [修改] [清除]                 │
  │                                     │
  │  提示：暗号仅在本机设置，不经过网络      │
  └─────────────────────────────────────┘
  ```
- **验证**：手动验收 — 输入暗号 → 写入 remote_auth.json → 再次打开看到"已设置"
- **完成时间**：

---

### Phase 2: Telegram 绑定 + 状态机

#### [ ] Task 5: Config 结构体新增 Telegram 字段

- **涉及文件**：
  - `src/services/config.rs` — `Config` 新增 3 个字段（均加 `#[serde(default)]`）：
    - `telegram_bot_token: Option<String>`
    - `telegram_chat_id: Option<String>`
    - `telegram_bound_at: Option<String>`
- **验证**：`cargo build` 通过；旧 config.json 反序列化不报错
- **完成时间**：

#### [ ] Task 6: TelegramTrigger::from_config() + main.rs 更新

- **涉及文件**：
  - `src/triggers/telegram.rs` — 新增 `from_config(config: &Config) -> Option<Self>`
  - `src/main.rs` — 启动逻辑改为：
    ```rust
    TelegramTrigger::from_config(&config)
        .or_else(|| TelegramTrigger::from_env())
    ```
- **验证**：`cargo build` 通过；配置优先、无配置时 fallback 到环境变量
- **完成时间**：

#### [ ] Task 7: GPUI Telegram 绑定引导 UI

- **涉及文件**：
  - `src/ui/dialogs.rs` 或 `src/ui/sidebar.rs` — 绑定引导 UI
  - `src/app_state.rs` — 绑定相关状态
- **核心流程**：
  1. 用户输入 Bot Token → 点击"绑定 Telegram"
  2. 调 `getMe` 验证 Token 有效性
  3. 生成随机绑定码 → 提示用户在 TG 发含绑定码的消息
  4. `getUpdates` 匹配绑定码 → 提取 chat_id
  5. 写入 `config.json`（调 `save_config`）
  6. 显示"已绑定"状态
- **绑定码实现**：
  ```rust
  let bind_code = format!("ONE_BIND_{}_{:x}", 
      chrono::Local::now().format("%Y%m%d"), 
      rand::random::<u32>());
  ```
- **验证**：手动验收全流程
- **完成时间**：

#### [ ] Task 8: PendingConfirmation 状态机

- **涉及文件**：
  - `src/triggers/telegram.rs` — 主要改造
- **新增结构**：
  - `TelegramTrigger` 新增三个 `std::sync::Mutex` 字段：
    - `pending: Mutex<HashMap<i64, PendingConfirmation>>`
    - `current_workspace_id: Mutex<String>`
    - `current_task_id: Mutex<Option<String>>`
  - `PendingConfirmation` 结构体（含 `Drop` 守卫）
- **消息循环改造**：
  1. 收到消息 → 检查 `pending` 中是否有该 chat_id 的待确认项
  2. 有 → 当作暗号回复处理（调用 `RemoteAuth::verify_cipher`）
  3. 无 → 当作普通命令处理（`dispatch` + 检查回复标记）
  4. 超时检查：每次 poll 后扫描 `expires_at`，过期项自动取消
- **确认回复逻辑**：
  - Dangerous：暗号正确 → 执行 skill；错误 → 回复失败 + 计数
  - Extreme：暗号正确 → 等待本机弹窗结果 → 双通过才执行
- **验证**：`cargo build` 通过
- **完成时间**：

#### [ ] Task 9: dispatcher 危险等级路由 + TriggerReply 扩展

- **涉及文件**：
  - `src/triggers/mod.rs` — `TriggerReply` 扩展
  - `src/triggers/dispatcher.rs` — `run_skill` 判断 danger_level
  - `src/triggers/telegram.rs` — 适配新 `TriggerReply`
- **TriggerReply 扩展**：
  ```rust
  pub struct TriggerReply {
      pub text: String,
      pub needs_cipher: bool,         // 是否需要暗号确认
      pub danger_level: DangerLevel,  // 危险等级
  }
  ```
- **dispatcher 改动**：
  - `run_skill()` 查 `skill.manifest().danger_level`
  - Normal → 直接执行，`needs_cipher: false`
  - Dangerous → 返回需要确认标记，不执行
  - Extreme → 返回确认标记 + 本机弹窗需要
  - 修复 `skill.execute(args, None).await` 调用缺参问题
- **验证**：单元测试通过
- **完成时间**：

---

### Phase 3: 极度危险双确认

#### [ ] Task 10: Extreme 极度危险双确认协调

- **涉及文件**：
  - `src/agents/permission.rs` — 新增 `enqueue_detached()`
  - `src/triggers/telegram.rs` — Extreme 流程
- **permission.rs 新增**：
  ```rust
  pub fn enqueue_detached(kind: ToolKind, detail: String) -> Option<oneshot::Receiver<bool>>;
  ```
- **Extreme 协调流程**（`tokio::select!`）：
  ```rust
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
  ```
- **验证**：`cargo build` 通过
- **完成时间**：

---

### Phase 4: Workspace + Task 机制

#### [ ] Task 11: Workspace 切换命令

- **涉及文件**：
  - `src/triggers/dispatcher.rs` — 新增 `/workspace`、`/workspaces` 命令解析
  - `src/triggers/telegram.rs` — 切换逻辑 + current_workspace_id 更新
- **新增命令**：
  - `/workspace [name]` — 切换到指定 workspace（模糊匹配），不带参数显示当前
  - `/workspaces` — 列出所有 workspace
- **绑定时默认 workspace**：在 TG 绑定流程中关联
- **验证**：TG 中 `/workspace 工作区A` 能切换并回复确认
- **完成时间**：

#### [ ] Task 12: 远程 Task 创建和 Step 追加

- **涉及文件**：
  - `src/task_db.rs` — messages 表迁移；新增活跃 Task 查询接口
  - `src/triggers/telegram.rs` — Task 创建/追加逻辑
  - `src/triggers/dispatcher.rs` — `/tasks`、`/status`、`/clear` 命令
- **messages 表迁移**：
  ```sql
  ALTER TABLE messages ADD COLUMN step_index INTEGER DEFAULT 0;
  ALTER TABLE messages ADD COLUMN step_type TEXT DEFAULT 'user_message';
  ALTER TABLE messages ADD COLUMN skill_id TEXT;
  ```
- **Task 生命周期**：
  - 有 current_task_id → 追加到该 Task
  - 无 → 创建新 Task，设置 current_task_id
  - `/clear` → 清空 current_task_id（老 Task 保留）
- **新增命令**：`/status`、`/tasks`、`/clear`
- **验证**：TG 发消息后能在 DB 中看到对应 Task 和 Step
- **完成时间**：

---

### Phase 5: 测试

#### [ ] Task 13: 测试

- **单元测试**：
  - `RemoteAuth` — verify / lock / unlock 逻辑
  - `DangerLevel` — serde 向后兼容（旧 json 反序列化）
  - `dispatcher` — 危险等级路由（Normal 直接执行、Dangerous 返回标记）
  - `from_config` — 构造器 None/Some 分支
- **集成测试**：
  - 绑定引导 mock（telegram API mock）
  - PendingConfirmation 超时/取消
  - Extreme 双确认（local_tx + cipher 组合）
- **手动验收**：
  - Telegram → 发送 `/run` → 暗号 → 执行全链路
  - `cargo build` + `cargo test` 全绿
- **完成时间**：

---

## 进度日志

| 时间 | 任务 | 状态变更 | 备注 |
|------|------|---------|------|
| 2026-06-05 | Task 1 | ✅ 完成 | bcrypt v0.19.1 已添加到 Cargo.toml |
| 2026-06-05 | Task 2 | ✅ 完成 | DangerLevel 枚举 + SkillManifest 扩展。附带修复了 pre-existing 的 execute/request_async 签名调用问题 |
| 2026-06-05 | Task 3 | ✅ 完成 | RemoteAuth 模块 (src/agents/remote_auth.rs): bcrypt 哈希、失败计数/锁定(3次/5分钟) |
| 2026-06-05 | Task 4 | ✅ 完成 | 暗号设置 UI：导航栏按钮 + 设置对话框 |
| 2026-06-05 | Task 5 | ✅ 完成 | Config 新增 telegram_bot_token/telegram_chat_id/telegram_bound_at，均加 #[serde(default)] |
| 2026-06-05 | Task 6 | ✅ 完成 | TelegramTrigger::from_config() 构造器 + main.rs config 优先、env fallback |
| 2026-06-05 | Task 7 | ✅ 完成 | Telegram 绑定引导 UI（Token 输入 + getMe 验证 + 绑定码） |
| 2026-06-05 | Task 8 | ✅ 完成 | PendingConfirmation 状态机（Mutex 包装 + 暗号回复路由 + 超时清理） |
| 2026-06-05 | Task 9 | ✅ 完成 | TriggerReply 扩展（needs_cipher/danger_level）+ dispatcher 危险等级路由 |
| 2026-06-05 | Task 10 | ✅ 完成 | Extreme 双确认（enqueue_detached + handle_cipher_reply 协调） |
| 2026-06-05 | Task 11 | ✅ 完成 | Workspace 切换命令 (/workspace /workspaces) |
| 2026-06-05 | Task 12 | ✅ 完成 | 远程 Task 命令 (/status /tasks /clear) + messages 表扩展 |
| 2026-06-05 | Task 13 | ✅ 完成 | 40 tests passed, cargo build 通过 |

---

## 关键决策记录

| 日期 | 决策 | 选择 | 理由 |
|------|------|------|------|
| 2026-06-05 | TriggerReply 扩展方式 | 方案 A：结构化标记 | 避免文本匹配脆弱性 |
| 2026-06-05 | 绑定引导 chat_id 获取 | 绑定码验证 | 防止竞态拿到错误 chat_id |
| 2026-06-05 | RemoteAuth 模块位置 | src/agents/remote_auth.rs | 与 permission.rs 同级，模块分离 |
| 2026-06-05 | tasks.status 字段 | 不作特殊处理 | 当前是死代码，远程 Task 不需要 |