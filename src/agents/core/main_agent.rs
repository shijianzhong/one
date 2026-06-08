use std::sync::Arc;
use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;
use std::fs;

use super::{Agent, AgentContext, AgentResponse, Tool, BaseAgent};

pub struct MainAgent {
    base: BaseAgent,
}

impl MainAgent {
    pub fn new(model: String, api_base: String, api_key: String) -> Self {
        Self::with_workspace(model, api_base, api_key, "Default".to_string())
    }

    pub fn with_workspace(
        model: String,
        api_base: String,
        api_key: String,
        workspace: String,
    ) -> Self {
        let soul_path = dirs::config_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join(".one")
            .join("soul.md");
        let soul_content = fs::read_to_string(&soul_path).unwrap_or_else(|_| {
            "你是一个通用的 AI 助手。".to_string()
        });

        let system_prompt = format!(
            "{}\n\n当前日期：{}\n操作环境：{}\n\n请严格按照上述灵魂设定和准则行动。\n\n你有 remember 和 recall 两个记忆工具：\n\
             - 每次对话开始时先调 recall 查看已有信息，避免重复提问。\n\
             - 遇到关于用户个人的信息（姓名、偏好、职业、语言习惯）→ remember(scope=\"global\")。\n\
             - 遇到关于当前项目/工作区的信息（技术栈、规范、ed 路径、团队成员）→ remember(scope=\"workspace\")。\n\
             - 不确定时 → remember(scope=\"both\")，宁可多存不要漏存。\n\n\
             你可以使用 update_work_dir 工具切换工作目录。\n\n\
             你有系统工具能力（run_system_task），可以实时查看电脑状态：\n\
             - 查看运行中的进程、CPU 占用、内存使用 → skill_id=\"system.tools\" args={{\"tool\": \"list_processes\"}}\n\
             - 查看磁盘剩余空间 → skill_id=\"system.tools\" args={{\"tool\": \"disk_free\"}}\n\
             - 查看目录内容、文件信息 → skill_id=\"system.tools\" args={{\"tool\": \"list_dir\", \"path\": \"...\"}}\n\
             用户问系统相关问题时，务必通过 run_system_task 调用 system.tools 获取真实数据，不要猜测。\n\n\
             目前没有安装编码相关的技能（skill）。如果用户需要编写代码，请告知用户当前没有编码技能可用，\
             需要先在技能市场中安装后才能使用。",
            soul_content,
            chrono::Local::now().format("%Y-%m-%d"),
            std::env::consts::OS,
        );

        let tools: Vec<Arc<dyn Tool>> = vec![
            Arc::new(RunSystemTaskTool),
            Arc::new(AnalyzeDiskTool),
            Arc::new(CleanDiskTool),
            Arc::new(RememberTool { workspace: workspace.clone() }),
            Arc::new(RecallTool { workspace: workspace.clone() }),
            Arc::new(ProposeSoulUpdateTool),
            Arc::new(UpdateWorkDirTool),
        ];

        Self {
            base: BaseAgent {
                id: "main".to_string(),
                name: "Main Agent".to_string(),
                system_prompt,
                tools,
                model,
                api_base,
                api_key,
            },
        }
    }
}

#[async_trait]
impl Agent for MainAgent {
    fn id(&self) -> &str { &self.base.id }
    fn name(&self) -> &str { &self.base.name }
    fn system_prompt(&self) -> &str { &self.base.system_prompt }
    fn tools(&self) -> Vec<Arc<dyn Tool>> { self.base.tools.clone() }

    async fn step(&self, context: &mut AgentContext) -> Result<AgentResponse> {
        self.base.call_llm(context).await
    }

    async fn step_stream(
        &self,
        context: &mut AgentContext,
        on_delta: Box<dyn FnMut(String) + Send>,
    ) -> Result<AgentResponse> {
        self.base.call_llm_stream(context, on_delta).await
    }
}

