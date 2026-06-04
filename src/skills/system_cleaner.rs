#![allow(dead_code)]

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde::Deserialize;

use crate::agents::permission::{global as permission, PermissionDecision, ToolKind};

use super::{Skill, SkillCategory, SkillExecution, SkillManifest, SkillPreview, SkillPreviewItem};

pub struct SystemCleanerSkill;

#[derive(Debug, Deserialize, Default)]
struct CleanerArgs {
    #[serde(default)]
    targets: Option<Vec<String>>,
}

impl SystemCleanerSkill {
    fn candidate_targets() -> Vec<(String, PathBuf)> {
        let mut v = Vec::new();
        if let Some(home) = dirs::home_dir() {
            v.push((
                "用户缓存（~/Library/Caches）".to_string(),
                home.join("Library").join("Caches"),
            ));
            v.push((
                "Xcode DerivedData".to_string(),
                home.join("Library")
                    .join("Developer")
                    .join("Xcode")
                    .join("DerivedData"),
            ));
            v.push((
                "Xcode iOS DeviceSupport".to_string(),
                home.join("Library")
                    .join("Developer")
                    .join("Xcode")
                    .join("iOS DeviceSupport"),
            ));
            v.push((
                "Homebrew 缓存".to_string(),
                home.join("Library").join("Caches").join("Homebrew"),
            ));
            v.push(("废纸篓".to_string(), home.join(".Trash")));
        }
        v
    }

    fn dir_size(path: &Path) -> u64 {
        if !path.exists() {
            return 0;
        }
        let mut total: u64 = 0;
        let mut stack = vec![path.to_path_buf()];
        while let Some(cur) = stack.pop() {
            let Ok(read_dir) = std::fs::read_dir(&cur) else {
                continue;
            };
            for entry in read_dir.flatten() {
                let Ok(meta) = entry.metadata() else {
                    continue;
                };
                if meta.is_symlink() {
                    continue;
                }
                if meta.is_dir() {
                    stack.push(entry.path());
                } else if meta.is_file() {
                    total = total.saturating_add(meta.len());
                }
            }
        }
        total
    }

    fn human(bytes: u64) -> String {
        const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
        let mut size = bytes as f64;
        let mut idx = 0;
        while size >= 1024.0 && idx < UNITS.len() - 1 {
            size /= 1024.0;
            idx += 1;
        }
        format!("{:.1} {}", size, UNITS[idx])
    }

    fn purge_dir_contents(path: &Path) -> std::io::Result<u64> {
        let mut freed: u64 = 0;
        if !path.exists() {
            return Ok(0);
        }
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let p = entry.path();
            let size_before = Self::dir_size(&p);
            let res = if entry.file_type()?.is_dir() {
                std::fs::remove_dir_all(&p)
            } else {
                std::fs::remove_file(&p)
            };
            if res.is_ok() {
                freed = freed.saturating_add(size_before);
            }
        }
        Ok(freed)
    }
}

#[async_trait]
impl Skill for SystemCleanerSkill {
    fn manifest(&self) -> SkillManifest {
        SkillManifest {
            id: "system.cleaner".to_string(),
            name: "系统清理".to_string(),
            description: "扫描 macOS 缓存、Xcode DerivedData、废纸篓等可清理目录，预览大小后由用户确认再执行删除。".to_string(),
            category: SkillCategory::System,
        }
    }

    async fn preview(&self, _args: serde_json::Value) -> anyhow::Result<SkillPreview> {
        let candidates = Self::candidate_targets();
        let mut items = Vec::with_capacity(candidates.len());
        let mut total: u64 = 0;
        for (label, path) in &candidates {
            if !path.exists() {
                continue;
            }
            let bytes = Self::dir_size(path);
            total = total.saturating_add(bytes);
            items.push(SkillPreviewItem {
                label: label.clone(),
                detail: path.display().to_string(),
                bytes,
            });
        }
        let summary = if items.is_empty() {
            "未发现可清理目录".to_string()
        } else {
            format!(
                "共 {} 个目录可清理，预计释放 {}",
                items.len(),
                Self::human(total)
            )
        };
        Ok(SkillPreview {
            summary,
            items,
            estimated_bytes: total,
            warnings: vec!["将仅删除目录内文件，不会删除目录本身。Caches 与废纸篓清理不可恢复。".to_string()],
        })
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        source: Option<&str>,
    ) -> anyhow::Result<SkillExecution> {
        let parsed: CleanerArgs = serde_json::from_value(args).unwrap_or_default();
        let allow_set = parsed.targets;

        let candidates = Self::candidate_targets();
        let to_run: Vec<(String, PathBuf)> = candidates
            .into_iter()
            .filter(|(label, path)| {
                path.exists()
                    && allow_set
                        .as_ref()
                        .map(|s| s.contains(label))
                        .unwrap_or(true)
            })
            .collect();

        if to_run.is_empty() {
            return Ok(SkillExecution {
                summary: "没有可清理项".to_string(),
                ..Default::default()
            });
        }

        let detail = to_run
            .iter()
            .map(|(label, path)| format!("{} ({})", label, path.display()))
            .collect::<Vec<_>>()
            .join("\n");

        match permission()
            .request_async(
                ToolKind::File,
                format!("system.cleaner 即将清理：\n{}", detail),
                source,
            )
            .await
        {
            PermissionDecision::Allow => {}
            PermissionDecision::Deny(reason) => {
                return Ok(SkillExecution {
                    summary: format!("用户已拒绝执行：{}", reason),
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

        let mut freed: u64 = 0;
        let mut success = Vec::new();
        let mut failed = Vec::new();
        for (label, path) in &to_run {
            match Self::purge_dir_contents(path) {
                Ok(bytes) => {
                    freed = freed.saturating_add(bytes);
                    success.push(format!("{} (-{})", label, Self::human(bytes)));
                }
                Err(e) => {
                    failed.push((label.clone(), e.to_string()));
                }
            }
        }

        Ok(SkillExecution {
            summary: format!("已清理 {} 项，共释放 {}", success.len(), Self::human(freed)),
            freed_bytes: freed,
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
    async fn manifest_returns_expected_id() {
        let m = SystemCleanerSkill.manifest();
        assert_eq!(m.id, "system.cleaner");
        assert!(matches!(m.category, SkillCategory::System));
    }

    #[tokio::test]
    async fn preview_runs_without_panicking() {
        let preview = SystemCleanerSkill
            .preview(serde_json::json!({}))
            .await
            .expect("preview should succeed");
        assert!(!preview.warnings.is_empty());
    }

    #[test]
    fn human_formats_bytes() {
        assert_eq!(SystemCleanerSkill::human(0), "0.0 B");
        assert_eq!(SystemCleanerSkill::human(2048), "2.0 KB");
    }
}
