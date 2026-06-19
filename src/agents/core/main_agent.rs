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
             你可以通过工具执行系统任务、启动编码工作流、写入/读取记忆、提出人格设定草案，或请求在右侧终端执行命令。\n\n\
             Skill 通过 run_system_task 调用：\n\
             - 查看 skill 使用说明 → run_system_task(skill_id=\"xxx\", apply=false)\n\
             - 查看进程/CPU/内存 → skill_id=\"system.tools\" args={{\"tool\": \"list_processes\"}}\n\
             - 查看磁盘空间 → skill_id=\"system.tools\" args={{\"tool\": \"disk_free\"}}\n\
             - 查看目录内容/文件信息 → skill_id=\"system.tools\" args={{\"tool\": \"list_dir\", \"path\": \"...\"}}\n\
             - 分析磁盘占用 → skill_id=\"system.tools\" args={{\"tool\": \"disk_usage\", \"path\": \"...\"}}\n\n\
             用户问系统相关问题时，务必通过 run_system_task 获取真实数据，不要猜测。\n\n\
             编码任务规则：当用户请求开发应用、实现功能、创建页面、修改代码、修复 bug、重构项目时，不要直接给完整代码，也不要直接调用 run_in_terminal。你需要先简要理解和整理用户需求，然后调用 start_coding_workflow。聊天区负责总结 Claude Code 阶段输出；终端区负责展示 Claude Code 执行过程。\n\n\
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
