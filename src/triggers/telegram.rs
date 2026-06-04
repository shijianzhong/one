#![allow(dead_code)]

//! Telegram Bot trigger，靠 reqwest 长轮询 `getUpdates`。
//!
//! 配置：
//!   * `ONE_TELEGRAM_BOT_TOKEN`         — Bot 父 token，缺省时 trigger 不启动
//!   * `ONE_TELEGRAM_ALLOWED_CHATS`     — 逗号分隔 chat_id 白名单（必须）
//!   * `ONE_TELEGRAM_API_BASE`          — 可选，默认 https://api.telegram.org
//!
//! 流程：
//!   1. 启动时拉一个 1 分钟超时的 `getUpdates`，把 `offset` 滚动到下一个未读位置；
//!   2. 收到消息后先校验 chat_id 白名单，否则丢弃并日志告警；
//!   3. 把消息文本送到 [`crate::triggers::dispatch`]，得到 [`TriggerReply`]；
//!   4. 通过 `sendMessage` 把 reply 回送给同一个 chat_id；
//!   5. 任意网络错误都打日志后短退避（5 秒）继续轮询，避免 hot loop。

use std::collections::HashSet;
use std::time::Duration;

use anyhow::Context;
use async_trait::async_trait;
use serde::Deserialize;

use super::{Trigger, TriggerEvent, TriggerReply};

const DEFAULT_API_BASE: &str = "https://api.telegram.org";
const LONG_POLL_TIMEOUT: u64 = 50;

pub struct TelegramTrigger {
    token: String,
    api_base: String,
    allowed_chats: HashSet<i64>,
    client: reqwest::Client,
}

impl TelegramTrigger {
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
            match self.poll_updates(offset).await {
                Ok(updates) => {
                    for update in updates {
                        offset = update.update_id + 1;
                        let Some(message) = update.message else { continue };
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
                                .send_message(
                                    chat_id,
                                    "未授权：此 chat_id 不在白名单中。",
                                )
                                .await;
                            continue;
                        }
                        let event = TriggerEvent {
                            source: "telegram".to_string(),
                            chat_id: chat_id.to_string(),
                            user: message
                                .from
                                .as_ref()
                                .map(|u| u.username.clone().unwrap_or_else(|| u.id.to_string())),
                            text: text.clone(),
                        };
                        let reply: TriggerReply = super::dispatch(&event.text).await;
                        if let Err(err) = self.send_message(chat_id, &reply.text).await {
                            log::error!("[telegram] sendMessage failed: {:?}", err);
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
}
