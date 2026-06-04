#![allow(dead_code)]

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde::Deserialize;

use crate::agents::permission::{global as permission, PermissionDecision, ToolKind};

use super::{Skill, SkillCategory, SkillExecution, SkillManifest, SkillPreview, SkillPreviewItem};

pub struct DocSummarizerSkill;

#[derive(Debug, Deserialize, Default)]
struct SummarizerArgs {
    path: Option<String>,
    #[serde(default)]
    max_chars: Option<usize>,
}

const DEFAULT_HEAD: usize = 600;
const DEFAULT_TAIL: usize = 300;

impl DocSummarizerSkill {
    fn supported_ext(ext: &str) -> bool {
        matches!(
            ext.to_ascii_lowercase().as_str(),
            "txt" | "md" | "markdown" | "log" | "csv" | "json" | "yaml" | "yml" | "toml" | "rs" | "py" | "js" | "ts"
        )
    }

    fn read_text(path: &Path) -> std::io::Result<String> {
        std::fs::read_to_string(path)
    }

    fn extract_summary(text: &str, max_chars: usize) -> String {
        let lines: Vec<&str> = text.lines().collect();
        let line_count = lines.len();
        let word_count = text.split_whitespace().count();
        let char_count = text.chars().count();

        let head_chars = max_chars.min(DEFAULT_HEAD);
        let tail_chars = (max_chars / 2).min(DEFAULT_TAIL);
        let head: String = text.chars().take(head_chars).collect();
        let tail: String = if char_count > head_chars + tail_chars {
            text.chars().skip(char_count - tail_chars).collect()
        } else {
            String::new()
        };

        let mut out = String::new();
        out.push_str(&format!(
            "【概览】 字符数 {} · 行数 {} · 词数 {}\n\n",
            char_count, line_count, word_count
        ));
        out.push_str("【开头摘录】\n");
        out.push_str(&head);
        if !tail.is_empty() {
            out.push_str("\n\n【结尾摘录】\n");
            out.push_str(&tail);
        }
        out
    }
}

#[async_trait]
impl Skill for DocSummarizerSkill {
    fn manifest(&self) -> SkillManifest {
        SkillManifest {
            id: "doc.summarizer".to_string(),
            name: "文档摘要".to_string(),
            description: "对文本类文件（txt/md/log/csv/json/源码 等）做抽取式摘要：统计字符/行/词数，并截取首尾。PDF/DOCX 解析待 M4 DocSkill 接入。".to_string(),
            category: SkillCategory::Doc,
        }
    }

    async fn preview(&self, args: serde_json::Value) -> anyhow::Result<SkillPreview> {
        let parsed: SummarizerArgs = serde_json::from_value(args).unwrap_or_default();
        let Some(path_str) = parsed.path else {
            return Ok(SkillPreview {
                summary: "请通过 args.path 指定文件路径".to_string(),
                ..Default::default()
            });
        };
        let path = PathBuf::from(&path_str);
        if !path.exists() || !path.is_file() {
            return Ok(SkillPreview {
                summary: format!("文件不存在或不是普通文件：{}", path.display()),
                ..Default::default()
            });
        }
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !Self::supported_ext(ext) {
            return Ok(SkillPreview {
                summary: format!("暂不支持的文件类型：.{}（M4 将接入 PDF/DOCX）", ext),
                ..Default::default()
            });
        }
        let text = match Self::read_text(&path) {
            Ok(t) => t,
            Err(e) => {
                return Ok(SkillPreview {
                    summary: format!("读取失败：{}", e),
                    ..Default::default()
                });
            }
        };
        let max_chars = parsed.max_chars.unwrap_or(1500).clamp(200, 8000);
        let summary_text = Self::extract_summary(&text, max_chars);

        Ok(SkillPreview {
            summary: format!("已生成摘要（{} 字符）", summary_text.chars().count()),
            items: vec![SkillPreviewItem {
                label: path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default(),
                detail: summary_text,
                bytes: std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0),
            }],
            estimated_bytes: 0,
            warnings: if matches!(ext.to_ascii_lowercase().as_str(), "csv" | "log") {
                vec!["大文件可能仅截取首尾，建议在 M4 接入流式分块 + 嵌入。".to_string()]
            } else {
                vec![]
            },
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<SkillExecution> {
        let parsed: SummarizerArgs = serde_json::from_value(args.clone()).unwrap_or_default();
        let Some(path_str) = parsed.path else {
            return Ok(SkillExecution {
                summary: "缺少 args.path".to_string(),
                ..Default::default()
            });
        };
        let path = PathBuf::from(&path_str);
        if !path.exists() {
            return Ok(SkillExecution {
                summary: format!("文件不存在：{}", path.display()),
                ..Default::default()
            });
        }
        let preview = self.preview(args).await?;
        let summary_text = preview
            .items
            .first()
            .map(|it| it.detail.clone())
            .unwrap_or_default();
        if summary_text.is_empty() {
            return Ok(SkillExecution {
                summary: "未能生成摘要".to_string(),
                ..Default::default()
            });
        }

        let dest = path.with_extension(format!(
            "{}.summary.md",
            path.extension().and_then(|e| e.to_str()).unwrap_or("txt")
        ));
        let detail = format!("doc.summarizer 即将写入摘要到：{}", dest.display());

        match permission().request_async(ToolKind::File, detail).await {
            PermissionDecision::Allow => {}
            PermissionDecision::Deny(reason) => {
                return Ok(SkillExecution {
                    summary: format!("用户已拒绝：{}", reason),
                    denied: true,
                    ..Default::default()
                });
            }
            PermissionDecision::Ask => {
                return Ok(SkillExecution {
                    summary: "审批通道未就绪".to_string(),
                    denied: true,
                    ..Default::default()
                });
            }
        }

        match std::fs::write(&dest, &summary_text) {
            Ok(_) => Ok(SkillExecution {
                summary: format!("摘要已写入 {}", dest.display()),
                freed_bytes: 0,
                success_items: vec![dest.display().to_string()],
                failed_items: vec![],
                denied: false,
            }),
            Err(e) => Ok(SkillExecution {
                summary: format!("写入失败：{}", e),
                denied: false,
                failed_items: vec![(dest.display().to_string(), e.to_string())],
                ..Default::default()
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn manifest_id_is_doc_summarizer() {
        let m = DocSummarizerSkill.manifest();
        assert_eq!(m.id, "doc.summarizer");
        assert!(matches!(m.category, SkillCategory::Doc));
    }

    #[test]
    fn extract_includes_overview() {
        let s = DocSummarizerSkill::extract_summary("hello world\n", 500);
        assert!(s.contains("概览"));
    }

    #[test]
    fn supported_ext_recognizes_md() {
        assert!(DocSummarizerSkill::supported_ext("md"));
        assert!(!DocSummarizerSkill::supported_ext("pdf"));
    }
}
