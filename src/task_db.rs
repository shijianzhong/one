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

        // 远程 Task 相关的 messages 表扩展
        ensure_messages_step_columns(conn_ref)?;

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

        let _ = (conn_ref
            .exec(
                "CREATE TABLE IF NOT EXISTS coding_workflows (
            id INTEGER PRIMARY KEY,
            task_id INTEGER NOT NULL,
            stage TEXT NOT NULL,
            user_request TEXT NOT NULL,
            main_agent_summary TEXT,
            known_constraints_json TEXT,
            suggested_direction TEXT,
            clarification_focus_json TEXT,
            plan_path TEXT,
            log_path TEXT,
            approval_notes_json TEXT,
            last_error TEXT,
            created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY (task_id) REFERENCES tasks(id)
        )",
            )
            .unwrap())();

        let _ = (conn_ref
            .exec(
                "CREATE TABLE IF NOT EXISTS task_artifacts (
            id INTEGER PRIMARY KEY,
            task_id INTEGER NOT NULL,
            workflow_id INTEGER,
            kind TEXT NOT NULL,
            path TEXT NOT NULL,
            title TEXT,
            status TEXT NOT NULL DEFAULT 'ready',
            metadata_json TEXT,
            created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY (task_id) REFERENCES tasks(id),
            FOREIGN KEY (workflow_id) REFERENCES coding_workflows(id)
        )",
            )
            .unwrap())();

        let _ = (conn_ref
            .exec(
                "CREATE INDEX IF NOT EXISTS idx_task_artifacts_task_id ON task_artifacts(task_id)",
            )
            .unwrap())();

        ensure_coding_workflow_columns(conn_ref)?;
        ensure_task_artifact_columns(conn_ref)?;
        ensure_workflow_tables(conn_ref)?;

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

fn ensure_column(conn: &Connection, table: &str, column: &str, definition: &str) -> Result<()> {
    if !table_has_column(conn, table, column)? {
        let sql = format!("ALTER TABLE {} ADD COLUMN {} {}", table, column, definition);
        let _ = (conn.exec(&sql).unwrap())();
    }
    Ok(())
}

fn ensure_coding_workflow_columns(conn: &Connection) -> Result<()> {
    for (column, definition) in [
        ("main_agent_summary", "TEXT"),
        ("known_constraints_json", "TEXT"),
        ("suggested_direction", "TEXT"),
        ("clarification_focus_json", "TEXT"),
        ("approval_notes_json", "TEXT"),
        ("last_error", "TEXT"),
    ] {
        ensure_column(conn, "coding_workflows", column, definition)?;
    }
    Ok(())
}

fn ensure_task_artifact_columns(conn: &Connection) -> Result<()> {
    ensure_column(
        conn,
        "task_artifacts",
        "status",
        "TEXT NOT NULL DEFAULT 'ready'",
    )?;
    ensure_column(conn, "task_artifacts", "metadata_json", "TEXT")?;
    Ok(())
}

pub fn ensure_workflow_tables(conn: &Connection) -> Result<()> {
    (conn.exec(
        "CREATE TABLE IF NOT EXISTS workflows (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            description TEXT,
            status TEXT NOT NULL DEFAULT 'draft',
            version INTEGER NOT NULL DEFAULT 1,
            definition_json TEXT NOT NULL,
            created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
        )",
    )?)()?;

    (conn.exec(
        "CREATE TABLE IF NOT EXISTS workflow_versions (
            id INTEGER PRIMARY KEY,
            workflow_id TEXT NOT NULL,
            version INTEGER NOT NULL,
            definition_json TEXT NOT NULL,
            created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY (workflow_id) REFERENCES workflows(id),
            UNIQUE(workflow_id, version)
        )",
    )?)()?;

    (conn.exec("CREATE INDEX IF NOT EXISTS idx_workflows_status ON workflows(status)")?)()?;
    (conn.exec(
        "CREATE INDEX IF NOT EXISTS idx_workflow_versions_workflow_id ON workflow_versions(workflow_id)",
    )?)()?;

    (conn.exec(
        "CREATE TABLE IF NOT EXISTS workflow_runs (
            id INTEGER PRIMARY KEY,
            workflow_id TEXT NOT NULL,
            workflow_version INTEGER NOT NULL,
            status TEXT NOT NULL DEFAULT 'running',
            error TEXT,
            started_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            finished_at TIMESTAMP,
            FOREIGN KEY (workflow_id) REFERENCES workflows(id)
        )",
    )?)()?;

    (conn.exec(
        "CREATE TABLE IF NOT EXISTS workflow_run_events (
            id INTEGER PRIMARY KEY,
            run_id INTEGER NOT NULL,
            kind TEXT NOT NULL,
            payload TEXT NOT NULL,
            created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY (run_id) REFERENCES workflow_runs(id)
        )",
    )?)()?;

    (conn.exec(
        "CREATE INDEX IF NOT EXISTS idx_workflow_runs_workflow_id ON workflow_runs(workflow_id)",
    )?)()?;
    (conn.exec(
        "CREATE INDEX IF NOT EXISTS idx_workflow_run_events_run_id ON workflow_run_events(run_id)",
    )?)()?;

    (conn.exec(
        "CREATE TABLE IF NOT EXISTS capabilities (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            description TEXT,
            workflow_id TEXT NOT NULL,
            workflow_version INTEGER NOT NULL,
            input_schema_json TEXT,
            output_schema_json TEXT,
            enabled INTEGER NOT NULL DEFAULT 1,
            created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY (workflow_id) REFERENCES workflows(id)
        )",
    )?)()?;

    (conn.exec("CREATE INDEX IF NOT EXISTS idx_capabilities_enabled ON capabilities(enabled)")?)()?;

    Ok(())
}

