#![allow(dead_code)]

//! Telegram Bot trigger，靠 reqwest 长轮询 `getUpdates`。
//!
//! 流程：
//!   1. 启动时拉一个 1 分钟超时的 `getUpdates`，把 `offset` 滚动到下一个未读位置；
//!   2. 收到消息后先校验 chat_id 白名单，否则丢弃并日志告警；
//!   3. 把消息文本送到 [`crate::triggers::dispatch`]，得到 [`TriggerReply`]；
//!   4. 如果 reply 标记了需要暗号确认，将该 chat 的后续消息视为暗号回复，进入状态机；
//!   5. 通过 `sendMessage` 把 reply 回送给同一个 chat_id；
//!   6. 任意网络错误都打日志后短退避（5 秒）继续轮询，避免 hot loop。

use std::collections::HashMap;
use std::collections::HashSet;
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
    let conn =
        sqlez::connection::Connection::open_file(db_path.to_str().unwrap_or("one.db"));
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
    let conn =
        sqlez::connection::Connection::open_file(db_path.to_str().unwrap_or("one.db"));
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

impl TelegramTrigger {
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
            log::warn!(
                "[telegram] ONE_TELEGRAM_ALLOWED_CHATS is empty/invalid; trigger disabled."
            );
            return None;
        }
        let api_base = std::env::var("ONE_TELEGRAM_API_BASE")
            .unwrap_or_else(|_| DEFAULT_API_BASE.to_string());
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
                                    "⚠️ 双确认失败：本机拒绝了操作，操作已取消。"
                                        .to_string(),
                                ));
                            }
                            Ok(Err(_)) => {
                                return Ok(Some(
                                    "⚠️ 双确认失败：审批通道异常，操作已取消。"
                                        .to_string(),
                                ));
                            }
                            Err(_) => {
                                return Ok(Some(
                                    "⚠️ 双确认超时，操作已取消。".to_string(),
                                ));
                            }
                        }
                    }
                }

                let skill = crate::skills::registry().find(&pc.skill_id);
                let skill_id = pc.skill_id.clone();
                let result_text = match skill {
                    Some(skill) => {
                        let _guard =
                            crate::agents::permission::RemoteScopeGuard::enter();
                        let args = std::mem::take(&mut pc.args);
                        match skill.execute(args, None).await {
                            Ok(exec) => {
                                let mut out = format!(
                                    "✅ 暗号验证通过！\n[run] {}\n{}\n",
                                    skill_id, exec.summary
                                );
                                if exec.freed_bytes > 0 {
                                    out.push_str(&format!(
                                        "释放 {} 字节\n",
                                        exec.freed_bytes
                                    ));
                                }
                                if !exec.success_items.is_empty() {
                                    out.push_str(&format!(
                                        "成功 {} 项\n",
                                        exec.success_items.len()
                                    ));
                                }
                                if !exec.failed_items.is_empty() {
                                    out.push_str(&format!(
                                        "失败 {} 项\n",
                                        exec.failed_items.len()
                                    ));
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
                            match self.handle_cipher_reply(chat_id, &text).await {
                                Ok(Some(reply_text)) => {
                                    let _ =
                                        self.send_message(chat_id, &reply_text).await;
                                }
                                Ok(None) => {}
                                Err(e) => {
                                    log::error!(
                                        "[telegram] handle_cipher_reply error: {:?}",
                                        e
                                    );
                                }
                            }
                            continue;
                        }

                        let reply: TriggerReply = super::dispatch(&text).await;

                        // 处理 workspace 切换
                        if text.starts_with("/workspace ") {
                            let name =
                                text.trim_start_matches("/workspace ").trim();
                            if !name.is_empty() {
                                let db_path = dirs::config_dir()
                                    .unwrap_or_else(|| std::path::PathBuf::from("."))
                                    .join(".one")
                                    .join("one.db");
                                let conn = sqlez::connection::Connection::open_file(
                                    db_path.to_str().unwrap_or("one.db"),
                                );
                                let ws_result =
                                    crate::task_db::load_workspaces(&conn);
                                let workspaces = ws_result.unwrap_or_default();
                                let matched = workspaces
                                    .iter()
                                    .find(|w| w.name == name)
                                    .or_else(|| {
                                        workspaces
                                            .iter()
                                            .find(|w| w.name.contains(name))
                                    });
                                if let Some(ws) = matched {
                                    *self.current_workspace_id.lock().unwrap() =
                                        ws.id.to_string();
                                    let _ = self
                                        .send_message(
                                            chat_id,
                                            &format!(
                                                "已切换到 workspace「{}」",
                                                ws.name
                                            ),
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
                                .send_message(
                                    chat_id,
                                    "远程任务已结束。下次消息将创建新任务。",
                                )
                                .await;
                            continue;
                        }

                        if reply.needs_cipher {
                            let skill_id = text
                                .strip_prefix("/run ")
                                .and_then(|rest| rest.split_whitespace().next())
                                .unwrap_or("unknown")
                                .to_string();

                            let local_approval_rx = if reply.danger_level
                                == DangerLevel::Extreme
                            {
                                crate::agents::permission::enqueue_detached(
                                    crate::agents::permission::ToolKind::Shell,
                                    format!(
                                        "远程 Extreme 操作「{}」请求双重确认",
                                        skill_id
                                    ),
                                )
                            } else {
                                None
                            };

                            let pc = PendingConfirmation {
                                skill_id,
                                args: serde_json::Value::Object(Default::default()),
                                workspace_id: self
                                    .current_workspace_id
                                    .lock()
                                    .unwrap()
                                    .clone(),
                                task_id: self
                                    .current_task_id
                                    .lock()
                                    .unwrap()
                                    .clone()
                                    .unwrap_or_default(),
                                danger_level: reply.danger_level,
                                needs_local_approval: reply.danger_level
                                    == DangerLevel::Extreme,
                                local_approval_rx,
                                expires_at: Instant::now()
                                    + Duration::from_secs(CIPHER_TIMEOUT_SECS),
                            };

                            {
                                let mut pending = self.pending.lock().unwrap();
                                pending.insert(chat_id, pc);
                            }

                            if let Err(err) =
                                self.send_message(chat_id, &reply.text).await
                            {
                                log::error!(
                                    "[telegram] sendMessage failed: {:?}",
                                    err
                                );
                            }
                        } else {
                            if let Err(err) =
                                self.send_message(chat_id, &reply.text).await
                            {
                                log::error!(
                                    "[telegram] sendMessage failed: {:?}",
                                    err
                                );
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