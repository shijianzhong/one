use anyhow::{Context, Result};
use sqlez::{connection::Connection, statement::Statement};

use crate::task_db::{ensure_workflow_tables, upsert_capability, CapabilityRow};

use super::definition::{WorkflowDefinition, WorkflowStatus};

#[derive(Debug, Clone)]
pub struct WorkflowSummary {
    pub id: String,
    pub name: String,
    pub description: String,
    pub status: WorkflowStatus,
    pub version: i64,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct WorkflowVersionSummary {
    pub workflow_id: String,
    pub version: i64,
}

pub struct WorkflowStore<'a> {
    conn: &'a Connection,
}

impl<'a> WorkflowStore<'a> {
    pub fn new(conn: &'a Connection) -> Result<Self> {
        ensure_workflow_tables(conn)?;
        Ok(Self { conn })
    }

    pub fn create_draft(&self, id: &str, name: &str) -> Result<WorkflowDefinition> {
        let definition = WorkflowDefinition::new_draft(id, name);
        self.save_draft(&definition)?;
        Ok(definition)
    }

    pub fn save_draft(&self, definition: &WorkflowDefinition) -> Result<()> {
        if definition.status != WorkflowStatus::Draft {
            anyhow::bail!("only draft workflows can be saved through save_draft");
        }
        if definition.id.trim().is_empty() {
            anyhow::bail!("workflow id cannot be empty");
        }
        if definition.name.trim().is_empty() {
            anyhow::bail!("workflow name cannot be empty");
        }

        let definition_json = serde_json::to_string(definition)
            .with_context(|| format!("failed to serialize workflow {}", definition.id))?;
        let status = status_to_str(&definition.status);
        let existing = self.load(&definition.id)?.is_some();

        if existing {
            let mut stmt = Statement::prepare(
                self.conn,
                "UPDATE workflows
                 SET name = ?, description = ?, status = ?, version = ?, definition_json = ?, updated_at = CURRENT_TIMESTAMP
                 WHERE id = ?",
            )?;
            stmt.with_bindings(&(
                definition.name.as_str(),
                definition.description.as_str(),
                status,
                definition.version,
                definition_json.as_str(),
                definition.id.as_str(),
            ))?;
            stmt.exec()?;
            return Ok(());
        }

        let mut stmt = Statement::prepare(
            self.conn,
            "INSERT INTO workflows (id, name, description, status, version, definition_json)
             VALUES (?, ?, ?, ?, ?, ?)",
        )?;
        stmt.with_bindings(&(
            definition.id.as_str(),
            definition.name.as_str(),
            definition.description.as_str(),
            status,
            definition.version,
            definition_json.as_str(),
        ))?;
        stmt.exec()?;
        Ok(())
    }

    pub fn load(&self, id: &str) -> Result<Option<WorkflowDefinition>> {
        let mut stmt = Statement::prepare(
            self.conn,
            "SELECT definition_json FROM workflows WHERE id = ? LIMIT 1",
        )?;
        stmt.with_bindings(&id)?;
        let rows: Vec<String> = stmt
            .map(|s| s.column_text(0).map(|v| v.to_string()))?
            .into_iter()
            .collect();
        rows.into_iter()
            .next()
            .map(|raw| {
                serde_json::from_str(&raw)
                    .with_context(|| format!("failed to parse workflow definition {}", id))
            })
            .transpose()
    }

    pub fn load_version(&self, id: &str, version: i64) -> Result<Option<WorkflowDefinition>> {
        let mut stmt = Statement::prepare(
            self.conn,
            "SELECT definition_json FROM workflow_versions
             WHERE workflow_id = ? AND version = ?
             LIMIT 1",
        )?;
        stmt.with_bindings(&(id, version))?;
        let rows: Vec<String> = stmt
            .map(|s| s.column_text(0).map(|value| value.to_string()))?
            .into_iter()
            .collect();
        rows.into_iter()
            .next()
            .map(|raw| {
                serde_json::from_str(&raw).with_context(|| {
                    format!("failed to parse workflow definition {} v{}", id, version)
                })
            })
            .transpose()
    }

    pub fn load_active_or_version(
        &self,
        id: &str,
        version: i64,
    ) -> Result<Option<WorkflowDefinition>> {
        if let Some(definition) = self.load_version(id, version)? {
            return Ok(Some(definition));
        }
        self.load(id)
    }

    pub fn list_versions(&self, workflow_id: &str) -> Result<Vec<WorkflowVersionSummary>> {
        let mut stmt = Statement::prepare(
            self.conn,
            "SELECT workflow_id, version FROM workflow_versions
             WHERE workflow_id = ?
             ORDER BY version DESC",
        )?;
        stmt.with_bindings(&workflow_id)?;
        stmt.map(|s| {
            Ok(WorkflowVersionSummary {
                workflow_id: s.column_text(0)?.to_string(),
                version: s.column_int64(1)?,
            })
        })
    }

