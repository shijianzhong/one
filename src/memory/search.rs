use crate::memory::types::{ChatMessage, MemoryChunk};
use crate::memory::storage::get_workspace_memory_dir;
use std::collections::HashMap;
use std::fs;

pub fn load_l3_chunks_internal(workspace_name: &str) -> Vec<MemoryChunk> {
    let path = get_workspace_memory_dir(workspace_name).join("l3_chunks.json");
    if path.exists() {
        if let Ok(raw) = fs::read_to_string(&path) {
            if let Ok(chunks) = serde_json::from_str::<Vec<MemoryChunk>>(&raw) {
                return chunks;
            }
        }
    }
    vec![]
}

fn save_l3_chunks(workspace_name: &str, chunks: &[MemoryChunk]) -> anyhow::Result<()> {
    let dir = get_workspace_memory_dir(workspace_name);
    fs::create_dir_all(&dir)?;
    let path = dir.join("l3_chunks.json");
    let content = serde_json::to_string_pretty(chunks)?;
    fs::write(&path, content)?;
    Ok(())
}

pub fn load_l3_chunks(workspace_name: &str) -> Vec<MemoryChunk> {
    load_l3_chunks_internal(workspace_name)
}

pub fn upsert_task_chunks(
    workspace_name: &str,
    task_id: usize,
    task_title: &str,
    messages: &[ChatMessage],
) {
    let mut chunks = load_l3_chunks_internal(workspace_name);
    chunks.retain(|c| c.task_id != task_id);
    for (i, msg) in messages.iter().enumerate() {
        chunks.push(MemoryChunk {
            chunk_id: format!("{}/{}/{}", workspace_name, task_id, i),
            workspace: workspace_name.to_string(),
            task_id,
            task_title: task_title.to_string(),
            role: msg.role.clone(),
            content: msg.content.clone(),
            turn_index: i,
        });
    }
    if let Err(e) = save_l3_chunks(workspace_name, &chunks) {
        eprintln!("[Memory L3] save failed: {}", e);
    }
}

pub fn tfidf_search(query: &str, chunks: &[MemoryChunk], top_k: usize) -> Vec<usize> {
    if chunks.is_empty() || query.is_empty() {
        return vec![];
    }
    let tokenize = |text: &str| -> Vec<String> {
        text.to_lowercase()
            .split(|c: char| !c.is_alphanumeric() && c != '_')
            .filter(|s| s.len() > 1)
            .map(|s| s.to_string())
            .collect()
    };
    let query_tokens = tokenize(query);
    let n = chunks.len() as f64;
    let mut df: HashMap<String, usize> = HashMap::new();
    for chunk in chunks.iter() {
        let toks: std::collections::HashSet<String> = tokenize(&chunk.content).into_iter().collect();
        for t in toks {
            *df.entry(t).or_insert(0) += 1;
        }
    }
    let mut scored: Vec<(f64, usize)> = chunks
        .iter()
        .enumerate()
        .map(|(idx, chunk)| {
            let toks = tokenize(&chunk.content);
            let total = toks.len() as f64;
            if total == 0.0 {
                return (0.0, idx);
            }
            let mut tf: HashMap<String, f64> = HashMap::new();
            for t in &toks {
                *tf.entry(t.clone()).or_insert(0.0) += 1.0 / total;
            }
            let score: f64 = query_tokens
                .iter()
                .map(|qt| {
                    let tf_val = tf.get(qt).copied().unwrap_or(0.0);
                    let idf_val = (n / (1.0 + *df.get(qt).unwrap_or(&0) as f64)).ln().max(0.0);
                    tf_val * idf_val
                })
                .sum();
            (score, idx)
        })
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    scored
        .into_iter()
        .filter(|(s, _)| *s > 0.0)
        .take(top_k)
        .map(|(_, idx)| idx)
        .collect()
}