#[derive(Debug, Clone)]
pub struct WorkflowRunRow {
    pub id: usize,
    pub workflow_id: String,
    pub workflow_version: i64,
    pub status: String,
    pub error: String,
}

#[derive(Debug, Clone)]
pub struct WorkflowRunEventRow {
    pub id: usize,
    pub run_id: usize,
    pub kind: String,
    pub payload: String,
}

#[derive(Debug, Clone)]
pub struct CapabilityRow {
    pub id: String,
    pub name: String,
    pub description: String,
    pub workflow_id: String,
    pub workflow_version: i64,
    pub input_schema_json: String,
    pub output_schema_json: String,
    pub enabled: bool,
}

pub fn upsert_capability(conn: &Connection, capability: &CapabilityRow) -> Result<()> {
    ensure_workflow_tables(conn)?;
    let enabled = if capability.enabled { 1_i64 } else { 0_i64 };
    let existing = {
        let mut stmt =
            Statement::prepare(conn, "SELECT id FROM capabilities WHERE id = ? LIMIT 1")?;
        stmt.with_bindings(&capability.id.as_str())?;
        let rows: Vec<String> = stmt
            .map(|s| s.column_text(0).map(|value| value.to_string()))?
            .into_iter()
            .collect();
        rows.into_iter().next()
    };

    if existing.is_some() {
        let mut stmt = Statement::prepare(
            conn,
            "UPDATE capabilities
             SET name = ?, description = ?, workflow_id = ?, workflow_version = ?,
                 input_schema_json = ?, output_schema_json = ?, enabled = ?,
                 updated_at = CURRENT_TIMESTAMP
             WHERE id = ?",
        )?;
        stmt.with_bindings(&(
            capability.name.as_str(),
            capability.description.as_str(),
            capability.workflow_id.as_str(),
            capability.workflow_version,
            capability.input_schema_json.as_str(),
            capability.output_schema_json.as_str(),
            enabled,
            capability.id.as_str(),
        ))?;
        stmt.exec()?;
        return Ok(());
    }

    let mut stmt = Statement::prepare(
        conn,
        "INSERT INTO capabilities
         (id, name, description, workflow_id, workflow_version, input_schema_json, output_schema_json, enabled)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )?;
    stmt.with_bindings(&(
        capability.id.as_str(),
        capability.name.as_str(),
        capability.description.as_str(),
        capability.workflow_id.as_str(),
        capability.workflow_version,
        capability.input_schema_json.as_str(),
        capability.output_schema_json.as_str(),
        enabled,
    ))?;
    stmt.exec()?;
    Ok(())
}

pub fn load_enabled_capabilities(conn: &Connection) -> Result<Vec<CapabilityRow>> {
    ensure_workflow_tables(conn)?;
    let mut stmt = Statement::prepare(
        conn,
        "SELECT id, name, COALESCE(description, ''), workflow_id, workflow_version,
                COALESCE(input_schema_json, '{}'), COALESCE(output_schema_json, '{}'), enabled
         FROM capabilities
         WHERE enabled = 1
         ORDER BY name ASC, id ASC",
    )?;
    stmt.map(|s| {
        Ok(CapabilityRow {
            id: s.column_text(0)?.to_string(),
            name: s.column_text(1)?.to_string(),
            description: s.column_text(2)?.to_string(),
            workflow_id: s.column_text(3)?.to_string(),
            workflow_version: s.column_int64(4)?,
            input_schema_json: s.column_text(5)?.to_string(),
            output_schema_json: s.column_text(6)?.to_string(),
            enabled: s.column_int64(7)? != 0,
        })
    })
}

