#![allow(dead_code)]

use crate::memory::search::load_l3_chunks_internal;
use crate::memory::search::tfidf_search;
use crate::memory::storage::{load_task_snapshot, save_task_snapshot};
use crate::memory::types::{ChatMessage, MemoryChunk, MemorySnapshot};
use crate::services::api::call_chat_api_sync;
use serde::Deserialize;

pub fn build_memory_context(workspace_name: &str, task_id: usize, query: &str) -> String {
    let mut parts: Vec<String> = vec![];

    // ── 注入本 task 的 snapshot 关键信息 ──────────────────────────
    if let Some(snap) = load_task_snapshot(workspace_name, task_id) {
        if !snap.summary.is_empty() {
            parts.push(format!("## Current Task Summary\n{}", snap.summary));
        }
        if !snap.key_facts.is_empty() {
            parts.push(format!(
                "## Current Task Key Facts\n{}",
                snap.key_facts
                    .iter()
                    .map(|f| format!("- {}", f))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }
        if !snap.open_loops.is_empty() {
            parts.push(format!(
                "## Open Questions\n{}",
                snap.open_loops
                    .iter()
                    .map(|f| format!("- {}", f))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }
        if !snap.preferences.is_empty() {
            parts.push(format!(
                "## User Preferences\n{}",
                snap.preferences
                    .iter()
                    .map(|f| format!("- {}", f))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }
    }

    // ── L3 跨任务语义检索 ────────────────────────────────────────
    if !query.is_empty() {
        let all = load_l3_chunks_internal(workspace_name);
        let others: Vec<MemoryChunk> = all.into_iter().filter(|c| c.task_id != task_id).collect();
        if !others.is_empty() {
            let hits = tfidf_search(query, &others, 5);
            if !hits.is_empty() {
                let mut section = vec!["## Related Context (from other tasks)".to_string()];
                for idx in &hits {
                    let hit = &others[*idx];
                    // 展示更多上下文信息：task 标题 + 角色 + 完整内容（截断至400字符）
                    let max_len = 400;
                    let preview = if hit.content.len() > max_len {
                        let mut end = max_len;
                        while !hit.content.is_char_boundary(end) {
                            end -= 1;
                        }
                        format!("{}...", &hit.content[..end])
                    } else {
                        hit.content.clone()
                    };
                    section.push(format!(
                        "[Task: {}] {}: {}",
                        hit.task_title,
                        if hit.role == "assistant" {
                            "Assistant"
                        } else {
                            "User"
                        },
                        preview
                    ));
                }

                // 补充：注入这些相关 task 的 snapshot key facts（如果有）
                let mut added_tasks = std::collections::HashSet::new();
                for idx in hits {
                    let hit = &others[idx];
                    if added_tasks.insert(hit.task_id) {
                        if let Some(related_snap) = load_task_snapshot(workspace_name, hit.task_id) {
                            if !related_snap.key_facts.is_empty() {
                                section.push(format!(
                                    "[Snapshot Facts from Task {}: {}]",
                                    hit.task_id, hit.task_title
                                ));
                                for f in &related_snap.key_facts {
                                    section.push(format!("  - {}", f));
                                }
                            }
                        }
                    }
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
        .map(|m| {
            format!(
                "{}: {}",
                if m.role == "user" {
                    "User"
                } else {
                    "Assistant"
                },
                m.content
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "You are a memory distiller. Analyze the conversation and return ONLY a valid JSON \
         object (no markdown fences) with these fields:\n\
         - \"summary\": string, 1-3 sentences overview\n\
         - \"key_facts\": array of strings, **global, permanent facts** about the user (their name, language, \
           profession, location, skills, important decisions). Do NOT include temporary preferences \
           or conversation-specific details here. Max 8 items.\n\
         - \"open_loops\": array of strings, unresolved questions or pending actions (max 5)\n\
         - \"preferences\": array of strings, **temporary, task-specific preferences** (e.g. output format for \
           this task, response style for this conversation). These are NOT permanent user traits. Max 5.\n\n\
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
    let req = vec![ChatMessage::new("user", &build_snapshot_prompt(messages))];
    match call_chat_api_sync(base_url, api_key, model, &req) {
        Ok(resp) => match parse_snapshot_from_llm(task_id, task_title, &resp) {
            Some(snap) => {
                eprintln!("[Memory L2] snapshot OK for task {}", task_id);
                if let Err(e) = save_task_snapshot(workspace_name, &snap) {
                    eprintln!("[Memory L2] save snapshot failed: {}", e);
                }

                // ── 自动提取 key_facts + preferences 写入 profile ──────────
                // key_facts → global + workspace（全局持久事实）
                // preferences → workspace 仅当前 workspace 可见（临时偏好）
                for fact in &snap.key_facts {
                    if !fact.trim().is_empty() {
                        let _ = crate::memory::profile::save_global_fact(fact, Some(task_id));
                        let _ = crate::memory::profile::save_fact(workspace_name, fact, Some(task_id));
                    }
                }
                for pref in &snap.preferences {
                    if !pref.trim().is_empty() {
                        // preferences 只写入 workspace，不写入 global
                        let _ = crate::memory::profile::save_fact(workspace_name, pref, Some(task_id));
                    }
                }
            }
            None => eprintln!("[Memory L2] parse failed: {}", resp),
        },
        Err(e) => eprintln!("[Memory L2] API error: {}", e),
    }
}
