#![allow(dead_code)]

//! Telegram Bot trigger，靠 reqwest 长轮询 `getUpdates`。
//!
//! 流程：
//!   1. 启动时拉一个 1 分钟超时的 `getUpdates`，把 `offset` 滚动到下一个未读位置；
//!   2. 收到消息后先校验 chat_id 白名单，否则丢弃并日志告警；
//!   3. 若消息以 `/` 开头 → 送到 dispatcher 处理命令（/help /run /workspace 等）；
//!   4. 若消息不以 `/` 开头且有 workspace → 走 Orchestrator 路径（带记忆系统）；
//!   5. 若消息触发了暗号状态机 → 进入 cipher 回复处理；
//!   6. 通过 `sendMessage` 把 reply 回送给同一个 chat_id；
//!   7. 任意网络错误都打日志后短退避（5 秒）继续轮询，避免 hot loop。

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::Context;
use async_trait::async_trait;
use serde::Deserialize;

use super::{Trigger, TriggerEvent, TriggerReply};
use crate::agents::permission::DangerLevel;
use crate::services::Config;

const DEFAULT_API_BASE: &str = "https://api.telegram.org";
const LONG_POLL_TIMEOUT: u64 = 50;
const CIPHER_TIMEOUT_SECS: u64 = 120; // 2 分钟

/// 全局 Telegram trigger 停止信号。
/// 绑定新 token 前设置此信号，旧 trigger 检测到后优雅退出。
static TRIGGER_STOP_SIGNAL: AtomicBool = AtomicBool::new(false);

/// 当前运行的 trigger 数量，用于等待旧实例退出。
static TRIGGER_RUNNING_COUNT: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

// ============================================================================
// PendingConfirmation — 等待暗号回复的状态记录
// ============================================================================

struct PendingConfirmation {
    skill_id: String,
    args: serde_json::Value,
    workspace_id: String,
    task_id: String,
    danger_level: DangerLevel,
    needs_local_approval: bool,
    local_approval_rx: Option<tokio::sync::oneshot::Receiver<bool>>,
    expires_at: Instant,
}

impl Drop for PendingConfirmation {
    fn drop(&mut self) {
        // PendingConfirmation 持有 Receiver，Drop 时无需操作
    }
}

// ============================================================================
// TelegramTrigger
// ============================================================================

pub struct TelegramTrigger {
    token: String,
    api_base: String,
    allowed_chats: HashSet<i64>,
    client: reqwest::Client,
    // Mutex 包装的可变状态（run() 签名不可变）
    pending: Mutex<HashMap<i64, PendingConfirmation>>,
    current_workspace_id: Mutex<String>,
    current_task_id: Mutex<Option<String>>,
}

/// 确保给定 workspace 下有一个远程 Task，返回 task_id
fn ensure_remote_task(
    current_workspace_id: &Mutex<String>,
    current_task_id: &Mutex<Option<String>>,
) -> String {
    let workspace_id_str = current_workspace_id.lock().unwrap().clone();
    if workspace_id_str.is_empty() {
        return String::new();
    }
    let ws_id: usize = match workspace_id_str.parse() {
        Ok(id) => id,
        Err(_) => return String::new(),
    };

    {
        let tid = current_task_id.lock().unwrap().clone();
        if let Some(id) = tid {
            return id;
        }
    }

    let db_path = dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".one")
        .join("one.db");
    let conn = sqlez::connection::Connection::open_file(db_path.to_str().unwrap_or("one.db"));
    if let Ok(task_id) = crate::task_db::insert_remote_task(&conn, ws_id) {
        let id_str = task_id.to_string();
        *current_task_id.lock().unwrap() = Some(id_str.clone());
        id_str
    } else {
        String::new()
    }
}

/// 追加一条 user message 作为 step 到指定 Task
fn append_step_to_task(
    task_id: &str,
    role: &str,
    content: &str,
    step_type: &str,
    skill_id: Option<&str>,
) {
    let task_id_usize: usize = match task_id.parse() {
        Ok(id) => id,
        Err(_) => return,
    };
    let db_path = dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".one")
        .join("one.db");
    let conn = sqlez::connection::Connection::open_file(db_path.to_str().unwrap_or("one.db"));
    let count = crate::task_db::count_messages(&conn, task_id_usize).unwrap_or(0);
    let _ = crate::task_db::insert_message_step(
        &conn,
        task_id_usize,
        role,
        content,
        count as i64 + 1,
        step_type,
        skill_id,
    );
}

fn open_one_db() -> sqlez::connection::Connection {
    let db_path = dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".one")
        .join("one.db");
    sqlez::connection::Connection::open_file(db_path.to_str().unwrap_or("one.db"))
}

fn load_current_workspace(
    conn: &sqlez::connection::Connection,
    workspace_id: usize,
) -> Option<(String, std::path::PathBuf)> {
    crate::task_db::load_workspaces(conn)
        .ok()?
        .into_iter()
        .find(|workspace| workspace.id == workspace_id)
        .map(|workspace| (workspace.name, std::path::PathBuf::from(workspace.path)))
}

