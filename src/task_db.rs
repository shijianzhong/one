use anyhow::Result;
use sqlez::{connection::Connection, statement::Statement};
use std::path::PathBuf;

pub struct Database {
    pub conn: Connection,
}

impl Database {
    pub fn new() -> Result<Self> {
        let db_path = get_db_path();
        let conn = Connection::open_file(db_path.to_str().unwrap_or("one.db"));
        let conn_ref = &conn;

        // Create tables
        let _ = (conn_ref
            .exec(
                "CREATE TABLE IF NOT EXISTS workspaces (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            path TEXT NOT NULL,
            expanded INTEGER DEFAULT 0,
            default_task_id INTEGER
        )",
            )
            .unwrap())();

        ensure_workspace_default_task_column(conn_ref)?;

        let _ = (conn_ref
            .exec(
                "CREATE TABLE IF NOT EXISTS tasks (
            id INTEGER PRIMARY KEY,
            workspace_id INTEGER NOT NULL,
            title TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'todo',
            is_draft INTEGER DEFAULT 0,
            FOREIGN KEY (workspace_id) REFERENCES workspaces(id)
        )",
            )
            .unwrap())();

        ensure_task_draft_column(conn_ref)?;

        // 远程 Task 相关的 messages 表扩展
        ensure_messages_step_columns(conn_ref)?;

        let _ = (conn_ref
            .exec(
                "CREATE TABLE IF NOT EXISTS messages (
            id INTEGER PRIMARY KEY,
            task_id INTEGER NOT NULL,
            role TEXT NOT NULL,
            content TEXT NOT NULL,
            created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY (task_id) REFERENCES tasks(id)
        )",
            )
            .unwrap())();

        // Agent tables
        let _ = (conn_ref
            .exec(
                "CREATE TABLE IF NOT EXISTS agents (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            agent_type TEXT NOT NULL,
            description TEXT,
            capabilities_json TEXT,
            config_json TEXT,
            memory_threshold INTEGER DEFAULT 3,
            created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
        )",
            )
            .unwrap())();

        let _ = (conn_ref
            .exec(
                "CREATE TABLE IF NOT EXISTS agent_instances (
            id INTEGER PRIMARY KEY,
            agent_id INTEGER NOT NULL,
            task_id INTEGER,
            status TEXT NOT NULL DEFAULT 'idle',
            session_state_json TEXT,
            last_active_at TIMESTAMP,
            FOREIGN KEY (agent_id) REFERENCES agents(id),
            FOREIGN KEY (task_id) REFERENCES tasks(id)
        )",
            )
            .unwrap())();

        let _ = (conn_ref
            .exec(
                "CREATE TABLE IF NOT EXISTS agent_conversations (
            id INTEGER PRIMARY KEY,
            agent_instance_id INTEGER NOT NULL,
            user_query TEXT NOT NULL,
            agent_response TEXT,
            context_snapshot TEXT,
            created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY (agent_instance_id) REFERENCES agent_instances(id)
        )",
            )
            .unwrap())();

        let _ = (conn_ref
            .exec(
                "CREATE TABLE IF NOT EXISTS agent_capabilities (
            id INTEGER PRIMARY KEY,
            agent_id INTEGER NOT NULL,
            capability_type TEXT NOT NULL,
            prompt_template TEXT,
            tools_json TEXT,
            FOREIGN KEY (agent_id) REFERENCES agents(id)
        )",
            )
            .unwrap())();

        let _ = (conn_ref
            .exec(
                "CREATE TABLE IF NOT EXISTS task_runs (
            id INTEGER PRIMARY KEY,
            task_id INTEGER NOT NULL,
            kind TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'running',
            started_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            finished_at TIMESTAMP,
            FOREIGN KEY (task_id) REFERENCES tasks(id)
        )",
            )
            .unwrap())();

        let _ = (conn_ref
            .exec(
                "CREATE TABLE IF NOT EXISTS run_events (
            id INTEGER PRIMARY KEY,
            run_id INTEGER NOT NULL,
            kind TEXT NOT NULL,
            payload TEXT NOT NULL,
            created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY (run_id) REFERENCES task_runs(id)
        )",
            )
            .unwrap())();

        let _ = (conn_ref
            .exec("CREATE INDEX IF NOT EXISTS idx_run_events_run_id ON run_events(run_id)")
            .unwrap())();

        Ok(Self { conn })
    }
}

fn table_has_column(conn: &Connection, table: &str, column: &str) -> Result<bool> {
    let mut stmt = Statement::prepare(conn, &format!("PRAGMA table_info({})", table))?;
    let cols: Vec<String> = stmt
        .map(|s| s.column_text(1).map(|v| v.to_string()))?
        .into_iter()
        .collect();
    Ok(cols.iter().any(|c| c == column))
}

