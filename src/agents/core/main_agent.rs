use std::fs;

use crate::agents::core::AgentTrait;

pub struct MainAgent {
    system_prompt: String,
    model: String,
    api_base: String,
    api_key: String,
}

impl MainAgent {
    pub fn with_workspace(
        model: String,
        api_base: String,
        api_key: String,
        _workspace: String,
    ) -> Self {
        let soul_path = dirs::config_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join(".one")
            .join("soul.md");
        let soul_content = fs::read_to_string(&soul_path)
            .unwrap_or_else(|_| "你是一个通用的 AI 助手。".to_string());

        let system_prompt = format!(
            "{}\n\n当前日期：{}\n操作环境：{}\n\n请严格按照上述灵魂设定和准则行动。\n\n\
             你可以通过工具执行系统任务、启动和操作持久 coding CLI 会话、写入/读取记忆、提出人格设定草案，或请求在右侧终端执行命令。\n\n\
             Skill 通过 run_system_task 调用：\n\
             - 查看 skill 使用说明 → run_system_task(skill_id=\"xxx\", apply=false)\n\
             - 查看进程/CPU/内存 → skill_id=\"system.tools\" args={{\"tool\": \"list_processes\"}}\n\
             - 查看磁盘空间 → skill_id=\"system.tools\" args={{\"tool\": \"disk_free\"}}\n\
             - 查看目录内容/文件信息 → skill_id=\"system.tools\" args={{\"tool\": \"list_dir\", \"path\": \"...\"}}\n\
             - 分析磁盘占用 → skill_id=\"system.tools\" args={{\"tool\": \"disk_usage\", \"path\": \"...\"}}\n\n\
             用户问系统相关问题时，务必通过 run_system_task 获取真实数据，不要猜测。\n\n\
             编码任务规则：当用户请求开发应用、实现功能、创建页面、修改代码、修复 bug、重构项目时，不要直接给完整代码，也不要直接调用 run_in_terminal。你是用户与右侧终端中真实交互式编码 CLI runtime 的中间人。第一次编码前先调用 detect_coding_clis 检查本机是否有 Claude Code、Codex、Gemini 等 CLI。若只有一个已安装 CLI，可优先使用它；若多个已安装 CLI，先让用户选择；若没有已安装 CLI，优先询问是否安装 Claude Code。只有用户明确同意安装后，才调用 install_coding_cli(confirmed=true)；安装失败时把安装说明给用户。选定 CLI 后调用 start_coding_terminal_runtime，这会打开右侧终端并在 workspace root 运行对应命令，例如 claude。启动后把用户需求整理成清晰的任务说明发送给终端 runtime，然后停止本轮，不要持续轮询或复述终端输出；后台会在 runtime 需要用户确认、登录、选择或授权时提醒用户。当前 task 已有 runtime 时，用户说“继续/同意/选 1/按这个改”等，应把用户新内容理解、拆解后调用 send_to_coding_terminal_runtime 转发到同一个 runtime；如果用户是在回应 Claude Code 的选项，“同意/可以/允许/选1”转发 `1`，“全部允许/本次都允许/选2”转发 `2`，“拒绝/不要/选3”转发 `3`。用户问进度、状态、是否卡住、Claude 在等什么时，优先调用 inspect_coding_terminal_runtime，并只解释 status/kind/suggested_message，不要把 raw terminal log 贴到聊天区；只有用户明确要求“原始日志/最近输出/完整输出”时才调用 read_coding_terminal_output。写代码、改文件、创建应用等任务 write_mode=true；只读查看或状态查询可为 false。\n\n\
             需要执行普通非编码命令时，使用 run_in_terminal。命令在右侧终端中执行，用户可以实时看到输出。",
            soul_content,
            chrono::Local::now().format("%Y-%m-%d"),
            std::env::consts::OS,
        );

        Self {
            system_prompt,
            model,
            api_base,
            api_key,
        }
    }
}

impl AgentTrait for MainAgent {
    fn id(&self) -> &str {
        "main"
    }

    fn name(&self) -> &str {
        "Main Agent"
    }

    fn soul_prompt(&self) -> &str {
        &self.system_prompt
    }

    fn model(&self) -> &str {
        &self.model
    }

    fn api_base(&self) -> &str {
        &self.api_base
    }

    fn api_key(&self) -> &str {
        &self.api_key
    }
}
