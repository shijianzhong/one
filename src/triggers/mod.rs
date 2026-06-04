#![allow(dead_code)]

//! Remote trigger layer (M3).
//!
//! 每个 `Trigger` 在后台 tokio 任务里拉取自己平台的消息（Telegram 长轮询、
//! Webhook、Hotkey…），把消息归一化成 [`TriggerEvent`] 后交给
//! [`dispatcher::dispatch`] 处理；dispatcher 在 SkillRegistry / RunRecorder
//! 之上提供"命令路由 + 白名单校验"，并产出 [`TriggerReply`] 让 trigger 自己
//! 决定如何回送（Telegram 用 sendMessage，Webhook 用 200 + body）。
//!
//! 安全边界：
//!   * 远程入口只能命中 `Skill::preview / execute`，没有 raw shell 通道。
//!   * `execute` 自身仍走 `PermissionPolicy::request_async`，会在本机 GPUI
//!     弹出 ApprovalRequest——远端发命令是第一次确认，本机用户点 Allow 是
//!     第二次确认，天然形成"双确认"。
//!   * Telegram chat_id 白名单由 `ONE_TELEGRAM_ALLOWED_CHATS` 环境变量
//!     配置，命中外的消息直接拒绝，不入 dispatcher。

pub mod dispatcher;
pub mod telegram;

use async_trait::async_trait;

#[derive(Debug, Clone)]
pub struct TriggerEvent {
    pub source: String,
    pub chat_id: String,
    pub user: Option<String>,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct TriggerReply {
    pub text: String,
}

#[async_trait]
pub trait Trigger: Send + Sync {
    fn name(&self) -> &str;

    async fn run(&self) -> anyhow::Result<()>;
}

pub use dispatcher::{dispatch, parse_command, TriggerCommand};