fn ensure_workspace_default_task_column(conn: &Connection) -> Result<()> {
    if !table_has_column(conn, "workspaces", "default_task_id")? {
        let _ = (conn
            .exec("ALTER TABLE workspaces ADD COLUMN default_task_id INTEGER")
            .unwrap())();
    }

    Ok(())
}

fn ensure_task_draft_column(conn: &Connection) -> Result<()> {
    if !table_has_column(conn, "tasks", "is_draft")? {
        let _ = (conn
            .exec("ALTER TABLE tasks ADD COLUMN is_draft INTEGER DEFAULT 0")
            .unwrap())();
    }

    // At most one draft per workspace; do not auto-create drafts here.
    let mut stmt = Statement::prepare(conn, "SELECT id FROM workspaces")?;
    let workspace_ids: Vec<usize> = stmt
        .map(|s| s.column_int64(0).map(|v| v as usize))?
        .into_iter()
        .collect();

    for workspace_id in workspace_ids {
        let mut stmt = Statement::prepare(
            conn,
            "SELECT id FROM tasks WHERE workspace_id = ? AND is_draft = 1 ORDER BY id",
        )?;
        stmt.with_bindings(&workspace_id)?;
        let draft_ids: Vec<usize> = stmt
            .map(|s| s.column_int64(0).map(|v| v as usize))?
            .into_iter()
            .collect();

        if draft_ids.len() > 1 {
            for id in draft_ids.into_iter().skip(1) {
                let mut stmt =
                    Statement::prepare(conn, "UPDATE tasks SET is_draft = 0 WHERE id = ?")?;
                stmt.with_bindings(&id)?;
                stmt.exec()?;
            }
        }
    }

    Ok(())
}

fn get_db_path() -> PathBuf {
    let config_dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".one");
    std::fs::create_dir_all(&config_dir).ok();
    config_dir.join("one.db")
}

// ====== 远程 Task Step 支持 ======

/// 确保 messages 表有远程 Task 所需的额外列
fn ensure_messages_step_columns(conn: &Connection) -> Result<()> {
    if !table_has_column(conn, "messages", "step_index")? {
        let _ = (conn
            .exec("ALTER TABLE messages ADD COLUMN step_index INTEGER DEFAULT 0")
            .unwrap())();
    }
    if !table_has_column(conn, "messages", "step_type")? {
        let _ = (conn
            .exec("ALTER TABLE messages ADD COLUMN step_type TEXT DEFAULT 'user_message'")
            .unwrap())();
    }
    if !table_has_column(conn, "messages", "skill_id")? {
        let _ = (conn
            .exec("ALTER TABLE messages ADD COLUMN skill_id TEXT")
            .unwrap())();
    }
    Ok(())
}

/// 获取指定 workspace 下所有远程 Task（非 draft）
pub fn load_remote_tasks(conn: &Connection, workspace_id: usize) -> Result<Vec<TaskRow>> {
    let mut stmt = Statement::prepare(
        conn,
        "SELECT id, title, is_draft FROM tasks WHERE workspace_id = ? ORDER BY id DESC LIMIT 20",
    )?;
    stmt.with_bindings(&workspace_id)?;
    stmt.map(|s| {
        let id = s.column_int64(0)? as usize;
        let title = s.column_text(1)?.to_string();
        let is_draft = s.column_int64(2).unwrap_or(0) != 0;
        Ok(TaskRow {
            id,
            title,
            is_draft,
        })
    })
}

/// 统计某个 task 的 messages 数量（用于 step_index）
pub fn count_messages(conn: &Connection, task_id: usize) -> Result<usize> {
    let mut stmt =
        Statement::prepare(conn, "SELECT COUNT(*) FROM messages WHERE task_id = ?")?;
    stmt.with_bindings(&task_id)?;
    let count: Vec<usize> = stmt
        .map(|s| s.column_int64(0).map(|v| v as usize))?
        .into_iter()
        .collect();
    Ok(count.into_iter().next().unwrap_or(0))
}

/// 创建远程 Task，返回 task_id
pub fn insert_remote_task(conn: &Connection, workspace_id: usize) -> Result<usize> {
    insert_task(conn, workspace_id, "远程任务")
}

