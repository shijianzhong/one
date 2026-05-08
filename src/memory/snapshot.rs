use crate::memory::types::{ChatMessage, MemoryChunk, MemorySnapshot};
use crate::memory::storage::{load_task_snapshot, save_task_snapshot};
use crate::memory::search::load_l3_chunks_internal;
use crate::memory::search::tfidf_search;
use crate::services::api::call_chat_api_sync;
use serde::Deserialize;

pub fn build_memory_context(workspace_name: &str, task_id: usize, query: &str) -> String {
    let mut parts: Vec<String> = vec![];

    if let Some(snap) = load_task_snapshot(workspace_name, task_id) {
        if !snap.summary.is_empty() {
            parts.push(format!("## Task Summary\n{}", snap.summary));
        }
        if !snap.key_facts.is_empty() {
            parts.push(format!(
                "## Key Facts\n{}",
                snap.key_facts.iter().map(|f| format!("- {}", f)).collect::<Vec<_>>().join("\n")
            ));
        }
        if !snap.open_loops.is_empty() {
            parts.push(format!(
                "## Open Questions\n{}",
                snap.open_loops.iter().map(|f| format!("- {}", f)).collect::<Vec<_>>().join("\n")
            ));
        }
        if !snap.preferences.is_empty() {
            parts.push(format!(
                "## User Preferences\n{}",
                snap.preferences.iter().map(|f| format!("- {}", f)).collect::<Vec<_>>().join("\n")
            ));
        }
    }

    if !query.is_empty() {
        let all = load_l3_chunks_internal(workspace_name);
        let others: Vec<MemoryChunk> = all.into_iter().filter(|c| c.task_id != task_id).collect();
        if !others.is_empty() {
            let hits = tfidf_search(query, &others, 3);
            if !hits.is_empty() {
                let mut section = vec!["## Related Context (from other tasks)".to_string()];
                for idx in hits {
                    let hit = &others[idx];
                    let preview = &hit.content[..hit.content.len().min(200)];
                    section.push(format!(
                        "[Task: {}] {}: {}",
                        hit.task_title,
                        if hit.role == "assistant" { "Assistant" } else { "User" },
                        preview
                    ));
                }
                parts.push(section.join("\n"));
            }
        }
    }

    if parts.is_empty() {
        return String::new();
    }
    format!("<memory>\n{}\n</memory>", parts.join("\n\n"))
}

fn build_snapshot_prompt(messages: &[ChatMessage]) -> String {
    let history: String = messages
        .iter()
        .map(|m| format!(
            "{}: {}",
            if m.role == "user" { "User" } else { "Assistant" },
            m.content
        ))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "You are a memory distiller. Analyze the conversation and return ONLY a valid JSON \
         object (no markdown fences) with these fields:\n\
         - \"summary\": string, 1-3 sentences overview\n\
         - \"key_facts\": array of strings, concrete facts/decisions (max 8)\n\
         - \"open_loops\": array of strings, unresolved questions or pending actions (max 5)\n\
         - \"preferences\": array of strings, user preferences or style hints (max 5)\n\n\
         Conversation:\n{}\n\nRespond with ONLY the JSON object.",
        history
    )
}

fn parse_snapshot_from_llm(
    task_id: usize,
    task_title: &str,
    json_str: &str,
) -> Option<MemorySnapshot> {
    let cleaned = match (json_str.find('{'), json_str.rfind('}')) {
        (Some(s), Some(e)) if e >= s => &json_str[s..=e],
        _ => json_str,
    };
    #[derive(Deserialize)]
    struct Resp {
        summary: Option<String>,
        key_facts: Option<Vec<String>>,
        open_loops: Option<Vec<String>>,
        preferences: Option<Vec<String>>,
    }
    let resp: Resp = serde_json::from_str(cleaned).ok()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default();
    Some(MemorySnapshot {
        task_id,
        task_title: task_title.to_string(),
        summary: resp.summary.unwrap_or_default(),
        key_facts: resp.key_facts.unwrap_or_default(),
        open_loops: resp.open_loops.unwrap_or_default(),
        preferences: resp.preferences.unwrap_or_default(),
        last_updated: now,
    })
}

pub fn generate_snapshot_sync(
    base_url: &str,
    api_key: &str,
    model: &str,
    messages: &[ChatMessage],
    task_id: usize,
    task_title: &str,
    workspace_name: &str,
) {
    if messages.len() < 2 {
        return;
    }
    let req = vec![ChatMessage {
        role: "user".to_string(),
        content: build_snapshot_prompt(messages),
    }];
    match call_chat_api_sync(base_url, api_key, model, &req) {
        Ok(resp) => match parse_snapshot_from_llm(task_id, task_title, &resp) {
            Some(snap) => {
                eprintln!("[Memory L2] snapshot OK for task {}", task_id);
                if let Err(e) = save_task_snapshot(workspace_name, &snap) {
                    eprintln!("[Memory L2] save snapshot failed: {}", e);
                }
            }
            None => eprintln!("[Memory L2] parse failed: {}", resp),
        },
        Err(e) => eprintln!("[Memory L2] API error: {}", e),
    }
}