// --- Tools for MainAgent ---

/// 标识工具：触发 SkillRegistry 执行系统任务。由 Orchestrator 拦截并转发到注册的 Skill。
struct RunSystemTaskTool;
#[async_trait]
impl Tool for RunSystemTaskTool {
    fn name(&self) -> &str { "run_system_task" }
    fn description(&self) -> &str {
        "执行系统级任务。通过 skill_id 调用已注册 Skill（当前可用：system.cleaner, desktop.organizer, app.uninstaller, doc.summarizer, media.dedup, system.tools）。\
         先设置 apply=false 预览，再 apply=true 执行。\n\n\
         **system.tools** 用于系统信息查询（进程/CPU/内存/磁盘）：用户问「查看进程」「CPU 占用」「内存使用」「运行中的应用」「磁盘空间」等时应当优先使用。\
         支持的 tool：list_processes, top_memory_procs, get_process_detail, disk_usage, disk_free, list_dir, file_info。"
    }
    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "skill_id": {
                    "type": "string",
                    "description": "目标 Skill 的 id，例如 system.tools。"
                },
                "apply": {
                    "type": "boolean",
                    "description": "仅当填了 skill_id 时有效：false 仅做 preview（只读、可重复），true 才执行 execute（需用户授权）。默认 false。"
                },
                "args": {
                    "type": "object",
                    "description": "传给 Skill preview/execute 的参数（结构由 Skill 自身决定）。"
                },
                "task": {
                    "type": "string",
                    "description": "未命中 Skill 时的自然语言任务描述，会交给 SystemAgent 处理。"
                }
            }
        })
    }
    async fn call(&self, _args: serde_json::Value) -> Result<serde_json::Value> {
        Ok(json!({ "status": "intercepted_by_orchestrator" }))
    }
}

/// 记忆存储工具
struct RememberTool {
    workspace: String,
}
#[async_trait]
impl Tool for RememberTool {
    fn name(&self) -> &str { "remember" }
    fn description(&self) -> &str { "记录关于用户的长期偏好、姓名或重要背景事实。" }
    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "fact": { "type": "string", "description": "需要记住的事实内容" },
                "scope": { 
                    "type": "string", 
                    "enum": ["global", "workspace", "both"],
                    "description": "存储范围。global：跨 workspace 的个人信息；workspace：仅限当前项目；both：同时存。默认 both。"
                }
            },
            "required": ["fact"]
        })
    }
    async fn call(&self, args: serde_json::Value) -> Result<serde_json::Value> {
        let fact = args["fact"].as_str().unwrap_or_default();
        let scope = args["scope"].as_str().unwrap_or("both");
        
        match scope {
            "global" => crate::memory::profile::save_global_fact(fact, None)?,
            "workspace" => crate::memory::profile::save_fact(&self.workspace, fact, None)?,
            _ => {
                crate::memory::profile::save_global_fact(fact, None)?;
                crate::memory::profile::save_fact(&self.workspace, fact, None)?;
            }
        }
        
        Ok(json!({ "status": "success", "message": format!("Fact remembered in scope: {}", scope) }))
    }
}

/// 记忆检索工具
struct RecallTool {
    workspace: String,
}
#[async_trait]
impl Tool for RecallTool {
    fn name(&self) -> &str { "recall" }
    fn description(&self) -> &str { "查询已保存的关于用户的长期事实和偏好。" }
    fn parameters_schema(&self) -> serde_json::Value {
        json!({ "type": "object", "properties": {} })
    }
    async fn call(&self, _args: serde_json::Value) -> Result<serde_json::Value> {
        let mut set = std::collections::HashSet::new();
        
        // 合并全局和工作区事实并去重
        for f in crate::memory::profile::get_global_facts() {
            set.insert(f);
        }
        for f in crate::memory::profile::get_all_facts(&self.workspace) {
            set.insert(f);
        }
        
        let facts: Vec<String> = set.into_iter().collect();
        Ok(json!(facts))
    }
}