/// 插入带 step 信息的消息（远程 Task 使用）
pub fn insert_message_step(
    conn: &Connection,
    task_id: usize,
    role: &str,
    content: &str,
    step_index: i64,
    step_type: &str,
    skill_id: Option<&str>,
) -> Result<usize> {
    let mut stmt = Statement::prepare(
        conn,
        "INSERT INTO messages (task_id, role, content, step_index, step_type, skill_id) VALUES (?, ?, ?, ?, ?, ?)",
    )?;
    stmt.with_bindings(&(task_id, role, content, step_index, step_type, skill_id))?;
    stmt.exec()?;
    let mut stmt = Statement::prepare(conn, "SELECT last_insert_rowid()")?;
    let id = stmt.map(|s| s.column_int64(0))?.into_iter().next().unwrap();
    Ok(id as usize)
}

/// 查询某个 task 最近 N 条消息
pub fn load_recent_messages(conn: &Connection, task_id: usize, limit: usize) -> Result<Vec<MessageRow>> {
    let mut stmt = Statement::prepare(
        conn,
        "SELECT role, content FROM messages WHERE task_id = ? ORDER BY created_at DESC LIMIT ?",
    )?;
    let limit_i64 = limit as i64;
    stmt.with_bindings(&(task_id, limit_i64))?;
    let mut rows: Vec<MessageRow> = stmt
        .map(|s| {
            let role = s.column_text(0)?.to_string();
            let content = s.column_text(1)?.to_string();
            Ok(MessageRow { role, content })
        })?;
    rows.reverse();
    Ok(rows)
}

#[derive(Debug, Clone)]
pub struct WorkspaceRow {
    pub id: usize,
    pub name: String,
    pub path: String,
    pub expanded: bool,
}

#[derive(Debug, Clone)]
pub struct TaskRow {
    pub id: usize,
    pub title: String,
    pub is_draft: bool,
}

#[derive(Debug, Clone)]
pub struct MessageRow {
    pub role: String,
    pub content: String,
}

pub fn load_workspaces(conn: &Connection) -> Result<Vec<WorkspaceRow>> {
    let mut stmt = Statement::prepare(conn, "SELECT id, name, path, expanded FROM workspaces")?;
    stmt.map(|s| {
        let id = s.column_int64(0)? as usize;
        let name = s.column_text(1)?.to_string();
        let path = s.column_text(2)?.to_string();
        let expanded = s.column_int64(3)? != 0;
        Ok(WorkspaceRow {
            id,
            name,
            path,
            expanded,
        })
    })
}

pub fn load_tasks(conn: &Connection, workspace_id: usize) -> Result<Vec<TaskRow>> {
    let mut stmt = Statement::prepare(
        conn,
        "SELECT id, title, is_draft FROM tasks WHERE workspace_id = ? ORDER BY id",
    )?;
    stmt.with_bindings(&workspace_id)?;
    stmt.map(|s| {
        let id = s.column_int64(0)? as usize;
        let title = s.column_text(1)?.to_string();
        let is_draft = s.column_int64(2).unwrap_or(0) != 0;
        Ok(TaskRow {
            id,
            title,
            is_draft,
        })
    })
}

pub fn ensure_draft_task(conn: &Connection, workspace_id: usize) -> Result<usize> {
    let mut stmt = Statement::prepare(
        conn,
        "SELECT id FROM tasks WHERE workspace_id = ? AND is_draft = 1 ORDER BY id LIMIT 1",
    )?;
    stmt.with_bindings(&workspace_id)?;
    let mut existing: Vec<usize> = stmt
        .map(|s| s.column_int64(0).map(|v| v as usize))?
        .into_iter()
        .collect();
    if let Some(id) = existing.pop() {
        return Ok(id);
    }

    let mut stmt =
        Statement::prepare(conn, "UPDATE tasks SET is_draft = 0 WHERE workspace_id = ?")?;
    stmt.with_bindings(&workspace_id)?;
    stmt.exec()?;

    let id = insert_task(conn, workspace_id, "")?;
    let mut stmt = Statement::prepare(conn, "UPDATE tasks SET is_draft = 1 WHERE id = ?")?;
    stmt.with_bindings(&id)?;
    stmt.exec()?;
    Ok(id)
}

pub fn insert_workspace(conn: &Connection, name: &str, path: &str) -> Result<usize> {
    let mut stmt = Statement::prepare(
        conn,
        "INSERT INTO workspaces (name, path, expanded) VALUES (?, ?, 0)",
    )?;
    stmt.with_bindings(&(name, path))?;
    stmt.exec()?;
    let mut stmt = Statement::prepare(conn, "SELECT last_insert_rowid()")?;
    let id = stmt.map(|s| s.column_int64(0))?.into_iter().next().unwrap();
    Ok(id as usize)
}

