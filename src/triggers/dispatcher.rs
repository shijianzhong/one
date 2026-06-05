#![allow(dead_code)]

//! 命令路由：把远端文本消息映射到 Skill / 审计 / 帮助。
//!
//! 当前支持的命令：
//!   * `/help`              帮助
//!   * `/skills`            列出已注册 Skill
//!   * `/preview <id> [json]` 跑 Skill::preview
//!   * `/run <id> [json]`     跑 Skill::execute（execute 自身仍会触发本机授权弹窗）
//!   * `/audit [n]`         拉取最近 N 条 RunEvent
//!
//! Dispatcher 不持有任何 GPUI 句柄；它纯粹在 tokio 上下文里调用
//! `crate::skills::registry()` + `crate::task_db`。这让 trigger 可以独立
//! 启动而不需要 AppState 引用。

use serde_json::Value;

use super::TriggerReply;
use crate::agents::permission::DangerLevel;

#[derive(Debug, Clone)]
pub enum TriggerCommand {
    Help,
    ListSkills,
    PreviewSkill { id: String, args: Value },
    RunSkill { id: String, args: Value },
    Audit { limit: usize },
    Workspace { name: String },
    ListWorkspaces,
    Status,
    ListRemoteTasks,
    ClearTask,
    Unknown(String),
}

pub fn parse_command(text: &str) -> TriggerCommand {
    let text = text.trim();
    if !text.starts_with('/') {
        return TriggerCommand::Unknown(text.to_string());
    }
    let mut parts = text.splitn(2, char::is_whitespace);
    let head = parts.next().unwrap_or("/");
    let rest = parts.next().unwrap_or("").trim();

    match head {
        "/help" | "/start" => TriggerCommand::Help,
        "/skills" => TriggerCommand::ListSkills,
        "/preview" => {
            let (id, args) = split_id_and_args(rest);
            TriggerCommand::PreviewSkill { id, args }
        }
        "/run" => {
            let (id, args) = split_id_and_args(rest);
            TriggerCommand::RunSkill { id, args }
        }
        "/audit" => {
            let limit = rest.parse::<usize>().unwrap_or(10).clamp(1, 50);
            TriggerCommand::Audit { limit }
        }
        "/workspace" => {
            TriggerCommand::Workspace {
                name: rest.to_string(),
            }
        }
        "/workspaces" => TriggerCommand::ListWorkspaces,
        "/status" => TriggerCommand::Status,
        "/tasks" => TriggerCommand::ListRemoteTasks,
        "/clear" => TriggerCommand::ClearTask,
        other => TriggerCommand::Unknown(other.to_string()),
    }
}

fn split_id_and_args(rest: &str) -> (String, Value) {
    let mut parts = rest.splitn(2, char::is_whitespace);
    let id = parts.next().unwrap_or("").trim().to_string();
    let json_part = parts.next().unwrap_or("").trim();
    let args = if json_part.is_empty() {
        Value::Object(Default::default())
    } else {
        serde_json::from_str(json_part).unwrap_or(Value::Object(Default::default()))
    };
    (id, args)
}

pub async fn dispatch(text: &str) -> TriggerReply {
    let command = parse_command(text);
    match command {
        TriggerCommand::Help => TriggerReply::new(help_text()),
        TriggerCommand::ListSkills => TriggerReply::new(list_skills()),
        TriggerCommand::PreviewSkill { id, args } => {
            TriggerReply::new(preview_skill(&id, args).await)
        }
        TriggerCommand::RunSkill { id, args } => run_skill(&id, args).await,
        TriggerCommand::Audit { limit } => TriggerReply::new(audit_text(limit)),
        TriggerCommand::Workspace { name } => {
            // 交给 telegram.rs 处理实际的 workspace 切换
            // 这里只返回格式化后的回复
            if name.is_empty() {
                TriggerReply::new("当前 workspace：请查看 /status".to_string())
            } else {
                TriggerReply::new(format!("/workspace {}", name))
            }
        }
        TriggerCommand::ListWorkspaces => {
            let workspaces = list_workspaces_text();
            TriggerReply::new(workspaces)
        }
        TriggerCommand::Status => {
            TriggerReply::new(status_text())
        }
        TriggerCommand::ListRemoteTasks => {
            TriggerReply::new(list_remote_tasks_text())
        }
        TriggerCommand::ClearTask => {
            TriggerReply::new("远程任务已清除。".to_string())
        }
        TriggerCommand::Unknown(s) => TriggerReply::new(format!(
            "未识别的命令：{}\n输入 /help 查看可用命令。",
            short_label(&s)
        )),
    }
}