/// 灵魂草案工具：把"修改 soul.md"的请求写入审核队列，等待用户在 GUI 中确认。
/// 不再允许 LLM 直接覆盖 soul.md（避免自我改写人格）。
struct ProposeSoulUpdateTool;
#[async_trait]
impl Tool for ProposeSoulUpdateTool {
    fn name(&self) -> &str { "propose_soul_update" }
    fn description(&self) -> &str { "提交一份对你自身人格设定（soul.md）的修订草案。仅写入审核队列，必须经用户在界面上确认后才会真正生效。" }
    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "rationale": { "type": "string", "description": "为什么需要更新人格设定的简要说明" },
                "new_soul_content": { "type": "string", "description": "完整的、更新后的 soul.md 内容（草案）" }
            },
            "required": ["rationale", "new_soul_content"]
        })
    }
    async fn call(&self, args: serde_json::Value) -> Result<serde_json::Value> {
        let rationale = args["rationale"].as_str().unwrap_or_default().to_string();
        let content = args["new_soul_content"].as_str().unwrap_or_default().to_string();
        if content.is_empty() {
            return Err(anyhow::anyhow!("New soul content cannot be empty"));
        }
        match crate::agents::soul::submit_proposal(rationale, content) {
            Some(id) => Ok(json!({
                "status": "queued",
                "proposal_id": id,
                "message": "草案已提交，等待用户在界面上审核确认后才会写入 soul.md"
            })),
            None => Err(anyhow::anyhow!("soul proposal queue unavailable")),
        }
    }
}

/// 磁盘分析工具：分析指定目录的磁盘占用情况
struct AnalyzeDiskTool;
#[async_trait]
impl Tool for AnalyzeDiskTool {
    fn name(&self) -> &str { "analyze_disk" }
    fn description(&self) -> &str {
        "分析指定目录或整个磁盘的空间使用情况，列出占用空间最大的子目录和文件。\
         返回分析结果，包括总使用量、大目录列表、可清理建议。\
         用户确认后可以使用 clean_disk 工具进行清理。\
         不传 path 时默认分析用户主目录。"
    }
    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "要分析的目录路径，默认为用户主目录" }
            }
        })
    }
    async fn call(&self, args: serde_json::Value) -> Result<serde_json::Value> {
        let path = args["path"].as_str()
            .map(|p| expand_path_for_disk(p))
            .unwrap_or_else(|| {
                dirs::home_dir()
                    .map(|h| h.to_string_lossy().to_string())
                    .unwrap_or_else(|| ".".to_string())
            });

        // 1. 查看磁盘总体使用情况
        let disk_free_result = system_tools::tools::disk::disk_free().unwrap_or_default();

        // 2. 分析目录深度1的子目录占用
        let usage_detail = system_tools::tools::disk::disk_usage_detailed(&path, 1)
            .unwrap_or_else(|_| "无法获取磁盘使用详情".to_string());

        // 3. 深度分析几个常见大目录
        let home = dirs::home_dir()
            .map(|h| h.to_string_lossy().to_string())
            .unwrap_or_default();
        let big_dirs = ["Downloads", "Desktop", "Documents", ".Trash", "Library/Caches"];
        let mut deep_analysis = String::new();
        for dir in &big_dirs {
            let full_path = format!("{}/{}", home, dir);
            if std::path::Path::new(&full_path).exists() {
                if let Ok(size) = system_tools::tools::disk::disk_usage_detailed(&full_path, 0) {
                    deep_analysis.push_str(&format!("{}: {}", dir, size.trim()));
                    deep_analysis.push('\n');
                }
            }
        }

        Ok(json!({
            "path": path,
            "disk_free": disk_free_result,
            "directory_analysis": usage_detail,
            "key_directories": deep_analysis,
            "hint": "如需清理，请调用 clean_disk 工具并提供清理路径。"
        }))
    }
}