pub fn insert_task(conn: &Connection, workspace_id: usize, title: &str) -> Result<usize> {
    let mut stmt = Statement::prepare(
        conn,
        "INSERT INTO tasks (workspace_id, title, status) VALUES (?, ?, 'todo')",
    )?;
    stmt.with_bindings(&(workspace_id, title))?;
    stmt.exec()?;
    let mut stmt = Statement::prepare(conn, "SELECT last_insert_rowid()")?;
    let id = stmt.map(|s| s.column_int64(0))?.into_iter().next().unwrap();
    Ok(id as usize)
}

pub fn update_task_title(conn: &Connection, task_id: usize, title: &str) -> Result<()> {
    let mut stmt = Statement::prepare(conn, "UPDATE tasks SET title = ? WHERE id = ?")?;
    stmt.with_bindings(&(title, task_id))?;
    stmt.exec()?;
    Ok(())
}

pub fn delete_task(conn: &Connection, task_id: usize) -> Result<()> {
    // ── 级联清理关联表 ────────────────────────────────────────────
    // 1. 通过 task_runs 找到所有 run_id，删除 run_events
    let run_ids: Vec<i64> = {
        let mut stmt = Statement::prepare(conn, "SELECT id FROM task_runs WHERE task_id = ?")?;
        stmt.with_bindings(&task_id)?;
        stmt.map(|s| s.column_int64(0))?
            .into_iter()
            .collect()
    };
    for run_id in &run_ids {
        let mut stmt = Statement::prepare(conn, "DELETE FROM run_events WHERE run_id = ?")?;
        stmt.with_bindings(run_id)?;
        stmt.exec()?;
    }

    // 2. 删除 task_runs
    {
        let mut stmt = Statement::prepare(conn, "DELETE FROM task_runs WHERE task_id = ?")?;
        stmt.with_bindings(&task_id)?;
        stmt.exec()?;
    }

    // 3. 删除 agent_instances 及关联的 agent_conversations
    let instance_ids: Vec<i64> = {
        let mut stmt = Statement::prepare(conn, "SELECT id FROM agent_instances WHERE task_id = ?")?;
        stmt.with_bindings(&task_id)?;
        stmt.map(|s| s.column_int64(0))?
            .into_iter()
            .collect()
    };
    for inst_id in &instance_ids {
        let mut stmt = Statement::prepare(conn, "DELETE FROM agent_conversations WHERE agent_instance_id = ?")?;
        stmt.with_bindings(inst_id)?;
        stmt.exec()?;
    }
    {
        let mut stmt = Statement::prepare(conn, "DELETE FROM agent_instances WHERE task_id = ?")?;
        stmt.with_bindings(&task_id)?;
        stmt.exec()?;
    }

    // 4. 删除 messages
    {
        let mut stmt = Statement::prepare(conn, "DELETE FROM messages WHERE task_id = ?")?;
        stmt.with_bindings(&task_id)?;
        stmt.exec()?;
    }

    // 5. 删除 task 本身
    {
        let mut stmt = Statement::prepare(conn, "DELETE FROM tasks WHERE id = ?")?;
        stmt.with_bindings(&task_id)?;
        stmt.exec()?;
    }
    Ok(())
}

pub fn delete_workspace(conn: &Connection, workspace_id: usize) -> Result<()> {
    // Get all task IDs for this workspace
    let mut stmt = Statement::prepare(conn, "SELECT id FROM tasks WHERE workspace_id = ?")?;
    stmt.with_bindings(&workspace_id)?;
    let task_ids: Vec<usize> = stmt
        .map(|s| s.column_int64(0).map(|v| v as usize))?
        .into_iter()
        .collect();

    // Delete messages for all tasks in the workspace
    for task_id in task_ids {
        let mut stmt = Statement::prepare(conn, "DELETE FROM messages WHERE task_id = ?")?;
        stmt.with_bindings(&task_id)?;
        stmt.exec()?;
    }

    // Delete all tasks in the workspace
    let mut stmt = Statement::prepare(conn, "DELETE FROM tasks WHERE workspace_id = ?")?;
    stmt.with_bindings(&workspace_id)?;
    stmt.exec()?;
    // Delete the workspace
    let mut stmt = Statement::prepare(conn, "DELETE FROM workspaces WHERE id = ?")?;
    stmt.with_bindings(&workspace_id)?;
    stmt.exec()?;
    Ok(())
}

pub fn update_workspace_expanded(
    conn: &Connection,
    workspace_id: usize,
    expanded: bool,
) -> Result<()> {
    let mut stmt = Statement::prepare(conn, "UPDATE workspaces SET expanded = ? WHERE id = ?")?;
    let expanded_i64 = expanded as i64;
    stmt.with_bindings(&(expanded_i64, workspace_id))?;
    stmt.exec()?;
    Ok(())
}