    pub fn import_definition(&self, definition: &WorkflowDefinition) -> Result<()> {
        if definition.id.trim().is_empty() {
            anyhow::bail!("workflow id cannot be empty");
        }
        if definition.name.trim().is_empty() {
            anyhow::bail!("workflow name cannot be empty");
        }

        let definition_json = serde_json::to_string(definition)
            .with_context(|| format!("failed to serialize workflow {}", definition.id))?;
        let status = status_to_str(&definition.status);
        let existing = self.load(&definition.id)?.is_some();

        if existing {
            let mut stmt = Statement::prepare(
                self.conn,
                "UPDATE workflows
                 SET name = ?, description = ?, status = ?, version = ?, definition_json = ?, updated_at = CURRENT_TIMESTAMP
                 WHERE id = ?",
            )?;
            stmt.with_bindings(&(
                definition.name.as_str(),
                definition.description.as_str(),
                status,
                definition.version,
                definition_json.as_str(),
                definition.id.as_str(),
            ))?;
            stmt.exec()?;
        } else {
            let mut stmt = Statement::prepare(
                self.conn,
                "INSERT INTO workflows (id, name, description, status, version, definition_json)
                 VALUES (?, ?, ?, ?, ?, ?)",
            )?;
            stmt.with_bindings(&(
                definition.id.as_str(),
                definition.name.as_str(),
                definition.description.as_str(),
                status,
                definition.version,
                definition_json.as_str(),
            ))?;
            stmt.exec()?;
        }

        if definition.status == WorkflowStatus::Published {
            let mut stmt = Statement::prepare(
                self.conn,
                "INSERT OR REPLACE INTO workflow_versions (workflow_id, version, definition_json)
                 VALUES (?, ?, ?)",
            )?;
            stmt.with_bindings(&(
                definition.id.as_str(),
                definition.version,
                definition_json.as_str(),
            ))?;
            stmt.exec()?;
        }

        Ok(())
    }

    pub fn list_drafts(&self) -> Result<Vec<WorkflowSummary>> {
        let mut stmt = Statement::prepare(
            self.conn,
            "SELECT id, name, COALESCE(description, ''), status, version, updated_at
             FROM workflows
             WHERE status = 'draft'
             ORDER BY updated_at DESC, id ASC",
        )?;
        stmt.map(|s| {
            Ok(WorkflowSummary {
                id: s.column_text(0)?.to_string(),
                name: s.column_text(1)?.to_string(),
                description: s.column_text(2)?.to_string(),
                status: status_from_str(s.column_text(3)?),
                version: s.column_int64(4)?,
                updated_at: s.column_text(5)?.to_string(),
            })
        })
    }

    pub fn publish_as_capability(
        &self,
        workflow_id: &str,
        capability_id: &str,
        capability_name: &str,
        capability_description: &str,
    ) -> Result<()> {
        let Some(mut definition) = self.load(workflow_id)? else {
            anyhow::bail!("workflow '{}' not found", workflow_id);
        };
        if capability_id.trim().is_empty() {
            anyhow::bail!("capability id cannot be empty");
        }
        if capability_name.trim().is_empty() {
            anyhow::bail!("capability name cannot be empty");
        }

        definition.status = WorkflowStatus::Published;
        let definition_json = serde_json::to_string(&definition)
            .with_context(|| format!("failed to serialize workflow {}", definition.id))?;

        let mut stmt = Statement::prepare(
            self.conn,
            "INSERT OR REPLACE INTO workflow_versions (workflow_id, version, definition_json)
             VALUES (?, ?, ?)",
        )?;
        stmt.with_bindings(&(
            definition.id.as_str(),
            definition.version,
            definition_json.as_str(),
        ))?;
        stmt.exec()?;

        let mut stmt = Statement::prepare(
            self.conn,
            "UPDATE workflows
             SET status = 'published', version = ?, definition_json = ?, updated_at = CURRENT_TIMESTAMP
             WHERE id = ?",
        )?;
        stmt.with_bindings(&(
            definition.version,
            definition_json.as_str(),
            definition.id.as_str(),
        ))?;
        stmt.exec()?;

        upsert_capability(
            self.conn,
            &CapabilityRow {
                id: capability_id.trim().to_string(),
                name: capability_name.trim().to_string(),
                description: capability_description.trim().to_string(),
                workflow_id: definition.id,
                workflow_version: definition.version,
                input_schema_json: "{}".to_string(),
                output_schema_json: "{}".to_string(),
                enabled: true,
            },
        )
    }
}

fn status_to_str(status: &WorkflowStatus) -> &'static str {
    match status {
        WorkflowStatus::Draft => "draft",
        WorkflowStatus::Published => "published",
        WorkflowStatus::Archived => "archived",
    }
}

