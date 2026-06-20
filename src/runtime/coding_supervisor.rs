use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::memory::types::ChatMessage;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkspaceSnapshot {
    files: BTreeMap<String, FileSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileSnapshot {
    len: u64,
    modified_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkspaceDelta {
    pub(crate) added: Vec<String>,
    pub(crate) modified: Vec<String>,
    pub(crate) deleted: Vec<String>,
}

impl WorkspaceDelta {
    pub(crate) fn describe(&self) -> String {
        let mut lines = Vec::new();
        if !self.added.is_empty() {
            lines.push(format!("added: {}", self.added.join(", ")));
        }
        if !self.modified.is_empty() {
            lines.push(format!("modified: {}", self.modified.join(", ")));
        }
        if !self.deleted.is_empty() {
            lines.push(format!("deleted: {}", self.deleted.join(", ")));
        }
        if lines.is_empty() {
            "no workspace file changes detected".to_string()
        } else {
            lines.join("\n")
        }
    }
}

pub(crate) fn capture_workspace_snapshot(root: &Path) -> WorkspaceSnapshot {
    let mut files = BTreeMap::new();
    collect_files(root, root, &mut files);
    WorkspaceSnapshot { files }
}

pub(crate) fn diff_workspace_snapshot(
    baseline: &WorkspaceSnapshot,
    current: &WorkspaceSnapshot,
) -> WorkspaceDelta {
    let before = baseline.files.keys().cloned().collect::<BTreeSet<_>>();
    let after = current.files.keys().cloned().collect::<BTreeSet<_>>();
    let added = after.difference(&before).cloned().collect::<Vec<_>>();
    let deleted = before.difference(&after).cloned().collect::<Vec<_>>();
    let modified = before
        .intersection(&after)
        .filter(|path| baseline.files.get(*path) != current.files.get(*path))
        .cloned()
        .collect::<Vec<_>>();
    WorkspaceDelta {
        added,
        modified,
        deleted,
    }
}

fn collect_files(root: &Path, dir: &Path, files: &mut BTreeMap<String, FileSnapshot>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if should_ignore_path(&name) {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if metadata.is_dir() {
            collect_files(root, &path, files);
            continue;
        }
        if !metadata.is_file() {
            continue;
        }
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        let modified_secs = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_secs())
            .unwrap_or_default();
        files.insert(
            relative.to_string_lossy().to_string(),
            FileSnapshot {
                len: metadata.len(),
                modified_secs,
            },
        );
    }
}

fn should_ignore_path(name: &str) -> bool {
    matches!(
        name,
        ".git" | "target" | "node_modules" | ".next" | ".nuxt" | "dist" | "build" | ".DS_Store"
    )
}

#[derive(Debug, Clone)]
pub(crate) struct CodingSupervisionRequest {
    pub(crate) session_id: String,
    pub(crate) agent_label: String,
    pub(crate) cwd: PathBuf,
    pub(crate) submitted_task: String,
    pub(crate) terminal_transcript: Vec<String>,
    pub(crate) workspace_delta: WorkspaceDelta,
    pub(crate) fingerprint: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CodingSupervisorState {
    Running,
    WaitingUser,
    Completed,
    Failed,
    Unclear,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct CodingSupervisorDecision {
    pub(crate) state: CodingSupervisorState,
    #[serde(default)]
    pub(crate) confidence: u8,
    #[serde(default)]
    pub(crate) user_message: String,
    #[serde(default)]
    pub(crate) options: Vec<String>,
    #[serde(default)]
    pub(crate) artifacts: Vec<String>,
    #[serde(default)]
    pub(crate) risks: Vec<String>,
}

pub(crate) async fn supervise_coding_session(
    base_url: &str,
    api_key: &str,
    model: &str,
    request: &CodingSupervisionRequest,
) -> Result<CodingSupervisorDecision, String> {
    let transcript = request.terminal_transcript.join("\n");
    let prompt = format!(
        "你是 CodingSessionSupervisor。你不是终端执行者，而是 MainAgent 的监督层。\n\
         请基于任务、终端 transcript、workspace cwd 和文件变更，判断 coding CLI 当前语义状态。\n\
         严格只返回 JSON，不要 Markdown，不要解释。\n\n\
         JSON schema:\n\
         {{\"state\":\"running|waiting_user|completed|failed|unclear\",\"confidence\":0-100,\"user_message\":\"给主聊天区用户看的中文简洁消息\",\"options\":[\"用户可选项\"],\"artifacts\":[\"产物路径或文件\"],\"risks\":[\"风险或异常\"]}}\n\n\
         判断要求：\n\
         - 不要依赖固定关键词；要综合 transcript、任务目标和文件变更。\n\
         - 如果 runtime 明显还在执行，state=running，user_message 简短，不要复述日志。\n\
         - 如果需要用户确认、登录、授权或选择，state=waiting_user，并把选项转成用户能理解的中文。\n\
         - 如果任务已经完成，state=completed，必须总结完成内容和产物。\n\
         - 如果写入路径不在 cwd 下，必须在 risks 中指出。\n\
         - workspace_delta 是事实验证信号；没有变更时不要轻易判 completed。\n\n\
         agent: {}\n\
         cwd: {}\n\
         submitted_task:\n{}\n\n\
         workspace_delta:\n{}\n\n\
         terminal_transcript:\n{}",
        request.agent_label,
        request.cwd.to_string_lossy(),
        request.submitted_task,
        request.workspace_delta.describe(),
        transcript
    );
    let messages = vec![ChatMessage::new("user", &prompt)];
    let response = crate::services::api::call_chat_api_once(base_url, api_key, model, &messages)
        .await
        .map_err(|error| format!("supervisor model call failed: {}", error))?;
    parse_supervisor_decision(&response)
}

pub(crate) fn parse_supervisor_decision(raw: &str) -> Result<CodingSupervisorDecision, String> {
    let trimmed = raw.trim();
    let json_text = if let Some(start) = trimmed.find('{') {
        if let Some(end) = trimmed.rfind('}') {
            &trimmed[start..=end]
        } else {
            trimmed
        }
    } else {
        trimmed
    };
    let mut decision: CodingSupervisorDecision =
        serde_json::from_str(json_text).map_err(|error| error.to_string())?;
    decision.confidence = decision.confidence.min(100);
    decision.user_message = decision.user_message.trim().to_string();
    Ok(decision)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_json_inside_text() {
        let decision = parse_supervisor_decision(
            "```json\n{\"state\":\"completed\",\"confidence\":91,\"user_message\":\"完成\",\"artifacts\":[\"a.html\"]}\n```",
        )
        .unwrap();
        assert_eq!(decision.state, CodingSupervisorState::Completed);
        assert_eq!(decision.confidence, 91);
        assert_eq!(decision.artifacts, vec!["a.html"]);
    }

    #[test]
    fn workspace_delta_detects_added_modified_deleted() {
        let before = WorkspaceSnapshot {
            files: BTreeMap::from([
                (
                    "a.txt".to_string(),
                    FileSnapshot {
                        len: 1,
                        modified_secs: 1,
                    },
                ),
                (
                    "b.txt".to_string(),
                    FileSnapshot {
                        len: 2,
                        modified_secs: 1,
                    },
                ),
            ]),
        };
        let after = WorkspaceSnapshot {
            files: BTreeMap::from([
                (
                    "a.txt".to_string(),
                    FileSnapshot {
                        len: 3,
                        modified_secs: 1,
                    },
                ),
                (
                    "c.txt".to_string(),
                    FileSnapshot {
                        len: 1,
                        modified_secs: 1,
                    },
                ),
            ]),
        };
        let delta = diff_workspace_snapshot(&before, &after);
        assert_eq!(delta.added, vec!["c.txt"]);
        assert_eq!(delta.modified, vec!["a.txt"]);
        assert_eq!(delta.deleted, vec!["b.txt"]);
    }
}
