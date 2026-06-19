use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityManifest {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub workflow_id: String,
    pub workflow_version: i64,
    #[serde(default)]
    pub input_schema: Value,
    #[serde(default)]
    pub output_schema: Value,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityExportPackage {
    pub schema_version: u32,
    pub capability: CapabilityManifest,
    pub workflow: crate::workflows::WorkflowDefinition,
}

fn default_enabled() -> bool {
    true
}

pub fn capability_manifests() -> Vec<CapabilityManifest> {
    let mut manifests = db_capability_manifests();
    let known_ids: std::collections::HashSet<String> = manifests
        .iter()
        .map(|manifest| manifest.id.clone())
        .collect();
    manifests.extend(
        capability_dirs()
            .into_iter()
            .flat_map(|dir| load_manifests_from_dir(&dir))
            .filter(|manifest| manifest.enabled)
            .filter(|manifest| !known_ids.contains(&manifest.id)),
    );
    manifests.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    manifests
}

pub fn has_published_capabilities() -> bool {
    capability_manifests().into_iter().next().is_some()
}

pub fn format_capabilities_for_prompt(capabilities: &[CapabilityManifest]) -> String {
    if capabilities.is_empty() {
        return String::new();
    }

    let capability_info: Vec<String> = capabilities
        .iter()
        .map(|capability| {
            let input_schema = serde_json::to_string(&capability.input_schema)
                .unwrap_or_else(|_| "{}".to_string());
            format!(
                "- **{}** (`{}`), workflow `{}` v{}: {}\n  input_schema: `{}`",
                capability.name,
                capability.id,
                capability.workflow_id,
                capability.workflow_version,
                capability.description,
                input_schema
            )
        })
        .collect();

    format!(
        "### 已发布能力\n\
         以下能力由工作流发布而来。只有当用户请求与某个能力明确匹配时才调用 `run_capability`；\
         不要编造 capability_id；`capability_id` 必须从列表中精确选择；`input` 必须按对应 input_schema 组织。\n{}",
        capability_info.join("\n")
    )
}

pub fn export_capability_package(capability_id: &str) -> Result<CapabilityExportPackage> {
    let Some(manifest) = capability_manifests()
        .into_iter()
        .find(|manifest| manifest.id == capability_id)
    else {
        anyhow::bail!("published capability '{}' not found", capability_id);
    };

    let definition = {
        let db = crate::task_db::Database::new()?;
        let store = crate::workflows::WorkflowStore::new(&db.conn)?;
        store.load_active_or_version(&manifest.workflow_id, manifest.workflow_version)?
    };
    let Some(definition) = definition else {
        anyhow::bail!(
            "workflow definition '{}' not found for capability '{}'",
            manifest.workflow_id,
            manifest.id
        );
    };

    Ok(CapabilityExportPackage {
        schema_version: 1,
        capability: manifest,
        workflow: definition,
    })
}

pub fn export_capability_package_json(capability_id: &str) -> Result<String> {
    let package = export_capability_package(capability_id)?;
    serde_json::to_string_pretty(&package)
        .with_context(|| format!("failed to serialize capability package {}", capability_id))
}

pub fn import_capability_package_json(raw_json: &str) -> Result<String> {
    let package: CapabilityExportPackage = serde_json::from_str(raw_json)
        .with_context(|| "failed to parse capability package JSON")?;
    if package.schema_version != 1 {
        anyhow::bail!(
            "unsupported capability package schema_version {}",
            package.schema_version
        );
    }
    if package.capability.id.trim().is_empty() {
        anyhow::bail!("capability id cannot be empty");
    }
    if package.workflow.id.trim().is_empty() {
        anyhow::bail!("workflow id cannot be empty");
    }
    if package.capability.workflow_id != package.workflow.id {
        anyhow::bail!(
            "capability workflow_id '{}' does not match workflow id '{}'",
            package.capability.workflow_id,
            package.workflow.id
        );
    }
    if package.capability.workflow_version != package.workflow.version {
        anyhow::bail!(
            "capability workflow_version {} does not match workflow version {}",
            package.capability.workflow_version,
            package.workflow.version
        );
    }

    let db = crate::task_db::Database::new()?;
    let store = crate::workflows::WorkflowStore::new(&db.conn)?;
    store.import_definition(&package.workflow)?;
    crate::task_db::upsert_capability(
        &db.conn,
        &crate::task_db::CapabilityRow {
            id: package.capability.id.trim().to_string(),
            name: package.capability.name.trim().to_string(),
            description: package.capability.description.trim().to_string(),
            workflow_id: package.capability.workflow_id.trim().to_string(),
            workflow_version: package.capability.workflow_version,
            input_schema_json: serde_json::to_string(&package.capability.input_schema)?,
            output_schema_json: serde_json::to_string(&package.capability.output_schema)?,
            enabled: package.capability.enabled,
        },
    )?;

    Ok(package.capability.id)
}

pub async fn run_capability(capability_id: &str, input: Value) -> Result<Value> {
    let Some(manifest) = capability_manifests()
        .into_iter()
        .find(|manifest| manifest.id == capability_id)
    else {
        anyhow::bail!("published capability '{}' not found", capability_id);
    };

    let definition = {
        let db = crate::task_db::Database::new()?;
        let store = crate::workflows::WorkflowStore::new(&db.conn)?;
        store.load_active_or_version(&manifest.workflow_id, manifest.workflow_version)?
    };

    let Some(definition) = definition else {
        return Ok(serde_json::json!({
            "status": "not_ready",
            "capability_id": manifest.id,
            "workflow_id": manifest.workflow_id,
            "workflow_version": manifest.workflow_version,
            "input": input,
            "message": "Capability manifest is registered, but the workflow definition was not found in the local database."
        }));
    };

    let run_id = {
        let db = crate::task_db::Database::new()?;
        let run_id =
            crate::task_db::insert_workflow_run(&db.conn, &definition.id, definition.version)?;
        crate::task_db::insert_workflow_run_event(
            &db.conn,
            run_id,
            "run_started",
            &serde_json::to_string(&serde_json::json!({
                "capability_id": manifest.id.clone(),
                "input": input.clone(),
            }))?,
        )?;
        run_id
    };

    let runtime = crate::workflows::WorkflowRuntime::new();
    let result = match runtime.run_definition(&definition, input).await {
        Ok(result) => result,
        Err(err) => {
            let db = crate::task_db::Database::new()?;
            let _ = crate::task_db::insert_workflow_run_event(
                &db.conn,
                run_id,
                "run_failed",
                &serde_json::to_string(&serde_json::json!({
                    "capability_id": manifest.id.clone(),
                    "error": err.to_string(),
                }))?,
            );
            let _ = crate::task_db::finish_workflow_run(
                &db.conn,
                run_id,
                "failed",
                Some(&err.to_string()),
            );
            return Err(err);
        }
    };

    if is_awaiting_human_approval(&result) {
        let db = crate::task_db::Database::new()?;
        crate::task_db::insert_workflow_run_event(
            &db.conn,
            run_id,
            "human_approval_requested",
            &serde_json::to_string(&serde_json::json!({
                "capability_id": manifest.id.clone(),
                "result": result.clone(),
            }))?,
        )?;
        return Ok(serde_json::json!({
            "status": "awaiting_human_approval",
            "capability_id": manifest.id,
            "workflow_id": manifest.workflow_id,
            "workflow_version": manifest.workflow_version,
            "run_id": run_id,
            "workflow_run": result,
        }));
    }

    {
        let db = crate::task_db::Database::new()?;
        crate::task_db::insert_workflow_run_event(
            &db.conn,
            run_id,
            "run_finished",
            &serde_json::to_string(&serde_json::json!({
                "capability_id": manifest.id.clone(),
                "result": result.clone(),
            }))?,
        )?;
        crate::task_db::finish_workflow_run(&db.conn, run_id, "succeeded", None)?;
    }

    Ok(serde_json::json!({
        "status": result.get("status").cloned().unwrap_or_else(|| serde_json::json!("succeeded")),
        "capability_id": manifest.id,
        "workflow_id": manifest.workflow_id,
        "workflow_version": manifest.workflow_version,
        "run_id": run_id,
        "workflow_run": result,
    }))
}

pub async fn resume_capability_run(run_id: usize, approved: bool) -> Result<Value> {
    resume_capability_run_with_note(run_id, approved, None).await
}

pub async fn resume_capability_run_with_note(
    run_id: usize,
    approved: bool,
    note: Option<String>,
) -> Result<Value> {
    let (run, paused_node_id, capability_id) = {
        let db = crate::task_db::Database::new()?;
        let Some(run) = crate::task_db::load_workflow_run(&db.conn, run_id)? else {
            anyhow::bail!("workflow run {} not found", run_id);
        };
        if run.status != "running" {
            anyhow::bail!(
                "workflow run {} cannot be resumed because status is '{}'",
                run_id,
                run.status
            );
        }
        let events = crate::task_db::load_workflow_run_events(&db.conn, run_id)?;
        let Some(paused_event) = events
            .iter()
            .rev()
            .find(|event| event.kind == "human_approval_requested")
        else {
            anyhow::bail!(
                "workflow run {} has no pending human approval event",
                run_id
            );
        };
        let payload: Value = serde_json::from_str(&paused_event.payload)?;
        let paused_node_id = payload
            .pointer("/result/result/node_id")
            .and_then(|value| value.as_str())
            .ok_or_else(|| anyhow::anyhow!("workflow run {} approval node_id missing", run_id))?
            .to_string();
        let capability_id = payload
            .get("capability_id")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string();
        (run, paused_node_id, capability_id)
    };

    let definition = {
        let db = crate::task_db::Database::new()?;
        let store = crate::workflows::WorkflowStore::new(&db.conn)?;
        store.load_active_or_version(&run.workflow_id, run.workflow_version)?
    };
    let Some(definition) = definition else {
        anyhow::bail!("workflow definition '{}' not found", run.workflow_id);
    };

    let note = note.unwrap_or_default();
    let resume_input = serde_json::json!({
        "approved": approved,
        "note": note,
    });
    {
        let db = crate::task_db::Database::new()?;
        crate::task_db::insert_workflow_run_event(
            &db.conn,
            run_id,
            "human_approval_resolved",
            &serde_json::to_string(&serde_json::json!({
                "capability_id": capability_id,
                "node_id": paused_node_id,
                "approved": approved,
                "note": note,
            }))?,
        )?;
    }

    let runtime = crate::workflows::WorkflowRuntime::new();
    let result = match runtime
        .resume_definition(&definition, &paused_node_id, resume_input)
        .await
    {
        Ok(result) => result,
        Err(err) => {
            let db = crate::task_db::Database::new()?;
            let _ = crate::task_db::insert_workflow_run_event(
                &db.conn,
                run_id,
                "run_failed",
                &serde_json::to_string(&serde_json::json!({
                    "capability_id": capability_id,
                    "error": err.to_string(),
                }))?,
            );
            let _ = crate::task_db::finish_workflow_run(
                &db.conn,
                run_id,
                "failed",
                Some(&err.to_string()),
            );
            return Err(err);
        }
    };

    {
        let db = crate::task_db::Database::new()?;
        crate::task_db::insert_workflow_run_event(
            &db.conn,
            run_id,
            "run_finished",
            &serde_json::to_string(&serde_json::json!({
                "capability_id": capability_id,
                "result": result.clone(),
            }))?,
        )?;
        crate::task_db::finish_workflow_run(&db.conn, run_id, "succeeded", None)?;
    }

    Ok(serde_json::json!({
        "status": result.get("status").cloned().unwrap_or_else(|| serde_json::json!("succeeded")),
        "capability_id": capability_id,
        "workflow_id": run.workflow_id,
        "workflow_version": run.workflow_version,
        "run_id": run_id,
        "workflow_run": result,
    }))
}

fn is_awaiting_human_approval(result: &Value) -> bool {
    result
        .get("result")
        .and_then(|value| value.get("status"))
        .and_then(|status| status.as_str())
        .map(|status| status == "awaiting_human_approval")
        .unwrap_or(false)
}

fn capability_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(data_dir) = dirs::data_dir() {
        dirs.push(data_dir.join("one").join("capabilities"));
    }
    dirs.push(
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".one")
            .join("capabilities"),
    );
    dirs
}