pub fn update_capability_workflow_version(
    conn: &Connection,
    capability_id: &str,
    workflow_version: i64,
) -> Result<()> {
    ensure_workflow_tables(conn)?;
    let mut stmt = Statement::prepare(
        conn,
        "UPDATE capabilities
         SET workflow_version = ?, updated_at = CURRENT_TIMESTAMP
         WHERE id = ?",
    )?;
    stmt.with_bindings(&(workflow_version, capability_id))?;
    stmt.exec()?;
    Ok(())
}

pub fn insert_workflow_run(
    conn: &Connection,
    workflow_id: &str,
    workflow_version: i64,
) -> Result<usize> {
    ensure_workflow_tables(conn)?;
    let mut stmt = Statement::prepare(
        conn,
        "INSERT INTO workflow_runs (workflow_id, workflow_version, status) VALUES (?, ?, 'running')",
    )?;
    stmt.with_bindings(&(workflow_id, workflow_version))?;
    stmt.exec()?;
    let mut stmt = Statement::prepare(conn, "SELECT last_insert_rowid()")?;
    let id = stmt.map(|s| s.column_int64(0))?.into_iter().next().unwrap();
    Ok(id as usize)
}

pub fn insert_workflow_run_event(
    conn: &Connection,
    run_id: usize,
    kind: &str,
    payload: &str,
) -> Result<usize> {
    ensure_workflow_tables(conn)?;
    let mut stmt = Statement::prepare(
        conn,
        "INSERT INTO workflow_run_events (run_id, kind, payload) VALUES (?, ?, ?)",
    )?;
    stmt.with_bindings(&(run_id, kind, payload))?;
    stmt.exec()?;
    let mut stmt = Statement::prepare(conn, "SELECT last_insert_rowid()")?;
    let id = stmt.map(|s| s.column_int64(0))?.into_iter().next().unwrap();
    Ok(id as usize)
}

pub fn finish_workflow_run(
    conn: &Connection,
    run_id: usize,
    status: &str,
    error: Option<&str>,
) -> Result<()> {
    ensure_workflow_tables(conn)?;
    let mut stmt = Statement::prepare(
        conn,
        "UPDATE workflow_runs
         SET status = ?, error = ?, finished_at = CURRENT_TIMESTAMP
         WHERE id = ?",
    )?;
    stmt.with_bindings(&(status, error, run_id))?;
    stmt.exec()?;
    Ok(())
}

pub fn load_workflow_run(conn: &Connection, run_id: usize) -> Result<Option<WorkflowRunRow>> {
    ensure_workflow_tables(conn)?;
    let mut stmt = Statement::prepare(
        conn,
        "SELECT id, workflow_id, workflow_version, status, COALESCE(error, '')
         FROM workflow_runs
         WHERE id = ?
         LIMIT 1",
    )?;
    stmt.with_bindings(&run_id)?;
    let rows: Vec<WorkflowRunRow> = stmt.map(|s| {
        Ok(WorkflowRunRow {
            id: s.column_int64(0)? as usize,
            workflow_id: s.column_text(1)?.to_string(),
            workflow_version: s.column_int64(2)?,
            status: s.column_text(3)?.to_string(),
            error: s.column_text(4)?.to_string(),
        })
    })?;
    Ok(rows.into_iter().next())
}

pub fn load_recent_workflow_runs(conn: &Connection, limit: usize) -> Result<Vec<WorkflowRunRow>> {
    ensure_workflow_tables(conn)?;
    let limit = limit.max(1) as i64;
    let mut stmt = Statement::prepare(
        conn,
        "SELECT id, workflow_id, workflow_version, status, COALESCE(error, '')
         FROM workflow_runs
         ORDER BY id DESC
         LIMIT ?",
    )?;
    stmt.with_bindings(&limit)?;
    stmt.map(|s| {
        Ok(WorkflowRunRow {
            id: s.column_int64(0)? as usize,
            workflow_id: s.column_text(1)?.to_string(),
            workflow_version: s.column_int64(2)?,
            status: s.column_text(3)?.to_string(),
            error: s.column_text(4)?.to_string(),
        })
    })
}