fn short_label(s: &str) -> String {
    if s.len() > 80 {
        format!("{}...", &s[..80])
    } else {
        s.to_string()
    }
}

fn help_text() -> String {
    r#"可用命令：
/skills              列出已注册 Skill
/preview <id> [json] 预览 Skill（只读）
/run <id> [json]     执行 Skill（需要本机授权弹窗确认）
/audit [n]           最近 N 条 RunEvent（默认 10）
/workspace [name]    切换到指定 workspace
/workspaces          列出所有 workspace
/status              显示当前远程 Task 状态
/tasks               列出该 workspace 下所有远程 Task
/clear               结束当前远程 Task
/help                显示本帮助"#
        .to_string()
}

fn list_skills() -> String {
    let manifests = crate::skills::registry().manifests();
    if manifests.is_empty() {
        return "（无已注册 Skill）".to_string();
    }
    let mut out = String::from("已注册 Skill：\n");
    for m in manifests {
        out.push_str(&format!(
            "• {}（{}）— {}\n",
            m.id,
            m.category.label(),
            m.description
        ));
    }
    out
}

fn list_workspaces_text() -> String {
    // 通过 SQLite 直接查询 workspace 列表
    let db_path = dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".one")
        .join("one.db");
    if !db_path.exists() {
        return "暂无 workspace".to_string();
    }
    let conn = sqlez::connection::Connection::open_file(
        db_path.to_str().unwrap_or("one.db"),
    );
    match crate::task_db::load_workspaces(&conn) {
        Ok(rows) if rows.is_empty() => "暂无 workspace".to_string(),
        Ok(rows) => {
            let mut out = "Workspace 列表：\n".to_string();
            for w in rows {
                out.push_str(&format!("• {}（ID: {}）— {}\n", w.name, w.id, w.path));
            }
            out
        }
        Err(_) => "查询 workspace 失败".to_string(),
    }
}

/// 构建 /status 回复文本
fn status_text() -> String {
    let db_path = dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".one")
        .join("one.db");
    if !db_path.exists() {
        return "暂无状态信息（数据库不存在）".to_string();
    }
    let conn = sqlez::connection::Connection::open_file(
        db_path.to_str().unwrap_or("one.db"),
    );
    match crate::task_db::load_workspaces(&conn) {
        Ok(rows) if rows.is_empty() => "暂无 workspace".to_string(),
        Ok(rows) => {
            let mut out = "远程运行状态：\n".to_string();
            for w in rows {
                let task_count = crate::task_db::load_remote_tasks(&conn, w.id)
                    .map(|t| t.len())
                    .unwrap_or(0);
                out.push_str(&format!(
                    "• {} — {} 个任务\n",
                    w.name, task_count
                ));
            }
            out
        }
        Err(_) => "查询失败".to_string(),
    }
}

/// 构建 /tasks 回复文本
fn list_remote_tasks_text() -> String {
    // 当前没有 workspace 上下文信息，只能查所有 workspace
    let db_path = dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".one")
        .join("one.db");
    if !db_path.exists() {
        return "暂无任务（数据库不存在）".to_string();
    }
    let conn = sqlez::connection::Connection::open_file(
        db_path.to_str().unwrap_or("one.db"),
    );
    match crate::task_db::load_workspaces(&conn) {
        Ok(rows) if rows.is_empty() => "暂无 workspace".to_string(),
        Ok(rows) => {
            let mut out = "所有远程任务：\n".to_string();
            for w in rows {
                let tasks = crate::task_db::load_remote_tasks(&conn, w.id).unwrap_or_default();
                if tasks.is_empty() {
                    continue;
                }
                out.push_str(&format!("\n【{}】\n", w.name));
                for t in tasks.iter().take(10) {
                    let title = if t.title.is_empty() { "（无标题）" } else { &t.title };
                    out.push_str(&format!("  • #{} — {}\n", t.id, title));
                }
                if tasks.len() > 10 {
                    out.push_str(&format!("  …共 {} 个任务\n", tasks.len()));
                }
            }
            if out == "所有远程任务：\n" {
                out.push_str("（无任务）");
            }
            out
        }
        Err(_) => "查询失败".to_string(),
    }
}

