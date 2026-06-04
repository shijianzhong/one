#![allow(dead_code)]

use std::collections::BTreeMap;
use std::hash::Hasher;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde::Deserialize;

use crate::agents::permission::{global as permission, PermissionDecision, ToolKind};

use super::{Skill, SkillCategory, SkillExecution, SkillManifest, SkillPreview, SkillPreviewItem};

pub struct MediaDedupSkill;

#[derive(Debug, Deserialize, Default)]
struct DedupArgs {
    folder: Option<String>,
    #[serde(default)]
    extensions: Option<Vec<String>>,
    #[serde(default)]
    keep_strategy: Option<String>,
}

const DEFAULT_EXTS: &[&str] = &[
    "jpg", "jpeg", "png", "heic", "gif", "webp", "bmp", "mp4", "mov", "avi", "mkv", "m4v",
];

const SAMPLE_BYTES: u64 = 65536;

impl MediaDedupSkill {
    fn target_folder(args: &DedupArgs) -> Option<PathBuf> {
        if let Some(f) = &args.folder {
            let p = PathBuf::from(f);
            if p.exists() {
                return Some(p);
            }
        }
        dirs::home_dir().map(|h| h.join("Pictures"))
    }

    fn collect(folder: &Path, exts: &[String]) -> Vec<(PathBuf, u64)> {
        let mut out = Vec::new();
        let mut stack = vec![folder.to_path_buf()];
        while let Some(cur) = stack.pop() {
            let Ok(read_dir) = std::fs::read_dir(&cur) else {
                continue;
            };
            for entry in read_dir.flatten() {
                let path = entry.path();
                let Ok(meta) = entry.metadata() else { continue };
                if meta.is_symlink() {
                    continue;
                }
                if meta.is_dir() {
                    stack.push(path);
                    continue;
                }
                let ext = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|s| s.to_ascii_lowercase())
                    .unwrap_or_default();
                if exts.iter().any(|e| *e == ext) {
                    out.push((path, meta.len()));
                }
            }
        }
        out
    }

    fn hash_sample(path: &Path, size: u64) -> Option<u64> {
        use std::io::Read;
        let mut file = std::fs::File::open(path).ok()?;
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        hasher.write_u64(size);
        let cap = SAMPLE_BYTES.min(size) as usize;
        let mut buf = vec![0u8; cap];
        let n = file.read(&mut buf).ok()?;
        hasher.write(&buf[..n]);
        Some(hasher.finish())
    }

    fn group_dups(files: Vec<(PathBuf, u64)>) -> Vec<Vec<(PathBuf, u64)>> {
        let mut size_groups: BTreeMap<u64, Vec<(PathBuf, u64)>> = BTreeMap::new();
        for (p, s) in files {
            size_groups.entry(s).or_default().push((p, s));
        }
        let mut out = Vec::new();
        for (_size, group) in size_groups {
            if group.len() < 2 {
                continue;
            }
            let mut hash_groups: BTreeMap<u64, Vec<(PathBuf, u64)>> = BTreeMap::new();
            for (p, s) in group {
                if let Some(h) = Self::hash_sample(&p, s) {
                    hash_groups.entry(h).or_default().push((p, s));
                }
            }
            for (_h, g) in hash_groups {
                if g.len() >= 2 {
                    out.push(g);
                }
            }
        }
        out
    }

    fn pick_keeper<'a>(
        group: &'a [(PathBuf, u64)],
        strategy: &str,
    ) -> &'a (PathBuf, u64) {
        match strategy {
            "shortest_path" => group
                .iter()
                .min_by_key(|(p, _)| p.as_os_str().len())
                .unwrap_or(&group[0]),
            "newest" => group
                .iter()
                .max_by_key(|(p, _)| {
                    std::fs::metadata(p)
                        .and_then(|m| m.modified())
                        .ok()
                        .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
                })
                .unwrap_or(&group[0]),
            // default: oldest (treat earliest copy as canonical)
            _ => group
                .iter()
                .min_by_key(|(p, _)| {
                    std::fs::metadata(p)
                        .and_then(|m| m.modified())
                        .ok()
                        .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
                })
                .unwrap_or(&group[0]),
        }
    }
}

#[async_trait]
impl Skill for MediaDedupSkill {
    fn manifest(&self) -> SkillManifest {
        SkillManifest {
            id: "media.dedup".to_string(),
            name: "媒体去重".to_string(),
            description: "递归扫描目录，按文件大小 + 首 64KB 哈希识别重复的图片/视频，预览重复组与可释放空间，确认后删除冗余副本（保留最旧/最新/最短路径，由 keep_strategy 决定）。".to_string(),
            category: SkillCategory::Media,
        }
    }