fn db_capability_manifests() -> Vec<CapabilityManifest> {
    let Ok(db) = crate::task_db::Database::new() else {
        return Vec::new();
    };
    let Ok(rows) = crate::task_db::load_enabled_capabilities(&db.conn) else {
        return Vec::new();
    };

    rows.into_iter()
        .filter_map(|row| {
            let input_schema = serde_json::from_str(&row.input_schema_json).unwrap_or(Value::Null);
            let output_schema =
                serde_json::from_str(&row.output_schema_json).unwrap_or(Value::Null);
            if row.id.trim().is_empty() || row.workflow_id.trim().is_empty() {
                return None;
            }
            Some(CapabilityManifest {
                id: row.id,
                name: row.name,
                description: row.description,
                workflow_id: row.workflow_id,
                workflow_version: row.workflow_version,
                input_schema,
                output_schema,
                enabled: row.enabled,
            })
        })
        .collect()
}

fn load_manifests_from_dir(dir: &Path) -> Vec<CapabilityManifest> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };

    let mut manifests = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        match load_manifest(&path) {
            Ok(manifest) => manifests.push(manifest),
            Err(err) => {
                log::warn!(
                    "[Capabilities] Failed to load manifest {}: {}",
                    path.display(),
                    err
                );
            }
        }
    }
    manifests.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    manifests
}

