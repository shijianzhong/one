#![allow(dead_code)]

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

use anyhow::{anyhow, Context, Result};
use tokio::sync::mpsc::UnboundedSender;

#[derive(Debug, Clone)]
pub enum ClaudeStreamEvent {
    Started {
        command: String,
        workdir: String,
    },
    AssistantText(String),
    Progress {
        label: String,
        detail: String,
    },
    Stderr(String),
    Session {
        session_id: String,
    },
    AskUserQuestion {
        prompt: String,
        options: Vec<String>,
    },
    ModifiedFiles {
        files: Vec<String>,
    },
    Finished {
        result: String,
    },
    Failed {
        error: String,
    },
}

pub struct ClaudeCodeAgent {
    binary_path: PathBuf,
}

impl ClaudeCodeAgent {
    pub fn new() -> Self {
        let binary_path = which::which("claude").unwrap_or_else(|_| PathBuf::from("claude"));
        Self { binary_path }
    }

    pub fn check_installation() -> Option<PathBuf> {
        which::which("claude").ok()
    }

    pub fn get_project_dir(task_id: usize) -> PathBuf {
        PathBuf::from(format!("/tmp/one_task_{}", task_id))
    }

    fn extract_detail(value: &serde_json::Value) -> String {
        let preferred_fields = [
            "message",
            "status",
            "summary",
            "subtype",
            "result",
            "session_id",
            "model",
        ];

        for field in preferred_fields {
            if let Some(field_value) = value.get(field) {
                if let Some(text) = field_value.as_str() {
                    if !text.trim().is_empty() {
                        return text.to_string();
                    }
                } else {
                    return serde_json::to_string(field_value)
                        .unwrap_or_else(|_| "<invalid event>".to_string());
                }
            }
        }

        serde_json::to_string(value).unwrap_or_else(|_| "<invalid event>".to_string())
    }