async fn handle_agent_command(trigger: &TelegramTrigger, chat_id: i64, text: &str) -> bool {
    let Some(rest) = text.strip_prefix("/agent") else {
        return false;
    };
    let rest = rest.trim();
    let mut parts = rest.splitn(3, ' ');
    let command = parts.next().unwrap_or_default();
    let arg1 = parts.next().unwrap_or_default();
    let arg2 = parts.next().unwrap_or_default();

    let conn = open_one_db();
    let workspace_id_str = trigger.current_workspace_id.lock().unwrap().clone();
    let workspace_id: usize = match workspace_id_str.parse() {
        Ok(id) => id,
        Err(_) => {
            let _ = trigger
                .send_message(chat_id, "请先发送 /workspace <name> 选择 workspace。")
                .await;
            return true;
        }
    };

    match command {
        "start" => {
            let Some(agent_kind) = crate::runtime::resolve_coding_agent_provider(arg1) else {
                let providers = crate::runtime::configured_coding_agent_usage();
                let _ = trigger
                    .send_message(
                        chat_id,
                        &format!("用法：/agent start {} [任务说明]", providers),
                    )
                    .await;
                return true;
            };
            let task_id_str =
                ensure_remote_task(&trigger.current_workspace_id, &trigger.current_task_id);
            let task_id: usize = match task_id_str.parse() {
                Ok(id) => id,
                Err(_) => {
                    let _ = trigger.send_message(chat_id, "创建远程 task 失败。").await;
                    return true;
                }
            };
            let Some((_workspace_name, cwd)) = load_current_workspace(&conn, workspace_id) else {
                let _ = trigger
                    .send_message(chat_id, "未找到当前 workspace。")
                    .await;
                return true;
            };
            let prompt = if arg2.trim().is_empty() {
                "请接手当前 task 的编码工作。先阅读当前 workspace，说明你准备如何推进；需要用户确认时停下来询问。"
                    .to_string()
            } else {
                arg2.trim().to_string()
            };
            let manager = crate::runtime::global_coding_session_manager();
            let result = manager
                .lock()
                .map_err(|_| "coding session manager lock poisoned".to_string())
                .and_then(|mut sessions| {
                    sessions
                        .start_session(
                            &conn,
                            task_id,
                            workspace_id,
                            agent_kind.clone(),
                            cwd.clone(),
                            true,
                            Some(&prompt),
                        )
                        .map_err(|error| error.to_string())
                });
            let reply = match result {
                Ok(session_id) => format!(
                    "{} 持久会话已启动：`{}`\ncwd={}",
                    agent_kind.label(),
                    session_id,
                    cwd.to_string_lossy()
                ),
                Err(error) => format!("启动失败：{}", error),
            };
            let _ = trigger.send_message(chat_id, &reply).await;
            true
        }
        "send" => {
            let text = if arg1.is_empty() {
                String::new()
            } else if arg2.is_empty() {
                arg1.to_string()
            } else {
                format!("{} {}", arg1, arg2)
            };
            if text.trim().is_empty() {
                let _ = trigger
                    .send_message(chat_id, "用法：/agent send <内容>")
                    .await;
                return true;
            }
            let task_id_str =
                ensure_remote_task(&trigger.current_workspace_id, &trigger.current_task_id);
            let task_id: usize = match task_id_str.parse() {
                Ok(id) => id,
                Err(_) => {
                    let _ = trigger.send_message(chat_id, "当前 task 无效。").await;
                    return true;
                }
            };
            let manager = crate::runtime::global_coding_session_manager();
            let result = manager
                .lock()
                .map_err(|_| "coding session manager lock poisoned".to_string())
                .and_then(|mut sessions| {
                    let session_id = sessions
                        .attached_session_id_for_task(task_id)
                        .ok_or_else(|| "当前 task 没有绑定 session。".to_string())?;
                    sessions
                        .send_input(&conn, &session_id, &text)
                        .map(|_| session_id)
                        .map_err(|error| error.to_string())
                });
            let reply = match result {
                Ok(session_id) => format!("已发送到 `{}`。", session_id),
                Err(error) => format!("发送失败：{}", error),
            };
            let _ = trigger.send_message(chat_id, &reply).await;
            true
        }
        "status" => {
            let task_id_str =
                ensure_remote_task(&trigger.current_workspace_id, &trigger.current_task_id);
            let task_id: usize = match task_id_str.parse() {
                Ok(id) => id,
                Err(_) => {
                    let _ = trigger.send_message(chat_id, "当前 task 无效。").await;
                    return true;
                }
            };
            let manager = crate::runtime::global_coding_session_manager();
            let reply = manager
                .lock()
                .map(|mut sessions| {
                    let Some(session_id) = sessions.attached_session_id_for_task(task_id) else {
                        return "当前 task 没有绑定 session。".to_string();
                    };
                    let output = sessions
                        .read_recent_output(&conn, &session_id, 30)
                        .unwrap_or_default()
                        .join("\n");
                    format!(
                        "session `{}` 最近输出：\n{}",
                        session_id,
                        if output.trim().is_empty() {
                            "(无可见输出)"
                        } else {
                            output.trim()
                        }
                    )
                })
                .unwrap_or_else(|_| "读取状态失败。".to_string());
            let _ = trigger.send_message(chat_id, &reply).await;
            true
        }
        "stop" => {
            let task_id_str =
                ensure_remote_task(&trigger.current_workspace_id, &trigger.current_task_id);
            let task_id: usize = match task_id_str.parse() {
                Ok(id) => id,
                Err(_) => {
                    let _ = trigger.send_message(chat_id, "当前 task 无效。").await;
                    return true;
                }
            };
            let manager = crate::runtime::global_coding_session_manager();
            let result = manager
                .lock()
                .map_err(|_| "coding session manager lock poisoned".to_string())
                .and_then(|mut sessions| {
                    let session_id = sessions
                        .attached_session_id_for_task(task_id)
                        .ok_or_else(|| "当前 task 没有绑定 session。".to_string())?;
                    sessions
                        .stop_session(&conn, &session_id)
                        .map(|_| session_id)
                        .map_err(|error| error.to_string())
                });
            let reply = match result {
                Ok(session_id) => format!("已停止 `{}`。", session_id),
                Err(error) => format!("停止失败：{}", error),
            };
            let _ = trigger.send_message(chat_id, &reply).await;
            true
        }
        "attach" => {
            let Ok(task_id) = arg1.parse::<usize>() else {
                let _ = trigger
                    .send_message(chat_id, "用法：/agent attach <task_id>")
                    .await;
                return true;
            };
            *trigger.current_task_id.lock().unwrap() = Some(task_id.to_string());
            let _ = trigger
                .send_message(chat_id, &format!("已 attach 到 task `{}`。", task_id))
                .await;
            true
        }
        "sessions" => {
            let manager = crate::runtime::global_coding_session_manager();
            let sessions = manager
                .lock()
                .map(|sessions| sessions.list_sessions())
                .unwrap_or_default();
            let reply = if sessions.is_empty() {
                "当前没有持久 coding session。".to_string()
            } else {
                sessions
                    .into_iter()
                    .map(|session| {
                        format!(
                            "- `{}` {} task={} workspace={} status={} cwd={}",
                            session.session_id,
                            session.agent_kind.label(),
                            session.task_id,
                            session.workspace_id,
                            session.status.label(),
                            session.cwd.to_string_lossy()
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            let _ = trigger.send_message(chat_id, &reply).await;
            true
        }
        _ => {
            let _ = trigger
                .send_message(
                    chat_id,
                    &format!(
                        "用法：/agent start {} [任务]；/agent send <内容>；/agent status；/agent stop；/agent attach <task_id>；/agent sessions",
                        crate::runtime::configured_coding_agent_usage()
                    ),
                )
                .await;
            true
        }
    }
}

impl TelegramTrigger {
    /// 停止正在运行的所有 Telegram trigger 实例。
    ///
    /// 仅设置全局停止信号，不阻塞等待。
    /// 旧实例会在完成当前 getUpdates poll 后检测到信号并自行退出（最长 ~55 秒）。
    /// 新实例立即启动，通过 getUpdates offset 机制保证不会重复消费消息。
    pub fn stop_all() {
        TRIGGER_STOP_SIGNAL.store(true, Ordering::SeqCst);
        log::info!("[telegram] 已设置停止信号，旧实例将在下次 poll 后退出");
    }

    pub fn spawn_in_background(config: &Config) {
        if let Some(trigger) = Self::from_config(config) {
            // 通知旧实例停止，但不等待（避免网络差时阻塞 UI）
            // 旧实例会在完成当前 getUpdates poll 后自行退出
            Self::stop_all();

            TRIGGER_RUNNING_COUNT.fetch_add(1, Ordering::Relaxed);
            std::thread::spawn(move || {
                let rt = match tokio::runtime::Runtime::new() {
                    Ok(rt) => rt,
                    Err(e) => {
                        log::error!("[telegram] 无法创建 tokio runtime: {:?}", e);
                        TRIGGER_RUNNING_COUNT.fetch_sub(1, Ordering::Relaxed);
                        return;
                    }
                };
                rt.block_on(async move {
                    if let Err(e) = trigger.run().await {
                        log::error!("[telegram] trigger 退出：{:?}", e);
                    }
                });
                TRIGGER_RUNNING_COUNT.fetch_sub(1, Ordering::Relaxed);
            });
        }
    }

    /// 从配置文件构造；缺 token 或 chat_id 时返回 None。
    pub fn from_config(config: &Config) -> Option<Self> {
        let token = config.telegram_bot_token.clone()?;
        let chat_id_str = config.telegram_chat_id.clone()?;
        let chat_id: i64 = chat_id_str.parse().ok()?;
        let mut allowed_chats = HashSet::new();
        allowed_chats.insert(chat_id);
        let api_base = DEFAULT_API_BASE.to_string();
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(LONG_POLL_TIMEOUT + 10))
            .build()
            .ok()?;
        Some(Self {
            token,
            api_base,
            allowed_chats,
            client,
            pending: Mutex::new(HashMap::new()),
            current_workspace_id: Mutex::new(String::new()),
            current_task_id: Mutex::new(None),
        })
    }

    /// 从环境变量构造；缺 token 或 allowlist 时返回 None，让上层静默禁用此 trigger。
    pub fn from_env() -> Option<Self> {
        let token = std::env::var("ONE_TELEGRAM_BOT_TOKEN").ok()?;
        let allowed_raw = std::env::var("ONE_TELEGRAM_ALLOWED_CHATS").ok()?;
        let allowed_chats: HashSet<i64> = allowed_raw
            .split(',')
            .filter_map(|s| s.trim().parse::<i64>().ok())
            .collect();
        if allowed_chats.is_empty() {
            log::warn!("[telegram] ONE_TELEGRAM_ALLOWED_CHATS is empty/invalid; trigger disabled.");
            return None;
        }
        let api_base =
            std::env::var("ONE_TELEGRAM_API_BASE").unwrap_or_else(|_| DEFAULT_API_BASE.to_string());
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(LONG_POLL_TIMEOUT + 10))
            .build()
            .ok()?;
        Some(Self {
            token,
            api_base,
            allowed_chats,
            client,
            pending: Mutex::new(HashMap::new()),
            current_workspace_id: Mutex::new(String::new()),
            current_task_id: Mutex::new(None),
        })
    }

    fn url(&self, method: &str) -> String {
        format!("{}/bot{}/{}", self.api_base, self.token, method)
    }

    async fn poll_updates(&self, offset: i64) -> anyhow::Result<Vec<Update>> {
        let resp: TgResponse<Vec<Update>> = self
            .client
            .post(self.url("getUpdates"))
            .json(&serde_json::json!({
                "offset": offset,
                "timeout": LONG_POLL_TIMEOUT,
                "allowed_updates": ["message"],
            }))
            .send()
            .await
            .context("getUpdates send")?
            .error_for_status()
            .context("getUpdates http status")?
            .json()
            .await
            .context("getUpdates decode")?;
        if !resp.ok {
            anyhow::bail!("getUpdates not ok: {:?}", resp.description);
        }
        Ok(resp.result.unwrap_or_default())
    }

    async fn send_message(&self, chat_id: i64, text: &str) -> anyhow::Result<()> {
        self.client
            .post(self.url("sendMessage"))
            .json(&serde_json::json!({
                "chat_id": chat_id,
                "text": text,
            }))
            .send()
            .await
            .context("sendMessage send")?
            .error_for_status()
            .context("sendMessage http status")?;
        Ok(())
    }

    /// 清除过期的待确认项
    fn sweep_expired(&self) {
        let now = Instant::now();
        if let Ok(mut pending) = self.pending.lock() {
            pending.retain(|_, pc| {
                if pc.expires_at > now {
                    true
                } else {
                    log::info!(
                        "[telegram] pending confirmation expired for skill={}",
                        pc.skill_id
                    );
                    false
                }
            });
        }
    }

    /// 处理来自 chat_id 的暗号回复
    async fn handle_cipher_reply(
        &self,
        chat_id: i64,
        cipher_text: &str,
    ) -> anyhow::Result<Option<String>> {
        let pc = {
            let mut pending = self.pending.lock().unwrap();
            pending.remove(&chat_id)
        };

        let Some(mut pc) = pc else {
            return Ok(None);
        };

        if Instant::now() > pc.expires_at {
            return Ok(Some("确认超时，操作已取消。".to_string()));
        }

        match crate::agents::remote_auth::RemoteAuth::verify_cipher(cipher_text) {
            Ok(true) => {
                if pc.danger_level == DangerLevel::Extreme {
                    if let Some(local_rx) = &mut pc.local_approval_rx {
                        let local_result = tokio::time::timeout(
                            Duration::from_secs(CIPHER_TIMEOUT_SECS),
                            local_rx,
                        )
                        .await;
                        match local_result {
                            Ok(Ok(true)) => {}
                            Ok(Ok(false)) => {
                                return Ok(Some(
                                    "⚠️ 双确认失败：本机拒绝了操作，操作已取消。".to_string(),
                                ));
                            }
                            Ok(Err(_)) => {
                                return Ok(Some(
                                    "⚠️ 双确认失败：审批通道异常，操作已取消。".to_string(),
                                ));
                            }
                            Err(_) => {
                                return Ok(Some("⚠️ 双确认超时，操作已取消。".to_string()));
                            }
                        }
                    }
                }

                let skill = crate::skills::find_skill(&pc.skill_id);
                let skill_id = pc.skill_id.clone();
                let result_text = match skill {
                    Some(skill) => {
                        let _guard = crate::agents::permission::RemoteScopeGuard::enter();
                        let args = std::mem::take(&mut pc.args);
                        match skill.execute(args, None).await {
                            Ok(exec) => {
                                let mut out = format!(
                                    "✅ 暗号验证通过！\n[run] {}\n{}\n",
                                    skill_id, exec.summary
                                );
                                if exec.freed_bytes > 0 {
                                    out.push_str(&format!("释放 {} 字节\n", exec.freed_bytes));
                                }
                                if !exec.success_items.is_empty() {
                                    out.push_str(&format!(
                                        "成功 {} 项\n",
                                        exec.success_items.len()
                                    ));
                                }
                                if !exec.failed_items.is_empty() {
                                    out.push_str(&format!("失败 {} 项\n", exec.failed_items.len()));
                                }
                                out
                            }
                            Err(e) => format!("execute 失败：{}", e),
                        }
                    }
                    None => format!("未找到 skill_id：{}", pc.skill_id),
                };
                Ok(Some(result_text))
            }
            Ok(false) => Ok(Some("暗号验证失败，请重试。".to_string())),
            Err(msg) => Ok(Some(msg)),
        }
    }
}

#[async_trait]
impl Trigger for TelegramTrigger {
    fn name(&self) -> &str {
        "telegram"
    }

    async fn run(&self) -> anyhow::Result<()> {
        log::info!(
            "[telegram] trigger started; allowed_chats={:?}",
            self.allowed_chats
        );
        let mut offset: i64 = 0;
        loop {
            // 检查全局停止信号
            if TRIGGER_STOP_SIGNAL.load(Ordering::SeqCst) {
                log::info!("[telegram] 收到停止信号，trigger 退出");
                return Ok(());
            }

            self.sweep_expired();
            match self.poll_updates(offset).await {
                Ok(updates) => {
                    for update in updates {
                        offset = update.update_id + 1;
                        let Some(message) = update.message else {
                            continue;
                        };
                        let chat_id = message.chat.id;
                        let text = message.text.unwrap_or_default();
                        if text.is_empty() {
                            continue;
                        }
                        if !self.allowed_chats.contains(&chat_id) {
                            log::warn!(
                                "[telegram] chat_id {} not in allowlist; dropping message",
                                chat_id
                            );
                            let _ = self
                                .send_message(chat_id, "未授权：此 chat_id 不在白名单中。")
                                .await;
                            continue;
                        }

                        // 检查是否是暗号回复
                        if self.pending.lock().unwrap().contains_key(&chat_id) {
                            // 预检：暗号应该短（<=32字符）、无空格、无特殊标点
                            let looks_like_cipher = text.len() <= 32
                                && !text.contains(' ')
                                && text
                                    .chars()
                                    .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
                                && !text.starts_with('/');
                            if !looks_like_cipher {
                                let _ = self
                                    .send_message(
                                        chat_id,
                                        "⏳ 正在等待暗号确认，请发送暗号或 /cancel 取消。",
                                    )
                                    .await;
                                continue;
                            }
                            // 支持 /cancel 取消暗号状态机
                            if text == "/cancel" {
                                self.pending.lock().unwrap().remove(&chat_id);
                                let _ = self.send_message(chat_id, "暗号确认已取消。").await;
                                continue;
                            }
                            match self.handle_cipher_reply(chat_id, &text).await {
                                Ok(Some(reply_text)) => {
                                    let _ = self.send_message(chat_id, &reply_text).await;
                                }
                                Ok(None) => {}
                                Err(e) => {
                                    log::error!("[telegram] handle_cipher_reply error: {:?}", e);
                                }
                            }
                            continue;
                        }

                        // ── 非命令消息：走 Orchestrator 路径（带记忆系统） ──
                        //
                        // 命令以 `/` 开头（/help, /run, /workspace 等），走 dispatcher。
                        // 非命令消息且有 workspace → 在 Telegram 线程内直接跑 Orchestrator。
                        if !text.starts_with('/') {
                            let ws_id_str = self.current_workspace_id.lock().unwrap().clone();
                            if ws_id_str.is_empty() {
                                let _ = self
                                    .send_message(
                                        chat_id,
                                        "请先发送 /workspace <name> 切换到 workspace 后再发消息。",
                                    )
                                    .await;
                                continue;
                            }

                            // 1. 确保有远程 task
                            let task_id_str = ensure_remote_task(
                                &self.current_workspace_id,
                                &self.current_task_id,
                            );
                            if task_id_str.is_empty() {
                                let _ = self
                                    .send_message(
                                        chat_id,
                                        "创建远程任务失败，请先在本机创建 workspace。",
                                    )
                                    .await;
                                continue;
                            }
                            let task_id_usize: usize = match task_id_str.parse() {
                                Ok(id) => id,
                                Err(_) => {
                                    let _ = self.send_message(chat_id, "任务 ID 无效。").await;
                                    continue;
                                }
                            };

                            // 2. 写用户消息到 DB
                            append_step_to_task(&task_id_str, "user", &text, "user_message", None);

                            // 3. 从 DB 加载历史消息
                            let db_path = dirs::config_dir()
                                .unwrap_or_else(|| std::path::PathBuf::from("."))
                                .join(".one")
                                .join("one.db");
                            let conn = sqlez::connection::Connection::open_file(
                                db_path.to_str().unwrap_or("one.db"),
                            );
                            let db_messages = crate::task_db::load_messages(&conn, task_id_usize)
                                .unwrap_or_default();
                            let history: Vec<crate::memory::types::ChatMessage> = db_messages
                                .into_iter()
                                .map(|m| {
                                    crate::memory::types::ChatMessage::new(&m.role, &m.content)
                                })
                                .collect();

                            // 4. 获取 workspace 信息
                            let ws_result = crate::task_db::load_workspaces(&conn);
                            let (ws_name, ws_path) = match ws_result {
                                Ok(workspaces) => {
                                    let ws_id: usize = ws_id_str.parse().unwrap_or(0);
                                    if let Some(ws) = workspaces.iter().find(|w| w.id == ws_id) {
                                        (ws.name.clone(), std::path::PathBuf::from(&ws.path))
                                    } else {
                                        ("Default".to_string(), std::path::PathBuf::from("."))
                                    }
                                }
                                Err(_) => ("Default".to_string(), std::path::PathBuf::from(".")),
                            };

                            // 5. 发送"正在处理"提示
                            let _ = self.send_message(chat_id, "⏳ 正在处理，请稍候...").await;

                            // 6. 在 Telegram 线程跑 Orchestrator
                            let config = crate::services::load_config();
                            match crate::agents::core::factory::AgentFactory::create_orchestrator(
                                &config,
                                &ws_name,
                                ws_path,
                                crate::mcp::global_manager(),
                            ) {
                                Ok(orchestrator) => {
                                    let session_id = format!("telegram-task-{}", task_id_str);
                                    let event_replies =
                                        std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
                                    let event_replies_for_run = event_replies.clone();
                                    let ws_path_for_events = match load_current_workspace(
                                        &conn,
                                        ws_id_str.parse().unwrap_or(0),
                                    ) {
                                        Some((_, path)) => path,
                                        None => std::path::PathBuf::from("."),
                                    };
                                    let workspace_id_for_events: usize =
                                        ws_id_str.parse().unwrap_or(0);
                                    let task_id_for_events = task_id_usize;

                                    // 注意：user_input_rx=None → 不支持多轮追问
                                    let result = orchestrator
                                        .run_task(
                                            &text,
                                            session_id,
                                            history,
                                            &ws_name,
                                            Some(task_id_usize),
                                            None, // cancel_flag=None
                                            None, // user_input_rx=None => 不支持多轮
                                            move |event| {
                                                use crate::agents::core::OrchestratorEvent;
                                                let manager =
                                                    crate::runtime::global_coding_session_manager();
                                                let conn = open_one_db();
                                                let mut push_reply = |reply: String| {
                                                    if let Ok(mut replies) =
                                                        event_replies_for_run.lock()
                                                    {
                                                        replies.push(reply);
                                                    }
                                                };
                                                match event {
                                                    OrchestratorEvent::StartCodingSession {
                                                        agent_kind,
                                                        prompt,
                                                        write_mode,
                                                    } => {
                                                        let kind = crate::runtime::resolve_coding_agent_provider(&agent_kind)
                                                            .unwrap_or_else(crate::runtime::default_coding_agent_provider);
                                                        let result = manager
                                                            .lock()
                                                            .map_err(|_| "coding session manager lock poisoned".to_string())
                                                            .and_then(|mut sessions| {
                                                                sessions.start_session(
                                                                    &conn,
                                                                    task_id_for_events,
                                                                    workspace_id_for_events,
                                                                    kind.clone(),
                                                                    ws_path_for_events.clone(),
                                                                    write_mode,
                                                                    Some(&prompt),
                                                                ).map_err(|error| error.to_string())
                                                            });
                                                        match result {
                                                            Ok(session_id) => push_reply(format!("{} 持久会话已启动：`{}`。", kind.label(), session_id)),
                                                            Err(error) => push_reply(format!("启动 coding session 失败：{}", error)),
                                                        }
                                                    }
                                                    OrchestratorEvent::SendToCodingSession {
                                                        session_id,
                                                        text,
                                                    } => {
                                                        let result = manager
                                                            .lock()
                                                            .map_err(|_| "coding session manager lock poisoned".to_string())
                                                            .and_then(|mut sessions| {
                                                                let id = match session_id {
                                                                    Some(id) => id,
                                                                    None => sessions.attached_session_id_for_task(task_id_for_events)
                                                                        .ok_or_else(|| "当前 task 没有绑定 session。".to_string())?,
                                                                };
                                                                sessions.send_input(&conn, &id, &text)
                                                                    .map(|_| id)
                                                                    .map_err(|error| error.to_string())
                                                            });
                                                        match result {
                                                            Ok(id) => push_reply(format!("已发送到 coding session `{}`。", id)),
                                                            Err(error) => push_reply(format!("发送到 coding session 失败：{}", error)),
                                                        }
                                                    }
                                                    OrchestratorEvent::ReadCodingSessionOutput {
                                                        session_id,
                                                        limit,
                                                    } => {
                                                        let result = manager
                                                            .lock()
                                                            .map_err(|_| "coding session manager lock poisoned".to_string())
                                                            .and_then(|mut sessions| {
                                                                let id = match session_id {
                                                                    Some(id) => id,
                                                                    None => sessions.attached_session_id_for_task(task_id_for_events)
                                                                        .ok_or_else(|| "当前 task 没有绑定 session。".to_string())?,
                                                                };
                                                                sessions.read_recent_output(&conn, &id, limit)
                                                                    .map(|lines| (id, lines))
                                                                    .map_err(|error| error.to_string())
                                                            });
                                                        match result {
                                                            Ok((id, lines)) => push_reply(format!("coding session `{}` 最近输出：\n{}", id, lines.join("\n"))),
                                                            Err(error) => push_reply(format!("读取 coding session 输出失败：{}", error)),
                                                        }
                                                    }
                                                    OrchestratorEvent::StopCodingSession {
                                                        session_id,
                                                    } => {
                                                        let result = manager
                                                            .lock()
                                                            .map_err(|_| "coding session manager lock poisoned".to_string())
                                                            .and_then(|mut sessions| {
                                                                let id = match session_id {
                                                                    Some(id) => id,
                                                                    None => sessions.attached_session_id_for_task(task_id_for_events)
                                                                        .ok_or_else(|| "当前 task 没有绑定 session。".to_string())?,
                                                                };
                                                                sessions.stop_session(&conn, &id)
                                                                    .map(|_| id)
                                                                    .map_err(|error| error.to_string())
                                                            });
                                                        match result {
                                                            Ok(id) => push_reply(format!("已停止 coding session `{}`。", id)),
                                                            Err(error) => push_reply(format!("停止 coding session 失败：{}", error)),
                                                        }
                                                    }
                                                    OrchestratorEvent::ListCodingSessions => {
                                                        let sessions = manager
                                                            .lock()
                                                            .map(|sessions| sessions.list_sessions())
                                                            .unwrap_or_default();
                                                        let reply = if sessions.is_empty() {
                                                            "当前没有持久 coding session。".to_string()
                                                        } else {
                                                            sessions
                                                                .into_iter()
                                                                .map(|session| {
                                                                    format!(
                                                                        "- `{}` {} task={} workspace={} status={}",
                                                                        session.session_id,
                                                                        session.agent_kind.label(),
                                                                        session.task_id,
                                                                        session.workspace_id,
                                                                        session.status.label()
                                                                    )
                                                                })
                                                                .collect::<Vec<_>>()
                                                                .join("\n")
                                                        };
                                                        push_reply(reply);
                                                    }
                                                    OrchestratorEvent::GetWorkspaceWriteStatus => {
                                                        let reply = manager
                                                            .lock()
                                                            .ok()
                                                            .and_then(|sessions| {
                                                                sessions.active_write_session_for_workspace(workspace_id_for_events)
                                                                    .map(|session| format!("写锁由 `{}` {} 持有。", session.session_id, session.agent_kind.label()))
                                                            })
                                                            .unwrap_or_else(|| "当前 workspace 没有 write-active coding session。".to_string());
                                                        push_reply(reply);
                                                    }
                                                    _ => {}
                                                }
                                            },
                                        )
                                        .await;

                                    match result {
                                        Ok(reply_text) => {
                                            let cleaned =
                                                crate::util::strip_think_tags(&reply_text);
                                            let event_text = event_replies
                                                .lock()
                                                .map(|replies| replies.join("\n\n"))
                                                .unwrap_or_default();
                                            if !event_text.trim().is_empty() {
                                                let _ =
                                                    self.send_message(chat_id, &event_text).await;
                                            }
                                            // 7. 写 AI 回复到 DB
                                            append_step_to_task(
                                                &task_id_str,
                                                "assistant",
                                                &cleaned,
                                                "ai_reply",
                                                None,
                                            );
                                            // 8. 回送
                                            let _ = self.send_message(chat_id, &cleaned).await;
                                        }
                                        Err(e) => {
                                            let err_msg = format!("处理失败：{}", e);
                                            let _ = self.send_message(chat_id, &err_msg).await;
                                        }
                                    }
                                }
                                Err(e) => {
                                    let err_msg = format!("无法创建 Orchestrator：{}", e);
                                    let _ = self.send_message(chat_id, &err_msg).await;
                                }
                            }
                            continue;
                        }

                        if handle_agent_command(self, chat_id, &text).await {
                            continue;
                        }

                        let reply: TriggerReply = super::dispatch(&text).await;

                        // 处理 workspace 切换
                        if text.starts_with("/workspace ") {
                            let name = text.trim_start_matches("/workspace ").trim();
                            if !name.is_empty() {
                                let db_path = dirs::config_dir()
                                    .unwrap_or_else(|| std::path::PathBuf::from("."))
                                    .join(".one")
                                    .join("one.db");
                                let conn = sqlez::connection::Connection::open_file(
                                    db_path.to_str().unwrap_or("one.db"),
                                );
                                let ws_result = crate::task_db::load_workspaces(&conn);
                                let workspaces = ws_result.unwrap_or_default();
                                let matched = workspaces
                                    .iter()
                                    .find(|w| w.name == name)
                                    .or_else(|| workspaces.iter().find(|w| w.name.contains(name)));
                                if let Some(ws) = matched {
                                    *self.current_workspace_id.lock().unwrap() = ws.id.to_string();
                                    *self.current_task_id.lock().unwrap() = None; // ✅ 清空旧 task
                                    let _ = self
                                        .send_message(
                                            chat_id,
                                            &format!("已切换到 workspace「{}」", ws.name),
                                        )
                                        .await;
                                } else {
                                    let _ = self
                                        .send_message(
                                            chat_id,
                                            &format!(
                                                "未找到 workspace「{}」，请先在本机创建",
                                                name
                                            ),
                                        )
                                        .await;
                                }
                            }
                            continue;
                        }

                        if text == "/clear" {
                            *self.current_task_id.lock().unwrap() = None;
                            let _ = self
                                .send_message(chat_id, "远程任务已结束。下次消息将创建新任务。")
                                .await;
                            continue;
                        }

                        if reply.needs_cipher {
                            let skill_id = text
                                .strip_prefix("/run ")
                                .and_then(|rest| rest.split_whitespace().next())
                                .unwrap_or("unknown")
                                .to_string();

                            let local_approval_rx = if reply.danger_level == DangerLevel::Extreme {
                                crate::agents::permission::enqueue_detached(
                                    crate::agents::permission::ToolKind::Shell,
                                    format!("远程 Extreme 操作「{}」请求双重确认", skill_id),
                                )
                            } else {
                                None
                            };

                            let pc = PendingConfirmation {
                                skill_id,
                                args: serde_json::Value::Object(Default::default()),
                                workspace_id: self.current_workspace_id.lock().unwrap().clone(),
                                task_id: self
                                    .current_task_id
                                    .lock()
                                    .unwrap()
                                    .clone()
                                    .unwrap_or_default(),
                                danger_level: reply.danger_level,
                                needs_local_approval: reply.danger_level == DangerLevel::Extreme,
                                local_approval_rx,
                                expires_at: Instant::now()
                                    + Duration::from_secs(CIPHER_TIMEOUT_SECS),
                            };

                            {
                                let mut pending = self.pending.lock().unwrap();
                                pending.insert(chat_id, pc);
                            }

                            if let Err(err) = self.send_message(chat_id, &reply.text).await {
                                log::error!("[telegram] sendMessage failed: {:?}", err);
                            }
                        } else {
                            if let Err(err) = self.send_message(chat_id, &reply.text).await {
                                log::error!("[telegram] sendMessage failed: {:?}", err);
                            }
                        }
                    }
                }
                Err(err) => {
                    log::error!("[telegram] poll error: {:?}", err);
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }
    }
}

// ============================================================================
// Telegram API 响应结构体

#[derive(Debug, Deserialize)]
struct TgResponse<T> {
    ok: bool,
    description: Option<String>,
    result: Option<T>,
}

#[derive(Debug, Deserialize)]
struct Update {
    update_id: i64,
    message: Option<Message>,
}

#[derive(Debug, Deserialize)]
struct Message {
    chat: Chat,
    text: Option<String>,
    from: Option<User>,
}

#[derive(Debug, Deserialize)]
struct Chat {
    id: i64,
}

#[derive(Debug, Deserialize)]
struct User {
    id: i64,
    username: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_env_returns_none_without_token() {
        std::env::remove_var("ONE_TELEGRAM_BOT_TOKEN");
        std::env::remove_var("ONE_TELEGRAM_ALLOWED_CHATS");
        assert!(TelegramTrigger::from_env().is_none());
    }

    #[test]
    fn pending_drop_does_not_panic() {
        {
            let pc = PendingConfirmation {
                skill_id: "test".to_string(),
                args: serde_json::Value::Null,
                workspace_id: "".to_string(),
                task_id: "".to_string(),
                danger_level: DangerLevel::Dangerous,
                needs_local_approval: false,
                local_approval_rx: None,
                expires_at: Instant::now() + Duration::from_secs(120),
            };
            // pc 的 Drop 不应该 panic
        }
    }
}