fn status_from_str(raw: &str) -> WorkflowStatus {
    match raw {
        "published" => WorkflowStatus::Published,
        "archived" => WorkflowStatus::Archived,
        _ => WorkflowStatus::Draft,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflows::definition::{WorkflowEdge, WorkflowNode, WorkflowNodeKind};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_conn() -> Connection {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "one-workflow-store-test-{}-{}.db",
            std::process::id(),
            nanos
        ));
        Connection::open_file(path.to_str().unwrap())
    }

    #[test]
    fn creates_and_loads_draft_workflow() {
        let conn = test_conn();
        let store = WorkflowStore::new(&conn).unwrap();

        let mut definition = store
            .create_draft("workflow.research", "Research Workflow")
            .unwrap();
        definition.description = "Research then summarize".to_string();
        definition.nodes.push(WorkflowNode {
            id: "main".to_string(),
            name: "MainAgent".to_string(),
            kind: WorkflowNodeKind::Agent {
                agent_id: "mainagent".to_string(),
            },
            config: serde_json::json!({"goal": "research"}),
        });
        definition.nodes.push(WorkflowNode {
            id: "output".to_string(),
            name: "Output".to_string(),
            kind: WorkflowNodeKind::Output,
            config: serde_json::Value::Null,
        });
        definition.edges.push(WorkflowEdge {
            id: "main_to_output".to_string(),
            from_node_id: "main".to_string(),
            to_node_id: "output".to_string(),
            condition: String::new(),
        });

        store.save_draft(&definition).unwrap();

        let loaded = store.load("workflow.research").unwrap().unwrap();
        assert_eq!(loaded, definition);

        let drafts = store.list_drafts().unwrap();
        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0].id, "workflow.research");
        assert_eq!(drafts[0].name, "Research Workflow");
        assert_eq!(drafts[0].description, "Research then summarize");
        assert_eq!(drafts[0].status, WorkflowStatus::Draft);
    }

    #[test]
    fn rejects_non_draft_save() {
        let conn = test_conn();
        let store = WorkflowStore::new(&conn).unwrap();
        let mut definition = WorkflowDefinition::new_draft("workflow.done", "Done");
        definition.status = WorkflowStatus::Published;

        let err = store.save_draft(&definition).unwrap_err();
        assert!(err.to_string().contains("only draft workflows"));
    }

    #[test]
    fn publishes_workflow_as_enabled_capability() {
        let conn = test_conn();
        let store = WorkflowStore::new(&conn).unwrap();
        store
            .create_draft("workflow.research", "Research Workflow")
            .unwrap();

        store
            .publish_as_capability(
                "workflow.research",
                "research_brief",
                "Research Brief",
                "Research and summarize sources",
            )
            .unwrap();

        let published = store.load("workflow.research").unwrap().unwrap();
        assert_eq!(published.status, WorkflowStatus::Published);

        let capabilities = crate::task_db::load_enabled_capabilities(&conn).unwrap();
        assert_eq!(capabilities.len(), 1);
        assert_eq!(capabilities[0].id, "research_brief");
        assert_eq!(capabilities[0].workflow_id, "workflow.research");
        assert_eq!(capabilities[0].workflow_version, 1);
    }

    #[test]
    fn imports_published_workflow_definition_and_version() {
        let conn = test_conn();
        let store = WorkflowStore::new(&conn).unwrap();
        let mut definition = WorkflowDefinition::new_draft("workflow.imported", "Imported");
        definition.status = WorkflowStatus::Published;
        definition.version = 3;
        definition.nodes.push(WorkflowNode {
            id: "output".to_string(),
            name: "Output".to_string(),
            kind: WorkflowNodeKind::Output,
            config: serde_json::json!({ "value": { "ok": true } }),
        });

        store.import_definition(&definition).unwrap();

        let loaded = store.load("workflow.imported").unwrap().unwrap();
        assert_eq!(loaded, definition);
        let loaded_version = store.load_version("workflow.imported", 3).unwrap().unwrap();
        assert_eq!(loaded_version, definition);
        let versions = store.list_versions("workflow.imported").unwrap();
        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0].workflow_id, "workflow.imported");
        assert_eq!(versions[0].version, 3);

        let mut stmt = Statement::prepare(
            &conn,
            "SELECT definition_json FROM workflow_versions WHERE workflow_id = ? AND version = ?",
        )
        .unwrap();
        stmt.with_bindings(&("workflow.imported", 3_i64)).unwrap();
        let rows: Vec<String> = stmt
            .map(|s| s.column_text(0).map(|value| value.to_string()))
            .unwrap();
        assert_eq!(rows.len(), 1);
        let imported: WorkflowDefinition = serde_json::from_str(&rows[0]).unwrap();
        assert_eq!(imported, definition);
    }
}