    fn parse_stream_line(line: &str) -> Vec<ClaudeStreamEvent> {
        let Ok(json) = serde_json::from_str::<serde_json::Value>(line) else {
            if line.trim().is_empty() {
                return vec![];
            }
            return vec![ClaudeStreamEvent::Progress {
                label: "stdout".to_string(),
                detail: line.to_string(),
            }];
        };

        let type_str = json
            .get("type")
            .and_then(|value| value.as_str())
            .unwrap_or("unknown");

        let mut prefix_events = Vec::new();
        if let Some(session_id) = json.get("session_id").and_then(|value| value.as_str()) {
            if !session_id.trim().is_empty() {
                prefix_events.push(ClaudeStreamEvent::Session {
                    session_id: session_id.to_string(),
                });
            }
        }

        let mut events = match type_str {
            "assistant" => {
                let mut events = Vec::new();
                let mut text_parts = Vec::new();

                if let Some(items) = json
                    .get("message")
                    .and_then(|message| message.get("content"))
                    .and_then(|content| content.as_array())
                {
                    for item in items {
                        match item.get("type").and_then(|value| value.as_str()) {
                            Some("text") => {
                                if let Some(text) =
                                    item.get("text").and_then(|value| value.as_str())
                                {
                                    if !text.trim().is_empty() {
                                        text_parts.push(text.to_string());
                                    }
                                }
                            }
                            Some(other) => events.push(ClaudeStreamEvent::Progress {
                                label: format!("assistant:{other}"),
                                detail: Self::extract_detail(item),
                            }),
                            None => {}
                        }
                    }
                }

                if !text_parts.is_empty() {
                    events.insert(0, ClaudeStreamEvent::AssistantText(text_parts.join("\n")));
                }

                events
            }
            "ask_user_question" | "question" | "input_required" => {
                let prompt = json
                    .get("question")
                    .and_then(|value| value.as_str())
                    .or_else(|| json.get("prompt").and_then(|value| value.as_str()))
                    .or_else(|| json.get("message").and_then(|value| value.as_str()))
                    .unwrap_or("Claude Code is waiting for your answer")
                    .to_string();
                let options = json
                    .get("options")
                    .and_then(|value| value.as_array())
                    .map(|values| {
                        values
                            .iter()
                            .filter_map(|value| value.as_str().map(|text| text.to_string()))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                vec![ClaudeStreamEvent::AskUserQuestion { prompt, options }]
            }
            "result" => {
                let detail = json
                    .get("result")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default()
                    .to_string();

                if detail.is_empty() {
                    vec![]
                } else {
                    vec![ClaudeStreamEvent::Progress {
                        label: "result".to_string(),
                        detail,
                    }]
                }
            }
            "system" => vec![ClaudeStreamEvent::Progress {
                label: "system".to_string(),
                detail: Self::extract_detail(&json),
            }],
            other => vec![ClaudeStreamEvent::Progress {
                label: other.to_string(),
                detail: Self::extract_detail(&json),
            }],
        };

        prefix_events.append(&mut events);
        prefix_events
    }

    pub fn execute_instruction_stream(
        project_dir: &PathBuf,
        instruction: &str,
        session_id: Option<&str>,
        sender: UnboundedSender<ClaudeStreamEvent>,
        cancel_flag: Option<Arc<AtomicBool>>,
        child_pid: Option<&std::sync::atomic::AtomicU32>,
    ) -> Result<String> {
        std::fs::create_dir_all(project_dir).with_context(|| {
            format!("Failed to create Claude workdir: {}", project_dir.display())
        })?;

        let binary_path = Self::check_installation().unwrap_or_else(|| PathBuf::from("claude"));
        let permission_flag = super::permission::global().mode().claude_code_flag();
        let mut args = vec![
            "-p",
            instruction,
            "--output-format",
            "stream-json",
            "--verbose",
            "--permission-mode",
            permission_flag,
        ];

        if super::permission::global().mode() == super::permission::PermissionMode::Bypass {
            args.push("--dangerously-skip-permissions");
        }

        let mut cmd = Command::new(&binary_path);
        cmd.args(&args)
            .current_dir(project_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

        if let Some(sid) = session_id {
            cmd.arg("--session-id");
            cmd.arg(sid);
        }

        let mut preview_args = vec![
            format!("-p {:?}", instruction),
            "--output-format stream-json".to_string(),
            "--verbose".to_string(),
            format!("--permission-mode {}", permission_flag),
        ];
        if super::permission::global().mode() == super::permission::PermissionMode::Bypass {
            preview_args.push("--dangerously-skip-permissions".to_string());
        }
        if let Some(sid) = session_id {
            preview_args.push(format!("--session-id {}", sid));
        }
        let command_preview = format!("{} {}", binary_path.display(), preview_args.join(" "));
        let _ = sender.send(ClaudeStreamEvent::Started {
            command: command_preview,
            workdir: project_dir.to_string_lossy().to_string(),
        });

        let mut child = cmd.spawn().with_context(|| {
            format!(
                "Failed to spawn claude process: binary={}, cwd={}",
                binary_path.display(),
                project_dir.display()
            )
        })?;

        // ── 存储子进程 PID（用于外部 kill） ─────────────────────────────
        if let Some(pid_storage) = child_pid {
            pid_storage.store(child.id(), Ordering::SeqCst);
        }

        let stdout = child.stdout.take().context("Failed to capture stdout")?;
        let stderr = child.stderr.take().context("Failed to capture stderr")?;
        let reader = BufReader::new(stdout);
        let stderr_sender = sender.clone();
        let stderr_thread = thread::spawn(move || {
            let stderr_reader = BufReader::new(stderr);
            for line in stderr_reader.lines() {
                match line {
                    Ok(line) if !line.trim().is_empty() => {
                        let _ = stderr_sender.send(ClaudeStreamEvent::Stderr(line));
                    }
                    Ok(_) => {}
                    Err(error) => {
                        let _ = stderr_sender.send(ClaudeStreamEvent::Stderr(format!(
                            "stderr read error: {}",
                            error
                        )));
                        break;
                    }
                }
            }
        });

        let mut result_text = String::new();
        let mut result_override: Option<String> = None;

        for line_result in reader.lines() {
            // ── 检查取消信号 ────────────────────────────────────────────
            if let Some(ref flag) = cancel_flag {
                if flag.load(Ordering::SeqCst) {
                    let _ = child.kill();
                    let _ = sender.send(ClaudeStreamEvent::Failed {
                        error: "用户取消了操作".to_string(),
                    });
                    let _ = child.wait();
                    return Err(anyhow!("cancelled by user"));
                }
            }

            let line = line_result.context("Failed to read line")?;
            for event in Self::parse_stream_line(&line) {
                match &event {
                    ClaudeStreamEvent::AssistantText(text) => {
                        if !result_text.is_empty() {
                            result_text.push('\n');
                        }
                        result_text.push_str(text);
                    }
                    ClaudeStreamEvent::Progress { label, detail } if label == "result" => {
                        if !detail.is_empty() {
                            result_override = Some(detail.clone());
                        }
                    }
                    _ => {}
                }
                let _ = sender.send(event);
            }
        }

        let status = child.wait().context("Failed to wait for process")?;
        let _ = stderr_thread.join();

        if !status.success() {
            return Err(anyhow!("claude exited with status {}", status));
        }

        // ── Git diff: 检测 Claude Code 修改了哪些文件 ──────────────────────
        let modified_files = Self::detect_modified_files(project_dir);
        if !modified_files.is_empty() {
            let _ = sender.send(ClaudeStreamEvent::ModifiedFiles {
                files: modified_files,
            });
        }

        let final_text = if result_text.trim().is_empty() {
            result_override.unwrap_or_default()
        } else {
            result_text.trim().to_string()
        };

        Ok(final_text)
    }

    /// 在 project_dir 中执行 git diff --name-only，返回 Claude Code 修改过的文件列表。
    fn detect_modified_files(project_dir: &PathBuf) -> Vec<String> {
        // 首先检查是否是 git 仓库
        let git_dir = project_dir.join(".git");
        if !git_dir.exists() {
            return vec![];
        }

        let output = match Command::new("git")
            .args(["-C", &project_dir.to_string_lossy(), "diff", "--name-only"])
            .output()
        {
            Ok(o) => o,
            Err(_) => return vec![],
        };

        if !output.status.success() {
            return vec![];
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let files: Vec<String> = stdout
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect();

        if files.is_empty() {
            return vec![];
        }

        files
    }
}

impl Default for ClaudeCodeAgent {
    fn default() -> Self {
        Self::new()
    }
}
