use anyhow::Result;
use std::path::PathBuf;
use sqlez::{
    connection::Connection,
    statement::Statement,
};

pub struct Database {
    pub conn: Connection,
}

impl Database {
    pub fn new() -> Result<Self> {
        let db_path = get_db_path();
        let conn = Connection::open_file(db_path.to_str().unwrap_or("solo3.db"));
        let conn_ref = &conn;

        // Create tables
        (conn_ref.exec("CREATE TABLE IF NOT EXISTS workspaces (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            path TEXT NOT NULL,
            expanded INTEGER DEFAULT 0
        )").unwrap())();

        (conn_ref.exec("CREATE TABLE IF NOT EXISTS tasks (
            id INTEGER PRIMARY KEY,
            workspace_id INTEGER NOT NULL,
            title TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'todo',
            FOREIGN KEY (workspace_id) REFERENCES workspaces(id)
        )").unwrap())();

        Ok(Self { conn })
    }
}

fn get_db_path() -> PathBuf {
    let config_dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".solo3_gpui");
    std::fs::create_dir_all(&config_dir).ok();
    config_dir.join("solo3.db")
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
    pub workspace_id: usize,
    pub title: String,
    pub status: String,
}

pub fn load_workspaces(conn: &Connection) -> Result<Vec<WorkspaceRow>> {
    let mut stmt = Statement::prepare(conn, "SELECT id, name, path, expanded FROM workspaces")?;
    stmt.map(|s| {
        let id = s.column_int64(0)? as usize;
        let name = s.column_text(1)?.to_string();
        let path = s.column_text(2)?.to_string();
        let expanded = s.column_int64(3)? != 0;
        Ok(WorkspaceRow { id, name, path, expanded })
    })
}

pub fn load_tasks(conn: &Connection, workspace_id: usize) -> Result<Vec<TaskRow>> {
    let mut stmt = Statement::prepare(conn, "SELECT id, workspace_id, title, status FROM tasks WHERE workspace_id = ?")?;
    stmt.with_bindings(&workspace_id)?;
    stmt.map(|s| {
        let id = s.column_int64(0)? as usize;
        let workspace_id = s.column_int64(1)? as usize;
        let title = s.column_text(2)?.to_string();
        let status = s.column_text(3)?.to_string();
        Ok(TaskRow { id, workspace_id, title, status })
    })
}

pub fn insert_workspace(conn: &Connection, name: &str, path: &str) -> Result<usize> {
    let mut stmt = Statement::prepare(conn, "INSERT INTO workspaces (name, path, expanded) VALUES (?, ?, 0)")?;
    stmt.with_bindings(&(name, path))?;
    stmt.exec()?;
    let mut stmt = Statement::prepare(conn, "SELECT last_insert_rowid()")?;
    let id = stmt.map(|s| s.column_int64(0))?.into_iter().next().unwrap();
    Ok(id as usize)
}

pub fn insert_task(conn: &Connection, workspace_id: usize, title: &str) -> Result<usize> {
    let mut stmt = Statement::prepare(conn, "INSERT INTO tasks (workspace_id, title, status) VALUES (?, ?, 'todo')")?;
    stmt.with_bindings(&(workspace_id, title))?;
    stmt.exec()?;
    let mut stmt = Statement::prepare(conn, "SELECT last_insert_rowid()")?;
    let id = stmt.map(|s| s.column_int64(0))?.into_iter().next().unwrap();
    Ok(id as usize)
}

pub fn update_task_status(conn: &Connection, task_id: usize, status: &str) -> Result<()> {
    let mut stmt = Statement::prepare(conn, "UPDATE tasks SET status = ? WHERE id = ?")?;
    stmt.with_bindings(&(status, task_id))?;
    stmt.exec()?;
    Ok(())
}

pub fn delete_task(conn: &Connection, task_id: usize) -> Result<()> {
    let mut stmt = Statement::prepare(conn, "DELETE FROM tasks WHERE id = ?")?;
    stmt.with_bindings(&task_id)?;
    stmt.exec()?;
    Ok(())
}

pub fn update_workspace_expanded(conn: &Connection, workspace_id: usize, expanded: bool) -> Result<()> {
    let mut stmt = Statement::prepare(conn, "UPDATE workspaces SET expanded = ? WHERE id = ?")?;
    let expanded_i64 = expanded as i64;
    stmt.with_bindings(&(expanded_i64, workspace_id))?;
    stmt.exec()?;
    Ok(())
}