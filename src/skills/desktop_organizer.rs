#![allow(dead_code)]

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde::Deserialize;

use crate::agents::permission::{global as permission, PermissionDecision, ToolKind};

use super::{Skill, SkillCategory, SkillExecution, SkillManifest, SkillPreview, SkillPreviewItem};

pub struct DesktopOrganizerSkill;

#[derive(Debug, Deserialize, Default)]
struct OrganizerArgs {
    #[serde(default)]
    folder: Option<String>,
    #[serde(default)]
    dry_run: Option<bool>,
}

const CATEGORIES: &[(&str, &[&str])] = &[
    ("图片", &["png", "jpg", "jpeg", "gif", "webp", "heic", "bmp", "svg"]),
    ("视频", &["mp4", "mov", "avi", "mkv", "webm", "m4v"]),
    ("音频", &["mp3", "wav", "flac", "m4a", "aac", "ogg"]),
    ("文档", &["pdf", "doc", "docx", "txt", "md", "rtf", "pages"]),
    ("表格", &["xls", "xlsx", "csv", "numbers"]),
    ("演示", &["ppt", "pptx", "key"]),
    ("压缩包", &["zip", "tar", "gz", "rar", "7z", "dmg", "iso"]),
    ("代码", &["rs", "py", "js", "ts", "tsx", "jsx", "go", "swift", "java", "c", "cpp", "h"]),
];

impl DesktopOrganizerSkill {
    fn target_folder(args: &OrganizerArgs) -> Option<PathBuf> {
        if let Some(folder) = args.folder.as_deref() {
            let p = PathBuf::from(folder);
            if p.exists() {
                return Some(p);
            }
        }
        dirs::home_dir().map(|h| h.join("Desktop"))
    }

    fn classify(ext: &str) -> &'static str {
        let lower = ext.to_ascii_lowercase();
        for (label, exts) in CATEGORIES {
            if exts.iter().any(|e| *e == lower) {
                return label;
            }
        }
        "其他"
    }

    fn collect(folder: &Path) -> std::io::Result<Vec<(String, PathBuf, u64)>> {
        let mut out = Vec::new();
        if !folder.exists() {
            return Ok(out);
        }
        for entry in std::fs::read_dir(folder)? {
            let Ok(entry) = entry else { continue };
            let path = entry.path();
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_symlink() || meta.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("");
            let category = Self::classify(ext).to_string();
            out.push((category, path, meta.len()));
        }
        Ok(out)
    }
}

#[async_trait]
impl Skill for DesktopOrganizerSkill {
    fn manifest(&self) -> SkillManifest {
        SkillManifest {
            id: "desktop.organizer".to_string(),
            name: "桌面整理".to_string(),
            description: "按文件类型把目标目录（默认 ~/Desktop）顶层文件分类挪到子目录（图片/视频/文档 等）。".to_string(),
            category: SkillCategory::Desktop,
        }
    }

    async fn preview(&self, args: serde_json::Value) -> anyhow::Result<SkillPreview> {
        let parsed: OrganizerArgs = serde_json::from_value(args).unwrap_or_default();
        let folder = match Self::target_folder(&parsed) {
            Some(f) => f,
            None => {
                return Ok(SkillPreview {
                    summary: "未找到目标目录".to_string(),
                    ..Default::default()
                });
            }
        };

        let files = Self::collect(&folder).unwrap_or_default();
        if files.is_empty() {
            return Ok(SkillPreview {
                summary: format!("目录 {} 为空或无可分类文件", folder.display()),
                ..Default::default()
            });
        }

        let mut groups: std::collections::BTreeMap<String, (u64, u64)> = std::collections::BTreeMap::new();
        let mut total: u64 = 0;
        for (cat, _, size) in &files {
            let entry = groups.entry(cat.clone()).or_insert((0, 0));
            entry.0 += 1;
            entry.1 += size;
            total = total.saturating_add(*size);
        }

        let items = groups
            .into_iter()
            .map(|(cat, (_count, size))| SkillPreviewItem {
                label: cat.clone(),
                detail: format!("将移动到 {}/{}", folder.display(), cat),
                bytes: size,
            })
            .chain(std::iter::once(SkillPreviewItem {
                label: "样例".to_string(),
                detail: files
                    .iter()
                    .take(5)
                    .map(|(_, p, _)| {
                        p.file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default()
                    })
                    .collect::<Vec<_>>()
                    .join(", "),
                bytes: 0,
            }))
            .collect::<Vec<_>>();

        Ok(SkillPreview {
            summary: format!("将整理 {} 个文件（共 {} 字节）", files.len(), total),
            items,
            estimated_bytes: total,
            warnings: vec!["仅处理顶层文件，不会递归子目录；子目录保持不动。".to_string()],
        })
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        source: Option<&str>,
    ) -> anyhow::Result<SkillExecution> {
        let parsed: OrganizerArgs = serde_json::from_value(args).unwrap_or_default();
        let folder = match Self::target_folder(&parsed) {
            Some(f) => f,
            None => {
                return Ok(SkillExecution {
                    summary: "未找到目标目录".to_string(),
                    denied: false,
                    ..Default::default()
                });
            }
        };

        let files = Self::collect(&folder).unwrap_or_default();
        if files.is_empty() {
            return Ok(SkillExecution {
                summary: "没有可整理文件".to_string(),
                ..Default::default()
            });
        }

        let detail = format!(
            "desktop.organizer 即将整理 {} 个文件到 {} 下的分类目录",
            files.len(),
            folder.display()
        );
        match permission()
            .request_async(ToolKind::File, detail, source)
            .await
        {
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

        let mut moved: u64 = 0;
        let mut success = Vec::new();
        let mut failed: Vec<(String, String)> = Vec::new();
        for (cat, src, size) in files {
            let dest_dir = folder.join(&cat);
            if let Err(e) = std::fs::create_dir_all(&dest_dir) {
                failed.push((src.display().to_string(), format!("mkdir 失败: {}", e)));
                continue;
            }
            let file_name = match src.file_name() {
                Some(n) => n.to_owned(),
                None => {
                    failed.push((src.display().to_string(), "无法识别文件名".to_string()));
                    continue;
                }
            };
            let dest = dest_dir.join(file_name);
            match std::fs::rename(&src, &dest) {
                Ok(_) => {
                    moved = moved.saturating_add(size);
                    success.push(format!("{} → {}", src.display(), dest.display()));
                }
                Err(e) => failed.push((src.display().to_string(), e.to_string())),
            }
        }

        Ok(SkillExecution {
            summary: format!("已整理 {} 个文件（{} 字节）", success.len(), moved),
            freed_bytes: 0,
            success_items: success,
            failed_items: failed,
            denied: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn manifest_id_is_desktop_organizer() {
        let m = DesktopOrganizerSkill.manifest();
        assert_eq!(m.id, "desktop.organizer");
        assert!(matches!(m.category, SkillCategory::Desktop));
    }

    #[test]
    fn classify_known_extensions() {
        assert_eq!(DesktopOrganizerSkill::classify("png"), "图片");
        assert_eq!(DesktopOrganizerSkill::classify("PDF"), "文档");
        assert_eq!(DesktopOrganizerSkill::classify("xyz"), "其他");
    }
}