/// 磁盘清理工具：清理指定路径下的文件
struct CleanDiskTool;
#[async_trait]
impl Tool for CleanDiskTool {
    fn name(&self) -> &str { "clean_disk" }
    fn description(&self) -> &str {
        "清理指定路径下的文件或目录。调用前应先用 analyze_disk 分析磁盘占用，\
         并向用户展示结果获得确认后再执行。\
         支持清空废纸篓、清理下载文件夹、清理缓存等。"
    }
    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "target": {
                    "type": "string",
                    "description": "清理目标：trash（废纸篓）、downloads（下载文件夹）、caches（用户缓存）、custom（自定义路径）"
                },
                "path": { "type": "string", "description": "当 target=custom 时，指定要删除的具体文件或目录路径" }
            },
            "required": ["target"]
        })
    }
    async fn call(&self, args: serde_json::Value) -> Result<serde_json::Value> {
        let target = args["target"].as_str().unwrap_or("").to_string();
        let custom_path = args["path"].as_str().map(|p| expand_path_for_disk(p));

        let (result, freed_bytes) = match target.as_str() {
            "trash" => {
                let home = dirs::home_dir().unwrap_or_default();
                let trash_path = home.join(".Trash");
                let mut count = 0u64;
                let size_before = dir_size(&trash_path);

                // 优先用 osascript 清空废纸篓（避免 macOS 权限问题）
                let osa_result = std::process::Command::new("osascript")
                    .args(["-e", r#"tell application "Finder" to empty trash"#])
                    .output();

                match osa_result {
                    Ok(output) if output.status.success() => {
                        (format!("✅ 已通过 Finder 清空废纸篓"), size_before)
                    }
                    _ => {
                        // fallback: 手动删除 .Trash 内容
                        if trash_path.exists() {
                            let mut fallback_count = 0u64;
                            for entry in std::fs::read_dir(&trash_path).map_err(|e| anyhow::anyhow!("{}", e))? {
                                let entry = entry.map_err(|e| anyhow::anyhow!("{}", e))?;
                                let path = entry.path();
                                if path.is_dir() {
                                    std::fs::remove_dir_all(&path).ok();
                                } else {
                                    std::fs::remove_file(&path).ok();
                                }
                                fallback_count += 1;
                            }
                            (format!("已清空废纸篓，清理了 {} 个项目", fallback_count), size_before)
                        } else {
                            ("废纸篓已为空".to_string(), 0)
                        }
                    }
                }
            }
            "downloads" => {
                let downloads = dirs::home_dir()
                    .unwrap_or_default()
                    .join("Downloads");
                let size_before = dir_size(&downloads);
                let mut count = 0u64;
                for entry in std::fs::read_dir(&downloads).map_err(|e| anyhow::anyhow!("{}", e))? {
                    let entry = entry.map_err(|e| anyhow::anyhow!("{}", e))?;
                    let path = entry.path();
                    if path.is_dir() {
                        std::fs::remove_dir_all(&path).ok();
                    } else {
                        std::fs::remove_file(&path).ok();
                    }
                    count += 1;
                }
                (format!("已清空下载文件夹，清理了 {} 个项目", count), size_before)
            }
            "caches" => {
                let home = dirs::home_dir().unwrap_or_default();
                let caches_path = home.join("Library").join("Caches");
                let size_before = dir_size(&caches_path);
                let mut count = 0u64;
                for entry in std::fs::read_dir(&caches_path).map_err(|e| anyhow::anyhow!("{}", e))? {
                    let entry = entry.map_err(|e| anyhow::anyhow!("{}", e))?;
                    let path = entry.path();
                    if path.is_dir() {
                        std::fs::remove_dir_all(&path).ok();
                    } else {
                        std::fs::remove_file(&path).ok();
                    }
                    count += 1;
                }
                (format!("已清理缓存目录，清理了 {} 个项目", count), size_before)
            }
            "custom" => {
                if let Some(p) = custom_path {
                    let pp = std::path::Path::new(&p);
                    if pp.exists() {
                        let size_before = dir_size(pp);
                        if pp.is_dir() {
                            std::fs::remove_dir_all(pp).map_err(|e| anyhow::anyhow!("{}", e))?;
                        } else {
                            std::fs::remove_file(pp).map_err(|e| anyhow::anyhow!("{}", e))?;
                        }
                        (format!("已删除：{}", p), size_before)
                    } else {
                        (format!("路径不存在：{}", p), 0)
                    }
                } else {
                    ("请指定要删除的路径".to_string(), 0)
                }
            }
            _ => (format!("未知的清理目标：{}", target), 0),
        };

        Ok(json!({
            "status": "success",
            "message": result,
            "freed_bytes": freed_bytes,
            "freed_human": human_bytes(freed_bytes)
        }))
    }
}

/// 工作目录切换工具：允许 MainAgent 动态切换 Claude Code 的启动目录
struct UpdateWorkDirTool;
#[async_trait]
impl Tool for UpdateWorkDirTool {
    fn name(&self) -> &str { "update_work_dir" }
    fn description(&self) -> &str {
        "切换当前工作目录至指定路径。默认情况下，整个工作区根目录为工作目录以保持全局视野。\
         当需要运行 cargo、npm、go 等构建工具命令，且在当前目录找不到对应的配置文件时，\
         应使用此工具切换至对应的子项目目录（如 server/、frontend/、backend/ 等）。\
         注意：此工具仅修改 Claude Code 的执行目录，不影响工作区结构。"
    }
    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "目标子项目目录路径，支持相对于当前工作目录的相对路径或绝对路径"
                }
            },
            "required": ["path"]
        })
    }
    async fn call(&self, args: serde_json::Value) -> Result<serde_json::Value> {
        let path = args["path"].as_str().unwrap_or_default().to_string();
        if path.is_empty() {
            return Ok(json!({ "status": "error", "message": "未提供路径参数" }));
        }
        Ok(json!({
            "status": "success",
            "message": format!("准备切换工作目录至: {}。将在下一次 Claude Code 调用时生效。", path)
        }))
    }
}