pub fn load_workflow_run_events(
    conn: &Connection,
    run_id: usize,
) -> Result<Vec<WorkflowRunEventRow>> {
    ensure_workflow_tables(conn)?;
    let mut stmt = Statement::prepare(
        conn,
        "SELECT id, run_id, kind, payload
         FROM workflow_run_events
         WHERE run_id = ?
         ORDER BY id ASC",
    )?;
    stmt.with_bindings(&run_id)?;
    stmt.map(|s| {
        Ok(WorkflowRunEventRow {
            id: s.column_int64(0)? as usize,
            run_id: s.column_int64(1)? as usize,
            kind: s.column_text(2)?.to_string(),
            payload: s.column_text(3)?.to_string(),
        })
    })
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
    let mut stmt = Statement::prepare(conn, "SELECT COUNT(*) FROM messages WHERE task_id = ?")?;
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
pub fn load_recent_messages(
    conn: &Connection,
    task_id: usize,
    limit: usize,
) -> Result<Vec<MessageRow>> {
    let mut stmt = Statement::prepare(
        conn,
        "SELECT role, content FROM messages WHERE task_id = ? ORDER BY created_at DESC LIMIT ?",
    )?;
    let limit_i64 = limit as i64;
    stmt.with_bindings(&(task_id, limit_i64))?;
    let mut rows: Vec<MessageRow> = stmt.map(|s| {
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
    pub default_task_id: Option<usize>,
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
    let mut stmt = Statement::prepare(
        conn,
        "SELECT id, name, path, expanded, COALESCE(default_task_id, 0) FROM workspaces",
    )?;
    stmt.map(|s| {
        let id = s.column_int64(0)? as usize;
        let name = s.column_text(1)?.to_string();
        let path = s.column_text(2)?.to_string();
        let expanded = s.column_int64(3)? != 0;
        let default_task_id = match s.column_int64(4).unwrap_or(0) {
            id if id > 0 => Some(id as usize),
            _ => None,
        };
        Ok(WorkspaceRow {
            id,
            name,
            path,
            expanded,
            default_task_id,
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
        stmt.map(|s| s.column_int64(0))?.into_iter().collect()
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

    // 2.5 删除编码工作流和产物记录
    {
        let mut stmt = Statement::prepare(conn, "DELETE FROM task_artifacts WHERE task_id = ?")?;
        stmt.with_bindings(&task_id)?;
        stmt.exec()?;
    }
    {
        let mut stmt = Statement::prepare(conn, "DELETE FROM coding_workflows WHERE task_id = ?")?;
        stmt.with_bindings(&task_id)?;
        stmt.exec()?;
    }

    // 3. 删除 agent_instances 及关联的 agent_conversations
    let instance_ids: Vec<i64> = {
        let mut stmt =
            Statement::prepare(conn, "SELECT id FROM agent_instances WHERE task_id = ?")?;
        stmt.with_bindings(&task_id)?;
        stmt.map(|s| s.column_int64(0))?.into_iter().collect()
    };
    for inst_id in &instance_ids {
        let mut stmt = Statement::prepare(
            conn,
            "DELETE FROM agent_conversations WHERE agent_instance_id = ?",
        )?;
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

pub fn update_workspace_default_task(
    conn: &Connection,
    workspace_id: usize,
    task_id: Option<usize>,
) -> Result<()> {
    let task_id_i64 = task_id.map(|id| id as i64).unwrap_or(0);
    let mut stmt = Statement::prepare(
        conn,
        "UPDATE workspaces SET default_task_id = NULLIF(?, 0) WHERE id = ?",
    )?;
    stmt.with_bindings(&(task_id_i64, workspace_id))?;
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

// ---------- coding_workflows / task_artifacts ----------

pub fn insert_coding_workflow(
    conn: &Connection,
    task_id: usize,
    stage: &str,
    user_request: &str,
    plan_path: &str,
    log_path: &str,
) -> Result<usize> {
    let mut stmt = Statement::prepare(
        conn,
        "INSERT INTO coding_workflows (task_id, stage, user_request, plan_path, log_path) VALUES (?, ?, ?, ?, ?)",
    )?;
    stmt.with_bindings(&(task_id, stage, user_request, plan_path, log_path))?;
    stmt.exec()?;
    let mut stmt = Statement::prepare(conn, "SELECT last_insert_rowid()")?;
    let id = stmt.map(|s| s.column_int64(0))?.into_iter().next().unwrap();
    Ok(id as usize)
}

pub fn update_coding_workflow_stage(
    conn: &Connection,
    workflow_id: usize,
    stage: &str,
) -> Result<()> {
    let mut stmt = Statement::prepare(
        conn,
        "UPDATE coding_workflows SET stage = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
    )?;
    stmt.with_bindings(&(stage, workflow_id))?;
    stmt.exec()?;
    Ok(())
}

pub fn update_coding_workflow_context(
    conn: &Connection,
    workflow_id: usize,
    main_agent_summary: &str,
    known_constraints_json: &str,
    suggested_direction: Option<&str>,
    clarification_focus_json: &str,
    approval_notes_json: &str,
    last_error: Option<&str>,
) -> Result<()> {
    let mut stmt = Statement::prepare(
        conn,
        "UPDATE coding_workflows
         SET main_agent_summary = ?,
             known_constraints_json = ?,
             suggested_direction = ?,
             clarification_focus_json = ?,
             approval_notes_json = ?,
             last_error = ?,
             updated_at = CURRENT_TIMESTAMP
         WHERE id = ?",
    )?;
    stmt.with_bindings(&(
        main_agent_summary,
        known_constraints_json,
        suggested_direction,
        clarification_focus_json,
        approval_notes_json,
        last_error,
        workflow_id,
    ))?;
    stmt.exec()?;
    Ok(())
}

pub fn upsert_task_artifact(
    conn: &Connection,
    task_id: usize,
    workflow_id: Option<usize>,
    kind: &str,
    path: &str,
    title: &str,
) -> Result<usize> {
    upsert_task_artifact_with_metadata(conn, task_id, workflow_id, kind, path, title, "ready", None)
}

pub fn upsert_task_artifact_with_metadata(
    conn: &Connection,
    task_id: usize,
    workflow_id: Option<usize>,
    kind: &str,
    path: &str,
    title: &str,
    status: &str,
    metadata_json: Option<&str>,
) -> Result<usize> {
    let workflow_id_val = workflow_id.map(|id| id as i64);
    let existing = {
        let mut stmt = Statement::prepare(
            conn,
            "SELECT id FROM task_artifacts WHERE task_id = ? AND path = ? ORDER BY id LIMIT 1",
        )?;
        stmt.with_bindings(&(task_id, path))?;
        let rows: Vec<usize> = stmt
            .map(|s| s.column_int64(0).map(|v| v as usize))?
            .into_iter()
            .collect();
        rows.into_iter().next()
    };

    if let Some(id) = existing {
        let mut stmt = Statement::prepare(
            conn,
            "UPDATE task_artifacts
             SET workflow_id = ?, kind = ?, title = ?, status = ?, metadata_json = ?, updated_at = CURRENT_TIMESTAMP
             WHERE id = ?",
        )?;
        stmt.with_bindings(&(workflow_id_val, kind, title, status, metadata_json, id))?;
        stmt.exec()?;
        return Ok(id);
    }

    let mut stmt = Statement::prepare(
        conn,
        "INSERT INTO task_artifacts (task_id, workflow_id, kind, path, title, status, metadata_json)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )?;
    stmt.with_bindings(&(
        task_id,
        workflow_id_val,
        kind,
        path,
        title,
        status,
        metadata_json,
    ))?;
    stmt.exec()?;
    let mut stmt = Statement::prepare(conn, "SELECT last_insert_rowid()")?;
    let id = stmt.map(|s| s.column_int64(0))?.into_iter().next().unwrap();
    Ok(id as usize)
}

#[derive(Debug, Clone)]
pub struct TaskArtifactRow {
    pub id: usize,
    pub task_id: usize,
    pub workflow_id: Option<usize>,
    pub kind: String,
    pub path: String,
    pub title: String,
    pub status: String,
    pub metadata_json: String,
    pub updated_at: String,
}

pub fn load_task_artifacts(conn: &Connection, task_id: usize) -> Result<Vec<TaskArtifactRow>> {
    let mut stmt = Statement::prepare(
        conn,
        "SELECT id, task_id, workflow_id, kind, path, COALESCE(title, ''), status, COALESCE(metadata_json, ''), updated_at
         FROM task_artifacts
         WHERE task_id = ?
         ORDER BY updated_at DESC, id DESC",
    )?;
    stmt.with_bindings(&task_id)?;
    stmt.map(|s| {
        let workflow_id = s.column_int64(2).ok().map(|v| v as usize);
        Ok(TaskArtifactRow {
            id: s.column_int64(0)? as usize,
            task_id: s.column_int64(1)? as usize,
            workflow_id,
            kind: s.column_text(3)?.to_string(),
            path: s.column_text(4)?.to_string(),
            title: s.column_text(5)?.to_string(),
            status: s.column_text(6)?.to_string(),
            metadata_json: s.column_text(7)?.to_string(),
            updated_at: s.column_text(8)?.to_string(),
        })
    })
}

#[derive(Debug, Clone)]
pub struct CodingWorkflowRow {
    pub id: usize,
    pub task_id: usize,
    pub stage: String,
    pub user_request: String,
    pub main_agent_summary: String,
    pub known_constraints_json: String,
    pub suggested_direction: String,
    pub clarification_focus_json: String,
    pub plan_path: String,
    pub log_path: String,
    pub approval_notes_json: String,
    pub last_error: String,
}

pub fn load_latest_coding_workflow(
    conn: &Connection,
    task_id: usize,
) -> Result<Option<CodingWorkflowRow>> {
    let mut stmt = Statement::prepare(
        conn,
        "SELECT id,
                task_id,
                stage,
                user_request,
                COALESCE(main_agent_summary, ''),
                COALESCE(known_constraints_json, '[]'),
                COALESCE(suggested_direction, ''),
                COALESCE(clarification_focus_json, '[]'),
                COALESCE(plan_path, ''),
                COALESCE(log_path, ''),
                COALESCE(approval_notes_json, '[]'),
                COALESCE(last_error, '')
         FROM coding_workflows
         WHERE task_id = ?
         ORDER BY updated_at DESC, id DESC
         LIMIT 1",
    )?;
    stmt.with_bindings(&task_id)?;
    let rows: Vec<CodingWorkflowRow> = stmt.map(|s| {
        Ok(CodingWorkflowRow {
            id: s.column_int64(0)? as usize,
            task_id: s.column_int64(1)? as usize,
            stage: s.column_text(2)?.to_string(),
            user_request: s.column_text(3)?.to_string(),
            main_agent_summary: s.column_text(4)?.to_string(),
            known_constraints_json: s.column_text(5)?.to_string(),
            suggested_direction: s.column_text(6)?.to_string(),
            clarification_focus_json: s.column_text(7)?.to_string(),
            plan_path: s.column_text(8)?.to_string(),
            log_path: s.column_text(9)?.to_string(),
            approval_notes_json: s.column_text(10)?.to_string(),
            last_error: s.column_text(11)?.to_string(),
        })
    })?;
    Ok(rows.into_iter().next())
}

#[cfg(test)]
mod coding_workflow_tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_conn() -> Connection {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "one-task-db-test-{}-{}.db",
            std::process::id(),
            nanos
        ));
        let conn = Connection::open_file(path.to_str().unwrap());
        let _ = (conn
            .exec(
                "CREATE TABLE workspaces (
                    id INTEGER PRIMARY KEY,
                    name TEXT NOT NULL,
                    path TEXT NOT NULL,
                    expanded INTEGER DEFAULT 0,
                    default_task_id INTEGER
                )",
            )
            .unwrap())();
        let _ = (conn
            .exec(
                "CREATE TABLE tasks (
                    id INTEGER PRIMARY KEY,
                    workspace_id INTEGER NOT NULL,
                    title TEXT NOT NULL,
                    status TEXT NOT NULL DEFAULT 'todo',
                    is_draft INTEGER DEFAULT 0
                )",
            )
            .unwrap())();
        let _ = (conn
            .exec(
                "CREATE TABLE task_runs (
                    id INTEGER PRIMARY KEY,
                    task_id INTEGER NOT NULL,
                    kind TEXT NOT NULL,
                    status TEXT NOT NULL DEFAULT 'running',
                    started_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                    finished_at TIMESTAMP
                )",
            )
            .unwrap())();
        let _ = (conn
            .exec(
                "CREATE TABLE messages (
                    id INTEGER PRIMARY KEY,
                    task_id INTEGER NOT NULL,
                    role TEXT NOT NULL,
                    content TEXT NOT NULL,
                    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
                )",
            )
            .unwrap())();
        let _ = (conn
            .exec(
                "CREATE TABLE agent_instances (
                    id INTEGER PRIMARY KEY,
                    agent_id INTEGER NOT NULL,
                    task_id INTEGER,
                    status TEXT NOT NULL DEFAULT 'idle',
                    session_state_json TEXT,
                    last_active_at TIMESTAMP
                )",
            )
            .unwrap())();
        let _ = (conn
            .exec(
                "CREATE TABLE agent_conversations (
                    id INTEGER PRIMARY KEY,
                    agent_instance_id INTEGER NOT NULL,
                    user_query TEXT NOT NULL,
                    agent_response TEXT,
                    context_snapshot TEXT,
                    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
                )",
            )
            .unwrap())();
        let _ = (conn
            .exec(
                "CREATE TABLE run_events (
                    id INTEGER PRIMARY KEY,
                    run_id INTEGER NOT NULL,
                    kind TEXT NOT NULL,
                    payload TEXT NOT NULL,
                    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
                )",
            )
            .unwrap())();
        let _ = (conn
            .exec(
                "CREATE TABLE coding_workflows (
                    id INTEGER PRIMARY KEY,
                    task_id INTEGER NOT NULL,
                    stage TEXT NOT NULL,
                    user_request TEXT NOT NULL,
                    main_agent_summary TEXT,
                    known_constraints_json TEXT,
                    suggested_direction TEXT,
                    clarification_focus_json TEXT,
                    plan_path TEXT,
                    log_path TEXT,
                    approval_notes_json TEXT,
                    last_error TEXT,
                    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
                )",
            )
            .unwrap())();
        let _ = (conn
            .exec(
                "CREATE TABLE task_artifacts (
                    id INTEGER PRIMARY KEY,
                    task_id INTEGER NOT NULL,
                    workflow_id INTEGER,
                    kind TEXT NOT NULL,
                    path TEXT NOT NULL,
                    title TEXT,
                    status TEXT NOT NULL DEFAULT 'ready',
                    metadata_json TEXT,
                    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
                )",
            )
            .unwrap())();
        conn
    }

    #[test]
    fn coding_workflow_round_trips_latest_stage() {
        let conn = test_conn();
        let workspace_id = insert_workspace(&conn, "Workspace", "/tmp/workspace").unwrap();
        let task_id = insert_task(&conn, workspace_id, "Build app").unwrap();

        let older_id = insert_coding_workflow(
            &conn,
            task_id,
            "planning_running",
            "make hello world",
            "/tmp/workspace/plan.md",
            "/tmp/workspace/claude.log",
        )
        .unwrap();
        let newer_id = insert_coding_workflow(
            &conn,
            task_id,
            "awaiting_approval",
            "make login page",
            "/tmp/workspace/plan-2.md",
            "/tmp/workspace/claude-2.log",
        )
        .unwrap();
        update_coding_workflow_stage(&conn, older_id, "failed").unwrap();

        let latest = load_latest_coding_workflow(&conn, task_id)
            .unwrap()
            .unwrap();
        assert_eq!(latest.id, newer_id);
        assert_eq!(latest.task_id, task_id);
        assert_eq!(latest.stage, "awaiting_approval");
        assert_eq!(latest.user_request, "make login page");
        assert_eq!(latest.main_agent_summary, "");
        assert_eq!(latest.known_constraints_json, "[]");
        assert_eq!(latest.suggested_direction, "");
        assert_eq!(latest.clarification_focus_json, "[]");
        assert_eq!(latest.plan_path, "/tmp/workspace/plan-2.md");
        assert_eq!(latest.log_path, "/tmp/workspace/claude-2.log");
        assert_eq!(latest.approval_notes_json, "[]");
        assert_eq!(latest.last_error, "");
    }

    #[test]
    fn workspace_default_task_round_trips() {
        let conn = test_conn();
        let workspace_id = insert_workspace(&conn, "Workspace", "/tmp/workspace").unwrap();
        let task_id = insert_task(&conn, workspace_id, "Build app").unwrap();

        update_workspace_default_task(&conn, workspace_id, Some(task_id)).unwrap();
        let workspaces = load_workspaces(&conn).unwrap();
        let workspace = workspaces
            .iter()
            .find(|workspace| workspace.id == workspace_id)
            .unwrap();
        assert_eq!(workspace.default_task_id, Some(task_id));

        update_workspace_default_task(&conn, workspace_id, None).unwrap();
        let workspaces = load_workspaces(&conn).unwrap();
        let workspace = workspaces
            .iter()
            .find(|workspace| workspace.id == workspace_id)
            .unwrap();
        assert_eq!(workspace.default_task_id, None);
    }

    #[test]
    fn coding_workflow_context_round_trips() {
        let conn = test_conn();
        let workspace_id = insert_workspace(&conn, "Workspace", "/tmp/workspace").unwrap();
        let task_id = insert_task(&conn, workspace_id, "Build app").unwrap();
        let workflow_id = insert_coding_workflow(
            &conn,
            task_id,
            "awaiting_approval",
            "make dashboard",
            "/tmp/workspace/plan.md",
            "/tmp/workspace/claude.log",
        )
        .unwrap();

        update_coding_workflow_context(
            &conn,
            workflow_id,
            "main summary",
            r#"["must use GPUI"]"#,
            Some("reuse existing layout"),
            r#"["confirm copy"]"#,
            r#"["make it tighter"]"#,
            Some("last failure"),
        )
        .unwrap();

        let latest = load_latest_coding_workflow(&conn, task_id)
            .unwrap()
            .unwrap();
        assert_eq!(latest.main_agent_summary, "main summary");
        assert_eq!(latest.known_constraints_json, r#"["must use GPUI"]"#);
        assert_eq!(latest.suggested_direction, "reuse existing layout");
        assert_eq!(latest.clarification_focus_json, r#"["confirm copy"]"#);
        assert_eq!(latest.approval_notes_json, r#"["make it tighter"]"#);
        assert_eq!(latest.last_error, "last failure");
    }

    #[test]
    fn task_artifact_upsert_updates_existing_path() {
        let conn = test_conn();
        let workspace_id = insert_workspace(&conn, "Workspace", "/tmp/workspace").unwrap();
        let task_id = insert_task(&conn, workspace_id, "Build app").unwrap();
        let workflow_id = insert_coding_workflow(
            &conn,
            task_id,
            "awaiting_approval",
            "make hello world",
            "/tmp/workspace/plan.md",
            "/tmp/workspace/claude.log",
        )
        .unwrap();

        let first_id = upsert_task_artifact(
            &conn,
            task_id,
            Some(workflow_id),
            "claude_plan",
            "/tmp/workspace/plan.md",
            "Old plan",
        )
        .unwrap();
        let second_id = upsert_task_artifact(
            &conn,
            task_id,
            Some(workflow_id),
            "claude_plan",
            "/tmp/workspace/plan.md",
            "Updated plan",
        )
        .unwrap();

        assert_eq!(first_id, second_id);
        let artifacts = load_task_artifacts(&conn, task_id).unwrap();
        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[0].id, first_id);
        assert_eq!(artifacts[0].workflow_id, Some(workflow_id));
        assert_eq!(artifacts[0].kind, "claude_plan");
        assert_eq!(artifacts[0].path, "/tmp/workspace/plan.md");
        assert_eq!(artifacts[0].title, "Updated plan");
        assert_eq!(artifacts[0].status, "ready");
        assert_eq!(artifacts[0].metadata_json, "");
    }

    #[test]
    fn delete_task_removes_workflows_and_artifacts() {
        let conn = test_conn();
        let workspace_id = insert_workspace(&conn, "Workspace", "/tmp/workspace").unwrap();
        let task_id = insert_task(&conn, workspace_id, "Build app").unwrap();
        let workflow_id = insert_coding_workflow(
            &conn,
            task_id,
            "awaiting_approval",
            "make hello world",
            "/tmp/workspace/plan.md",
            "/tmp/workspace/claude.log",
        )
        .unwrap();
        upsert_task_artifact(
            &conn,
            task_id,
            Some(workflow_id),
            "claude_log",
            "/tmp/workspace/claude.log",
            "Claude log",
        )
        .unwrap();

        delete_task(&conn, task_id).unwrap();

        assert!(load_latest_coding_workflow(&conn, task_id)
            .unwrap()
            .is_none());
        assert!(load_task_artifacts(&conn, task_id).unwrap().is_empty());
    }

    #[test]
    fn workflow_run_records_events_and_finish_status() {
        let conn = test_conn();
        ensure_workflow_tables(&conn).unwrap();
        let _ = (conn
            .exec(
                "INSERT INTO workflows (id, name, description, status, version, definition_json)
                 VALUES ('workflow.echo', 'Echo', '', 'draft', 1, '{}')",
            )
            .unwrap())();

        let run_id = insert_workflow_run(&conn, "workflow.echo", 1).unwrap();
        insert_workflow_run_event(
            &conn,
            run_id,
            "run_started",
            r#"{"input":{"message":"hello"}}"#,
        )
        .unwrap();
        insert_workflow_run_event(&conn, run_id, "run_finished", r#"{"ok":true}"#).unwrap();
        finish_workflow_run(&conn, run_id, "succeeded", None).unwrap();

        let run = load_workflow_run(&conn, run_id).unwrap().unwrap();
        assert_eq!(run.id, run_id);
        assert_eq!(run.workflow_id, "workflow.echo");
        assert_eq!(run.workflow_version, 1);
        assert_eq!(run.status, "succeeded");
        assert_eq!(run.error, "");

        let second_run_id = insert_workflow_run(&conn, "workflow.echo", 1).unwrap();
        let recent = load_recent_workflow_runs(&conn, 1).unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].id, second_run_id);
        assert_eq!(recent[0].status, "running");

        let events = load_workflow_run_events(&conn, run_id).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].run_id, run_id);
        assert_eq!(events[0].kind, "run_started");
        assert_eq!(events[0].payload, r#"{"input":{"message":"hello"}}"#);
        assert_eq!(events[1].kind, "run_finished");
    }
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

pub(crate) fn insert_task_run(conn: &Connection, task_id: usize, kind: &str) -> Result<usize> {
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