fn load_manifest(path: &Path) -> Result<CapabilityManifest> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read capability manifest {}", path.display()))?;
    let manifest: CapabilityManifest = serde_json::from_str(&content)
        .with_context(|| format!("failed to parse capability manifest {}", path.display()))?;
    if manifest.id.trim().is_empty() {
        anyhow::bail!("capability id cannot be empty");
    }
    if manifest.workflow_id.trim().is_empty() {
        anyhow::bail!("workflow_id cannot be empty");
    }
    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_manifest() {
        let manifest: CapabilityManifest = serde_json::from_str(
            r#"{
                "id": "research_brief",
                "name": "Research Brief",
                "workflow_id": "workflow.research",
                "workflow_version": 1
            }"#,
        )
        .unwrap();

        assert_eq!(manifest.id, "research_brief");
        assert!(manifest.enabled);
        assert!(manifest.description.is_empty());
    }

    #[test]
    fn formats_capability_prompt_with_stable_selection_rules() {
        let prompt = format_capabilities_for_prompt(&[CapabilityManifest {
            id: "research_brief".to_string(),
            name: "Research Brief".to_string(),
            description: "Research and summarize sources".to_string(),
            workflow_id: "workflow.research".to_string(),
            workflow_version: 2,
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "topic": { "type": "string" }
                },
                "required": ["topic"]
            }),
            output_schema: Value::Null,
            enabled: true,
        }]);

        assert!(prompt.contains("research_brief"));
        assert!(prompt.contains("workflow.research"));
        assert!(prompt.contains("input_schema"));
        assert!(prompt.contains("不要编造 capability_id"));
        assert!(prompt.contains("\"topic\""));
    }
}