fn expand_path_for_disk(path: &str) -> String {
    let expanded = if path.starts_with("~/") || path == "~" {
        dirs::home_dir()
            .map(|home| {
                if path == "~" { home.to_string_lossy().to_string() }
                else { home.join(path.trim_start_matches("~/")).to_string_lossy().to_string() }
            })
            .unwrap_or_else(|| path.to_string())
    } else {
        path.to_string()
    };
    // 中文目录名映射
    let chinese_map = [
        ("桌面", "Desktop"), ("下载", "Downloads"), ("文档", "Documents"),
        ("图片", "Pictures"), ("音乐", "Music"), ("视频", "Movies"),
    ];
    let mut result = expanded;
    for (cn, en) in &chinese_map {
        if result.contains(cn) {
            let home = dirs::home_dir().map(|h| h.to_string_lossy().to_string()).unwrap_or_default();
            result = result.replace(cn, &format!("{}/{}", home, en));
        }
    }
    result
}

fn dir_size(path: &std::path::Path) -> u64 {
    let mut total = 0u64;
    if let Ok(rd) = std::fs::read_dir(path) {
        for entry in rd.flatten() {
            if let Ok(meta) = entry.metadata() {
                if meta.is_symlink() { continue; }
                if meta.is_dir() {
                    total = total.saturating_add(dir_size(&entry.path()));
                } else if meta.is_file() {
                    total = total.saturating_add(meta.len());
                }
            }
        }
    }
    total
}

fn human_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut idx = 0;
    while size >= 1024.0 && idx < UNITS.len() - 1 {
        size /= 1024.0;
        idx += 1;
    }
    format!("{:.1} {}", size, UNITS[idx])
}
