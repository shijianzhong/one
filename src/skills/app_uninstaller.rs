#![allow(dead_code)]

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde::Deserialize;

use crate::agents::permission::{global as permission, PermissionDecision, ToolKind};

use super::{Skill, SkillCategory, SkillExecution, SkillManifest, SkillPreview, SkillPreviewItem};

pub struct AppUninstallerSkill;

#[derive(Debug, Deserialize, Default)]
struct UninstallArgs {
    app: Option<String>,
}

impl AppUninstallerSkill {
    fn applications_dirs() -> Vec<PathBuf> {
        let mut v = vec![PathBuf::from("/Applications")];
        if let Some(home) = dirs::home_dir() {
            v.push(home.join("Applications"));
        }
        v
    }

    fn list_installed() -> Vec<(String, PathBuf)> {
        let mut out = Vec::new();
        for dir in Self::applications_dirs() {
            let Ok(read_dir) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in read_dir.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if !name.ends_with(".app") {
                    continue;
                }
                let stem = name.trim_end_matches(".app").to_string();
                out.push((stem, entry.path()));
            }
        }
        out
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
                let Ok(meta) = entry.metadata() else { continue };
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

    fn residue_dirs(app_name: &str) -> Vec<PathBuf> {
        let mut v = Vec::new();
        let Some(home) = dirs::home_dir() else {
            return v;
        };
        let lib = home.join("Library");
        let candidates = [
            lib.join("Application Support").join(app_name),
            lib.join("Caches").join(app_name),
            lib.join("Preferences").join(format!("{}.plist", app_name)),
            lib.join("Logs").join(app_name),
            lib.join("Saved Application State").join(format!("{}.savedState", app_name)),
            lib.join("Containers").join(app_name),
        ];
        for c in candidates {
            if c.exists() {
                v.push(c);
            }
        }
        v
    }

    fn locate_app(name: &str) -> Option<(String, PathBuf)> {
        let lower = name.to_lowercase();
        let lower_trim = lower.trim_end_matches(".app");
        Self::list_installed()
            .into_iter()
            .find(|(stem, _)| stem.to_lowercase() == lower_trim)
    }
}

#[async_trait]
impl Skill for AppUninstallerSkill {
    fn manifest(&self) -> SkillManifest {
        SkillManifest {
            id: "app.uninstaller".to_string(),
            name: "应用卸载".to_string(),
            description: "卸载 macOS 应用并清理 ~/Library 下的残留（Application Support / Caches / Preferences / Logs 等）。".to_string(),
            category: SkillCategory::App,
            danger_level: crate::agents::permission::DangerLevel::Dangerous,
        }
    }

    async fn preview(&self, args: serde_json::Value) -> anyhow::Result<SkillPreview> {
        let parsed: UninstallArgs = serde_json::from_value(args).unwrap_or_default();
        let Some(target) = parsed.app else {
            let installed = Self::list_installed();
            let summary = format!("发现 {} 个 .app；请用 args.app 指定要卸载的应用名（不含 .app）", installed.len());
            let items = installed
                .into_iter()
                .take(20)
                .map(|(stem, path)| SkillPreviewItem {
                    label: stem,
                    detail: path.display().to_string(),
                    bytes: 0,
                })
                .collect();
            return Ok(SkillPreview {
                summary,
                items,
                estimated_bytes: 0,
                warnings: vec!["未指定 app 参数，仅列出候选。".to_string()],
            });
        };

        let Some((stem, app_path)) = Self::locate_app(&target) else {
            return Ok(SkillPreview {
                summary: format!("未找到应用：{}", target),
                ..Default::default()
            });
        };

        let app_size = Self::dir_size(&app_path);
        let mut items = vec![SkillPreviewItem {
            label: format!("{}（主体）", stem),
            detail: app_path.display().to_string(),
            bytes: app_size,
        }];
        let mut total = app_size;
        for r in Self::residue_dirs(&stem) {
            let bytes = if r.is_dir() {
                Self::dir_size(&r)
            } else {
                std::fs::metadata(&r).map(|m| m.len()).unwrap_or(0)
            };
            total = total.saturating_add(bytes);
            items.push(SkillPreviewItem {
                label: "残留".to_string(),
                detail: r.display().to_string(),
                bytes,
            });
        }

        Ok(SkillPreview {
            summary: format!("即将卸载 {}（含残留共 {} 字节）", stem, total),
            items,
            estimated_bytes: total,
            warnings: vec![
                "卸载操作不可恢复，请确认你不再需要该应用的本地数据。".to_string(),
                "Preferences 与登录项可能仍在 plist 缓存中，重启后才完全清理。".to_string(),
            ],
        })
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        source: Option<&str>,
    ) -> anyhow::Result<SkillExecution> {
        let parsed: UninstallArgs = serde_json::from_value(args).unwrap_or_default();
        let Some(target) = parsed.app else {
            return Ok(SkillExecution {
                summary: "缺少 args.app".to_string(),
                ..Default::default()
            });
        };
        let Some((stem, app_path)) = Self::locate_app(&target) else {
            return Ok(SkillExecution {
                summary: format!("未找到应用：{}", target),
                ..Default::default()
            });
        };
        let residues = Self::residue_dirs(&stem);

        let detail = format!(
            "app.uninstaller 即将删除：\n{}\n以及残留：\n{}",
            app_path.display(),
            residues
                .iter()
                .map(|p| format!("- {}", p.display()))
                .collect::<Vec<_>>()
                .join("\n")
        );

        match permission().request_async(ToolKind::File, detail, source).await {
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

        let mut freed: u64 = 0;
        let mut success = Vec::new();
        let mut failed: Vec<(String, String)> = Vec::new();
        let app_size = Self::dir_size(&app_path);
        match std::fs::remove_dir_all(&app_path) {
            Ok(_) => {
                freed = freed.saturating_add(app_size);
                success.push(app_path.display().to_string());
            }
            Err(e) => failed.push((app_path.display().to_string(), e.to_string())),
        }
        for r in residues {
            let size = if r.is_dir() {
                Self::dir_size(&r)
            } else {
                std::fs::metadata(&r).map(|m| m.len()).unwrap_or(0)
            };
            let res = if r.is_dir() {
                std::fs::remove_dir_all(&r)
            } else {
                std::fs::remove_file(&r)
            };
            match res {
                Ok(_) => {
                    freed = freed.saturating_add(size);
                    success.push(r.display().to_string());
                }
                Err(e) => failed.push((r.display().to_string(), e.to_string())),
            }
        }

        Ok(SkillExecution {
            summary: format!("已卸载 {}，共释放 {} 字节", stem, freed),
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
    async fn manifest_id_is_app_uninstaller() {
        let m = AppUninstallerSkill.manifest();
        assert_eq!(m.id, "app.uninstaller");
        assert!(matches!(m.category, SkillCategory::App));
    }

    #[tokio::test]
    async fn preview_without_app_lists_candidates() {
        let p = AppUninstallerSkill
            .preview(serde_json::json!({}))
            .await
            .expect("preview ok");
        assert!(!p.warnings.is_empty());
    }
}