    async fn preview(&self, args: serde_json::Value) -> anyhow::Result<SkillPreview> {
        let parsed: DedupArgs = serde_json::from_value(args).unwrap_or_default();
        let folder = match Self::target_folder(&parsed) {
            Some(f) => f,
            None => {
                return Ok(SkillPreview {
                    summary: "未找到目标目录".to_string(),
                    ..Default::default()
                });
            }
        };
        let exts: Vec<String> = parsed
            .extensions
            .unwrap_or_else(|| DEFAULT_EXTS.iter().map(|s| s.to_string()).collect());
        let files = Self::collect(&folder, &exts);
        let total_files = files.len();
        let groups = Self::group_dups(files);

        if groups.is_empty() {
            return Ok(SkillPreview {
                summary: format!(
                    "{} 共扫描 {} 个媒体文件，未发现重复。",
                    folder.display(),
                    total_files
                ),
                ..Default::default()
            });
        }

        let mut total_redundant: u64 = 0;
        let strategy = parsed.keep_strategy.as_deref().unwrap_or("oldest");
        let items: Vec<SkillPreviewItem> = groups
            .iter()
            .take(50)
            .map(|g| {
                let keeper = Self::pick_keeper(g, strategy);
                let redundant: u64 = g
                    .iter()
                    .filter(|(p, _)| p != &keeper.0)
                    .map(|(_, s)| *s)
                    .sum();
                total_redundant = total_redundant.saturating_add(redundant);
                SkillPreviewItem {
                    label: format!("重复 {} 份", g.len()),
                    detail: format!(
                        "保留 {}\n冗余 {}",
                        keeper.0.display(),
                        g.iter()
                            .filter(|(p, _)| p != &keeper.0)
                            .map(|(p, _)| p.display().to_string())
                            .collect::<Vec<_>>()
                            .join("\n")
                    ),
                    bytes: redundant,
                }
            })
            .collect();

        Ok(SkillPreview {
            summary: format!(
                "{} 中发现 {} 组重复，预计释放 {} 字节",
                folder.display(),
                groups.len(),
                total_redundant
            ),
            items,
            estimated_bytes: total_redundant,
            warnings: vec![
                "采用 size + 首 64KB 哈希做近似识别，碰撞概率虽低但不为零；删除前请抽查样例。".to_string(),
                "默认保留最旧文件，可通过 keep_strategy=oldest|newest|shortest_path 调整。".to_string(),
            ],
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<SkillExecution> {
        let parsed: DedupArgs = serde_json::from_value(args).unwrap_or_default();
        let folder = match Self::target_folder(&parsed) {
            Some(f) => f,
            None => {
                return Ok(SkillExecution {
                    summary: "未找到目标目录".to_string(),
                    ..Default::default()
                });
            }
        };
        let exts: Vec<String> = parsed
            .extensions
            .unwrap_or_else(|| DEFAULT_EXTS.iter().map(|s| s.to_string()).collect());
        let strategy = parsed.keep_strategy.as_deref().unwrap_or("oldest");
        let files = Self::collect(&folder, &exts);
        let groups = Self::group_dups(files);
        if groups.is_empty() {
            return Ok(SkillExecution {
                summary: "未发现重复文件".to_string(),
                ..Default::default()
            });
        }

        let detail = format!(
            "media.dedup 即将在 {} 中删除 {} 组冗余副本（策略：{}）",
            folder.display(),
            groups.len(),
            strategy
        );
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

        let mut freed: u64 = 0;
        let mut success: Vec<String> = Vec::new();
        let mut failed: Vec<(String, String)> = Vec::new();
        for group in groups {
            let keeper = Self::pick_keeper(&group, strategy).clone();
            for (p, size) in group.into_iter().filter(|(p, _)| p != &keeper.0) {
                match std::fs::remove_file(&p) {
                    Ok(_) => {
                        freed = freed.saturating_add(size);
                        success.push(p.display().to_string());
                    }
                    Err(e) => failed.push((p.display().to_string(), e.to_string())),
                }
            }
        }

        Ok(SkillExecution {
            summary: format!("已删除 {} 个冗余副本，释放 {} 字节", success.len(), freed),
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
    async fn manifest_id_is_media_dedup() {
        let m = MediaDedupSkill.manifest();
        assert_eq!(m.id, "media.dedup");
        assert!(matches!(m.category, SkillCategory::Media));
    }

    #[test]
    fn group_dups_returns_only_duplicate_groups() {
        let groups = MediaDedupSkill::group_dups(vec![
            (PathBuf::from("/tmp/a"), 1),
            (PathBuf::from("/tmp/b"), 2),
        ]);
        assert!(groups.is_empty());
    }
}