async fn preview_skill(id: &str, args: Value) -> String {
    let Some(skill) = crate::skills::registry().find(id) else {
        return format!(
            "未找到 skill_id：{}\n输入 /skills 查看可用列表。",
            short_label(id)
        );
    };
    match skill.preview(args).await {
        Ok(p) => {
            let mut out = format!("[preview] {}\n{}\n", id, p.summary);
            for it in p.items.iter().take(10) {
                out.push_str(&format!("• {} | {} | {} 字节\n", it.label, it.detail, it.bytes));
            }
            if !p.warnings.is_empty() {
                out.push_str("\n⚠ ");
                out.push_str(&p.warnings.join("\n⚠ "));
            }
            out
        }
        Err(e) => format!("preview 失败：{}", e),
    }
}

async fn run_skill(id: &str, args: Value) -> TriggerReply {
    let Some(skill) = crate::skills::registry().find(id) else {
        return TriggerReply::new(format!(
            "未找到 skill_id：{}\n输入 /skills 查看可用列表。",
            short_label(id)
        ));
    };

    // 检查远程危险等级
    let danger_level = skill.manifest().danger_level;
    if danger_level != DangerLevel::Normal {
        // 暗号未设置时直接拒绝
        if !crate::agents::remote_auth::RemoteAuth::is_cipher_set() {
            return TriggerReply::new(format!(
                "⚠️ 操作「{}」需要远程暗号确认，但暗号尚未设置。\n请先在本机 ONE 设置页配置远程暗号。",
                id
            ));
        }
        // 返回需要暗号确认的标记，由 telegram.rs 状态机处理
        return TriggerReply::new(format!(
            "📐 操作「{}」需要远程暗号确认，请在 2 分钟内回复暗号。",
            id
        ))
        .needs_cipher(danger_level);
    }

    // 远程执行自动进入 Strict 权限模式
    let _guard = crate::agents::permission::RemoteScopeGuard::enter();

    let result = skill.execute(args, None).await;
    TriggerReply::new(match result {
        Ok(exec) if exec.denied => {
            format!("[run] {}：用户在本机拒绝。{}", id, exec.summary)
        }
        Ok(exec) => {
            let mut out = format!("[run] {}\n{}\n", id, exec.summary);
            if exec.freed_bytes > 0 {
                out.push_str(&format!("释放 {} 字节\n", exec.freed_bytes));
            }
            if !exec.success_items.is_empty() {
                out.push_str(&format!("成功 {} 项\n", exec.success_items.len()));
            }
            if !exec.failed_items.is_empty() {
                out.push_str(&format!("失败 {} 项：\n", exec.failed_items.len()));
                for (k, v) in exec.failed_items.iter().take(5) {
                    out.push_str(&format!("  - {}: {}\n", k, v));
                }
            }
            out
        }
        Err(e) => format!("execute 失败：{}", e),
    })
}

fn audit_text(limit: usize) -> String {
    match crate::task_db::load_recent_run_events(limit) {
        Ok(rows) if rows.is_empty() => "暂无 RunEvent".to_string(),
        Ok(rows) => {
            let mut out = format!("最近 {} 条 RunEvent：\n", rows.len());
            for r in rows {
                out.push_str(&format!(
                    "[{}] run#{} {} {}\n",
                    r.created_at, r.run_id, r.kind, short_label(&r.payload)
                ));
            }
            out
        }
        Err(e) => format!("查询失败：{}", e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_help() {
        match parse_command("/help") {
            TriggerCommand::Help => {}
            other => panic!("expected Help, got {:?}", other),
        }
    }

    #[test]
    fn parses_preview_with_json_args() {
        match parse_command("/preview system.cleaner {\"targets\": [\"废纸篓\"]}") {
            TriggerCommand::PreviewSkill { id, args } => {
                assert_eq!(id, "system.cleaner");
                assert!(args.get("targets").is_some());
            }
            other => panic!("expected PreviewSkill, got {:?}", other),
        }
    }

    #[test]
    fn parses_audit_with_clamp() {
        match parse_command("/audit 9999") {
            TriggerCommand::Audit { limit } => assert_eq!(limit, 50),
            other => panic!("expected Audit, got {:?}", other),
        }
    }

    #[test]
    fn unknown_command_returns_unknown() {
        match parse_command("/unknown foo") {
            TriggerCommand::Unknown(_) => {}
            other => panic!("expected Unknown, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn dispatch_help_returns_text() {
        let reply = dispatch("/help").await;
        assert!(reply.text.contains("/skills"));
    }

    #[tokio::test]
    async fn dispatch_skills_lists_registry() {
        let reply = dispatch("/skills").await;
        assert!(reply.text.contains("system.cleaner"));
    }
}