// Message functions
pub fn load_messages(conn: &Connection, task_id: usize) -> Result<Vec<MessageRow>> {
    let mut stmt = Statement::prepare(
        conn,
        "SELECT role, content FROM messages WHERE task_id = ? ORDER BY created_at ASC",
    )?;
    stmt.with_bindings(&task_id)?;
    stmt.map(|s| {
        let role = s.column_text(0)?.to_string();
        let content = s.column_text(1)?.to_string();
        Ok(MessageRow { role, content })
    })
}

pub fn insert_message(
    conn: &Connection,
    task_id: usize,
    role: &str,
    content: &str,
) -> Result<usize> {
    let mut stmt = Statement::prepare(
        conn,
        "INSERT INTO messages (task_id, role, content) VALUES (?, ?, ?)",
    )?;
    stmt.with_bindings(&(task_id, role, content))?;
    stmt.exec()?;
    let mut stmt = Statement::prepare(conn, "SELECT last_insert_rowid()")?;
    let id = stmt.map(|s| s.column_int64(0))?.into_iter().next().unwrap();

    let mut stmt = Statement::prepare(conn, "SELECT is_draft FROM tasks WHERE id = ?")?;
    stmt.with_bindings(&task_id)?;
    let rows: Vec<bool> = stmt
        .map(|s| {
            let is_draft = s.column_int64(0).unwrap_or(0) != 0;
            Ok(is_draft)
        })?
        .into_iter()
        .collect();
    if let Some(true) = rows.into_iter().next() {
        let mut stmt = Statement::prepare(conn, "UPDATE tasks SET is_draft = 0 WHERE id = ?")?;
        stmt.with_bindings(&task_id)?;
        stmt.exec()?;
    }

    Ok(id as usize)
}

// Export messages to JSON format
pub fn export_messages_json(conn: &Connection, task_id: usize) -> Result<String> {
    let messages = load_messages(conn, task_id)?;
    let mut json_str = String::from("[\n");
    for (i, msg) in messages.iter().enumerate() {
        json_str.push_str(&format!(
            "  {{\"role\": \"{}\", \"content\": \"{}\"}}",
            msg.role.replace("\"", "\\\""),
            msg.content.replace("\"", "\\\"").replace("\n", "\\n")
        ));
        if i < messages.len() - 1 {
            json_str.push(',');
        }
        json_str.push('\n');
    }
    json_str.push_str("]");
    Ok(json_str)
}

// Export messages to Markdown format
pub fn export_messages_markdown(
    conn: &Connection,
    task_id: usize,
    task_title: &str,
) -> Result<String> {
    let messages = load_messages(conn, task_id)?;
    let mut md = String::new();
    md.push_str(&format!("# {}\n\n", task_title));
    md.push_str("*Exported from ONE*\n\n");
    for msg in messages.iter() {
        let role_label = if msg.role == "user" {
            "**You**"
        } else {
            "**Assistant**"
        };
        md.push_str(&format!("## {}\n\n{}\n\n---\n\n", role_label, msg.content));
    }
    Ok(md)
}

// ====================== Agent CRUD ======================

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct AgentRow {
    pub id: usize,
    pub name: String,
    pub agent_type: String,
    pub description: Option<String>,
    pub capabilities_json: Option<String>,
    pub config_json: Option<String>,
    pub memory_threshold: i64,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct AgentInstanceRow {
    pub id: usize,
    pub agent_id: usize,
    pub task_id: Option<usize>,
    pub status: String,
    pub session_state_json: Option<String>,
    pub last_active_at: Option<String>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct AgentCapabilityRow {
    pub id: usize,
    pub agent_id: usize,
    pub capability_type: String,
    pub prompt_template: Option<String>,
    pub tools_json: Option<String>,
}

#[allow(dead_code)]
pub fn load_agents(conn: &Connection) -> Result<Vec<AgentRow>> {
    let mut stmt = Statement::prepare(conn, "SELECT id, name, agent_type, description, capabilities_json, config_json, memory_threshold FROM agents ORDER BY created_at DESC")?;
    stmt.map(|s| {
        let id = s.column_int64(0)? as usize;
        let name = s.column_text(1)?.to_string();
        let agent_type = s.column_text(2)?.to_string();
        let description = s.column_text(3)?.to_string().into();
        let capabilities_json = s.column_text(4)?.to_string().into();
        let config_json = s.column_text(5)?.to_string().into();
        let memory_threshold = s.column_int64(6)?;
        Ok(AgentRow {
            id,
            name,
            agent_type,
            description,
            capabilities_json,
            config_json,
            memory_threshold,
        })
    })
}

#[allow(dead_code)]
pub fn load_agent_by_id(conn: &Connection, agent_id: usize) -> Result<Option<AgentRow>> {
    let mut stmt = Statement::prepare(conn, "SELECT id, name, agent_type, description, capabilities_json, config_json, memory_threshold FROM agents WHERE id = ?")?;
    stmt.with_bindings(&agent_id)?;
    let result = stmt.map(|s| {
        let id = s.column_int64(0)? as usize;
        let name = s.column_text(1)?.to_string();
        let agent_type = s.column_text(2)?.to_string();
        let description = s.column_text(3)?.to_string().into();
        let capabilities_json = s.column_text(4)?.to_string().into();
        let config_json = s.column_text(5)?.to_string().into();
        let memory_threshold = s.column_int64(6)?;
        Ok(AgentRow {
            id,
            name,
            agent_type,
            description,
            capabilities_json,
            config_json,
            memory_threshold,
        })
    })?;
    Ok(result.into_iter().next())
}

#[allow(dead_code)]
pub fn insert_agent(
    conn: &Connection,
    name: &str,
    agent_type: &str,
    description: Option<&str>,
    capabilities_json: Option<&str>,
    config_json: Option<&str>,
) -> Result<usize> {
    let mut stmt = Statement::prepare(conn, "INSERT INTO agents (name, agent_type, description, capabilities_json, config_json) VALUES (?, ?, ?, ?, ?)")?;
    stmt.with_bindings(&(
        name,
        agent_type,
        description,
        capabilities_json,
        config_json,
    ))?;
    stmt.exec()?;
    let mut stmt = Statement::prepare(conn, "SELECT last_insert_rowid()")?;
    let id = stmt.map(|s| s.column_int64(0))?.into_iter().next().unwrap();
    Ok(id as usize)
}

#[allow(dead_code)]
pub fn update_agent(
    conn: &Connection,
    agent_id: usize,
    name: &str,
    description: Option<&str>,
    capabilities_json: Option<&str>,
    config_json: Option<&str>,
) -> Result<()> {
    let mut stmt = Statement::prepare(conn, "UPDATE agents SET name = ?, description = ?, capabilities_json = ?, config_json = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?")?;
    stmt.with_bindings(&(name, description, capabilities_json, config_json, agent_id))?;
    stmt.exec()?;
    Ok(())
}

#[allow(dead_code)]
pub fn delete_agent(conn: &Connection, agent_id: usize) -> Result<()> {
    // Delete capabilities first
    let mut stmt = Statement::prepare(conn, "DELETE FROM agent_capabilities WHERE agent_id = ?")?;
    stmt.with_bindings(&agent_id)?;
    stmt.exec()?;
    // Delete instances first
    let mut stmt = Statement::prepare(conn, "DELETE FROM agent_instances WHERE agent_id = ?")?;
    stmt.with_bindings(&agent_id)?;
    stmt.exec()?;
    // Delete agent
    let mut stmt = Statement::prepare(conn, "DELETE FROM agents WHERE id = ?")?;
    stmt.with_bindings(&agent_id)?;
    stmt.exec()?;
    Ok(())
}

// Agent instances
#[allow(dead_code)]
pub fn load_agent_instances(conn: &Connection) -> Result<Vec<AgentInstanceRow>> {
    let mut stmt = Statement::prepare(conn, "SELECT id, agent_id, task_id, status, session_state_json, last_active_at FROM agent_instances")?;
    stmt.map(|s| {
        let id = s.column_int64(0)? as usize;
        let agent_id = s.column_int64(1)? as usize;
        let task_id = s.column_int64(2)? as usize;
        let status = s.column_text(3)?.to_string();
        let session_state_json = s.column_text(4)?.to_string().into();
        let last_active_at = s.column_text(5)?.to_string().into();
        Ok(AgentInstanceRow {
            id,
            agent_id,
            task_id: if task_id == 0 { None } else { Some(task_id) },
            status,
            session_state_json,
            last_active_at,
        })
    })
}

#[allow(dead_code)]
pub fn load_agent_instance_by_task(
    conn: &Connection,
    task_id: usize,
) -> Result<Option<AgentInstanceRow>> {
    let mut stmt = Statement::prepare(conn, "SELECT id, agent_id, task_id, status, session_state_json, last_active_at FROM agent_instances WHERE task_id = ?")?;
    stmt.with_bindings(&task_id)?;
    let result = stmt.map(|s| {
        let id = s.column_int64(0)? as usize;
        let agent_id = s.column_int64(1)? as usize;
        let task_id = s.column_int64(2)? as usize;
        let status = s.column_text(3)?.to_string();
        let session_state_json = s.column_text(4)?.to_string().into();
        let last_active_at = s.column_text(5)?.to_string().into();
        Ok(AgentInstanceRow {
            id,
            agent_id,
            task_id: if task_id == 0 { None } else { Some(task_id) },
            status,
            session_state_json,
            last_active_at,
        })
    })?;
    Ok(result.into_iter().next())
}

#[allow(dead_code)]
pub fn insert_agent_instance(
    conn: &Connection,
    agent_id: usize,
    task_id: Option<usize>,
    status: &str,
) -> Result<usize> {
    let mut stmt = Statement::prepare(
        conn,
        "INSERT INTO agent_instances (agent_id, task_id, status) VALUES (?, ?, ?)",
    )?;
    let task_id_val = task_id.unwrap_or(0);
    stmt.with_bindings(&(agent_id, task_id_val, status))?;
    stmt.exec()?;
    let mut stmt = Statement::prepare(conn, "SELECT last_insert_rowid()")?;
    let id = stmt.map(|s| s.column_int64(0))?.into_iter().next().unwrap();
    Ok(id as usize)
}

#[allow(dead_code)]
pub fn update_agent_instance_status(
    conn: &Connection,
    instance_id: usize,
    status: &str,
) -> Result<()> {
    let mut stmt = Statement::prepare(
        conn,
        "UPDATE agent_instances SET status = ?, last_active_at = CURRENT_TIMESTAMP WHERE id = ?",
    )?;
    stmt.with_bindings(&(status, instance_id))?;
    stmt.exec()?;
    Ok(())
}

#[allow(dead_code)]
pub fn update_agent_instance_session(
    conn: &Connection,
    instance_id: usize,
    session_state_json: &str,
) -> Result<()> {
    let mut stmt = Statement::prepare(conn, "UPDATE agent_instances SET session_state_json = ?, last_active_at = CURRENT_TIMESTAMP WHERE id = ?")?;
    stmt.with_bindings(&(session_state_json, instance_id))?;
    stmt.exec()?;
    Ok(())
}

#[allow(dead_code)]
pub fn delete_agent_instance(conn: &Connection, instance_id: usize) -> Result<()> {
    let mut stmt = Statement::prepare(
        conn,
        "DELETE FROM agent_conversations WHERE agent_instance_id = ?",
    )?;
    stmt.with_bindings(&instance_id)?;
    stmt.exec()?;
    let mut stmt = Statement::prepare(conn, "DELETE FROM agent_instances WHERE id = ?")?;
    stmt.with_bindings(&instance_id)?;
    stmt.exec()?;
    Ok(())
}

// Agent capabilities
#[allow(dead_code)]
pub fn load_agent_capabilities(
    conn: &Connection,
    agent_id: usize,
) -> Result<Vec<AgentCapabilityRow>> {
    let mut stmt = Statement::prepare(conn, "SELECT id, agent_id, capability_type, prompt_template, tools_json FROM agent_capabilities WHERE agent_id = ?")?;
    stmt.with_bindings(&agent_id)?;
    stmt.map(|s| {
        let id = s.column_int64(0)? as usize;
        let agent_id = s.column_int64(1)? as usize;
        let capability_type = s.column_text(2)?.to_string();
        let prompt_template = s.column_text(3)?.to_string().into();
        let tools_json = s.column_text(4)?.to_string().into();
        Ok(AgentCapabilityRow {
            id,
            agent_id,
            capability_type,
            prompt_template,
            tools_json,
        })
    })
}

#[allow(dead_code)]
pub fn insert_agent_capability(
    conn: &Connection,
    agent_id: usize,
    capability_type: &str,
    prompt_template: Option<&str>,
    tools_json: Option<&str>,
) -> Result<usize> {
    let mut stmt = Statement::prepare(conn, "INSERT INTO agent_capabilities (agent_id, capability_type, prompt_template, tools_json) VALUES (?, ?, ?, ?)")?;
    stmt.with_bindings(&(agent_id, capability_type, prompt_template, tools_json))?;
    stmt.exec()?;
    let mut stmt = Statement::prepare(conn, "SELECT last_insert_rowid()")?;
    let id = stmt.map(|s| s.column_int64(0))?.into_iter().next().unwrap();
    Ok(id as usize)
}

// Agent conversations (for cross-session memory)
#[allow(dead_code)]
pub fn insert_agent_conversation(
    conn: &Connection,
    agent_instance_id: usize,
    user_query: &str,
    agent_response: Option<&str>,
    context_snapshot: Option<&str>,
) -> Result<usize> {
    let mut stmt = Statement::prepare(conn, "INSERT INTO agent_conversations (agent_instance_id, user_query, agent_response, context_snapshot) VALUES (?, ?, ?, ?)")?;
    stmt.with_bindings(&(
        agent_instance_id,
        user_query,
        agent_response,
        context_snapshot,
    ))?;
    stmt.exec()?;
    let mut stmt = Statement::prepare(conn, "SELECT last_insert_rowid()")?;
    let id = stmt.map(|s| s.column_int64(0))?.into_iter().next().unwrap();
    Ok(id as usize)
}

#[allow(dead_code)]
pub fn load_agent_conversations(
    conn: &Connection,
    agent_instance_id: usize,
) -> Result<Vec<(usize, String, String, Option<String>)>> {
    let mut stmt = Statement::prepare(conn, "SELECT id, user_query, agent_response, context_snapshot FROM agent_conversations WHERE agent_instance_id = ? ORDER BY created_at ASC")?;
    stmt.with_bindings(&agent_instance_id)?;
    stmt.map(|s| {
        let id = s.column_int64(0)? as usize;
        let user_query = s.column_text(1)?.to_string();
        let agent_response = s.column_text(2)?.to_string();
        let context_snapshot = s.column_text(3)?.to_string().into();
        Ok((id, user_query, agent_response, context_snapshot))
    })
}

// ---------- task_runs / run_events ----------
//
// All call sites must go through `crate::run_log::RunRecorder`, which is the
// only consumer of these helpers. Keeping them at `pub(crate)` prevents a new
// caller from re-introducing the duplicated audit-write pattern this crate
// just consolidated.

pub(crate) fn insert_task_run(
    conn: &Connection,
    task_id: usize,
    kind: &str,
) -> Result<usize> {
    let mut stmt = Statement::prepare(
        conn,
        "INSERT INTO task_runs (task_id, kind, status) VALUES (?, ?, 'running')",
    )?;
    stmt.with_bindings(&(task_id, kind))?;
    stmt.exec()?;
    let mut stmt = Statement::prepare(conn, "SELECT last_insert_rowid()")?;
    let id = stmt.map(|s| s.column_int64(0))?.into_iter().next().unwrap();
    Ok(id as usize)
}

pub(crate) fn finish_task_run(conn: &Connection, run_id: usize, status: &str) -> Result<()> {
    let mut stmt = Statement::prepare(
        conn,
        "UPDATE task_runs SET status = ?, finished_at = CURRENT_TIMESTAMP WHERE id = ?",
    )?;
    stmt.with_bindings(&(status, run_id))?;
    stmt.exec()?;
    Ok(())
}

pub(crate) fn append_run_event(
    conn: &Connection,
    run_id: usize,
    kind: &str,
    payload: &str,
) -> Result<usize> {
    let mut stmt = Statement::prepare(
        conn,
        "INSERT INTO run_events (run_id, kind, payload) VALUES (?, ?, ?)",
    )?;
    stmt.with_bindings(&(run_id, kind, payload))?;
    stmt.exec()?;
    let mut stmt = Statement::prepare(conn, "SELECT last_insert_rowid()")?;
    let id = stmt.map(|s| s.column_int64(0))?.into_iter().next().unwrap();
    Ok(id as usize)
}

#[derive(Debug, Clone)]
pub struct RunEventRow {
    pub id: usize,
    pub run_id: usize,
    pub kind: String,
    pub payload: String,
    pub created_at: String,
}

/// 拉取最近 `limit` 条 RunEvent（按 id 倒序）。供远程触达 `/audit` 命令使用。
pub fn load_recent_run_events(limit: usize) -> Result<Vec<RunEventRow>> {
    let limit = limit.clamp(1, 200) as i64;
    let db_path = get_db_path();
    let conn = Connection::open_file(db_path.to_str().unwrap_or("one.db"));
    let mut stmt = Statement::prepare(
        &conn,
        "SELECT id, run_id, kind, payload, created_at FROM run_events ORDER BY id DESC LIMIT ?",
    )?;
    stmt.with_bindings(&limit)?;
    let rows = stmt.map(|s| {
        let id = s.column_int64(0)? as usize;
        let run_id = s.column_int64(1)? as usize;
        let kind = s.column_text(2)?.to_string();
        let payload = s.column_text(3)?.to_string();
        let created_at = s.column_text(4)?.to_string();
        Ok(RunEventRow {
            id,
            run_id,
            kind,
            payload,
            created_at,
        })
    })?;
    Ok(rows)
}
