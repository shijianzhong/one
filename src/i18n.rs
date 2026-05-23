use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Lang {
    Zh,
    En,
}

impl Default for Lang {
    fn default() -> Self {
        Lang::Zh
    }
}

impl Lang {
    pub fn toggle(self) -> Self {
        match self {
            Lang::Zh => Lang::En,
            Lang::En => Lang::Zh,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Lang::Zh => "中",
            Lang::En => "EN",
        }
    }
}

pub struct Translations;

#[allow(dead_code)]
impl Translations {
    // Navigation
    pub const NAV_ONE: (&'static str, &'static str) = ("ONE", "ONE");
    pub const NEW_WORKSPACE: (&'static str, &'static str) = ("新建 Workspace", "New Workspace");
    pub const SKILLS: (&'static str, &'static str) = ("技能", "Skills");
    pub const SKILLS_HINT: (&'static str, &'static str) = ("上传并管理技能包（.skill / .zip）", "Upload and manage skill packages (.skill / .zip)");
    pub const UPLOAD_SKILL_PACKAGE: (&'static str, &'static str) = ("上传技能包", "Upload Skill Package");
    pub const UPLOAD: (&'static str, &'static str) = ("上传", "Upload");
    pub const NO_SKILLS: (&'static str, &'static str) = ("还没有安装任何技能。", "No skills installed yet.");
    pub const DETAILS: (&'static str, &'static str) = ("详情", "Details");
    pub const CLOSE: (&'static str, &'static str) = ("关闭", "Close");
    pub const AUTOMATION: (&'static str, &'static str) = ("自动化", "Automation");
    pub const MODEL_CONFIG: (&'static str, &'static str) = ("模型配置", "Model Config");
    pub const CAPABILITIES: (&'static str, &'static str) = ("能力", "Capabilities");
    pub const MODELS: (&'static str, &'static str) = ("模型", "Models");
    pub const SETTINGS: (&'static str, &'static str) = ("设置", "Settings");
    pub const SUPPORT: (&'static str, &'static str) = ("支持", "Support");
    pub const EXPLORER: (&'static str, &'static str) = ("工作台", "Explorer");
    pub const WORKFLOWS: (&'static str, &'static str) = ("工作流", "Workflows");
    pub const API: (&'static str, &'static str) = ("API", "API");
    pub const PLACEHOLDER_ENTRY: (&'static str, &'static str) = ("占位入口", "Placeholder");

    // Task list
    pub const WORKSPACES: (&'static str, &'static str) = ("工作空间", "Workspaces");
    pub const WORKSPACES_HEADING: (&'static str, &'static str) = ("工作空间", "WORKSPACES");
    pub const NEW_TASK: (&'static str, &'static str) = ("新任务", "New Task");
    pub const DELETE_WORKSPACE: (&'static str, &'static str) = ("删除 Workspace", "Delete Workspace");

    // Chat
    pub const NO_TASK_SELECTED: (&'static str, &'static str) = ("未选择任务", "No task selected");
    pub const TYPE_MESSAGE: (&'static str, &'static str) = ("输入消息...", "Type a message...");
    pub const SEND: (&'static str, &'static str) = ("发送", "Send");
    pub const SENDING: (&'static str, &'static str) = ("发送中...", "Sending...");
    pub const WAITING_FOR_AI_RESPONSE: (&'static str, &'static str) = ("等待 AI 回复...", "Waiting for AI response...");
    pub const GENERATING_RESPONSE: (&'static str, &'static str) = ("生成回复中...", "Generating response...");
    pub const YOU: (&'static str, &'static str) = ("你", "You");
    pub const ASSISTANT: (&'static str, &'static str) = ("助手", "Assistant");
    pub const THINK: (&'static str, &'static str) = ("思考", "Think");
    pub const THINKING_IN_PROGRESS: (&'static str, &'static str) = ("正在思考", "Thinking");
    pub const THINKING_DONE: (&'static str, &'static str) = ("思考完成", "Thought complete");
    pub const AI_IS_THINKING: (&'static str, &'static str) = ("AI 正在思考...", "AI is thinking...");
    pub const EXPORT: (&'static str, &'static str) = ("导出", "Export");

    // Model config dialog
    pub const MODEL_SERVICE_CONFIG: (&'static str, &'static str) = ("模型服务配置", "Model Service Config");
    pub const MODEL_NAME: (&'static str, &'static str) = ("模型名称", "Model Name");
    pub const BASE_URL: (&'static str, &'static str) = ("接口地址", "Base URL");
    pub const API_KEY: (&'static str, &'static str) = ("API Key", "API Key");
    pub const API_KEY_PLACEHOLDER: (&'static str, &'static str) = ("sk-...", "sk-...");
    pub const CANCEL: (&'static str, &'static str) = ("取消", "Cancel");
    pub const SAVE: (&'static str, &'static str) = ("保存", "Save");

    // Export
    pub const EXPORT_JSON_TITLE: (&'static str, &'static str) = ("导出 JSON", "Export JSON");
    pub const EXPORT_MARKDOWN_TITLE: (&'static str, &'static str) = ("导出 Markdown", "Export Markdown");
    pub const SAVE_JSON: (&'static str, &'static str) = ("保存 JSON", "Save JSON");
    pub const SAVE_MARKDOWN: (&'static str, &'static str) = ("保存 Markdown", "Save Markdown");
    pub const JSON: (&'static str, &'static str) = ("JSON", "JSON");
    pub const MARKDOWN: (&'static str, &'static str) = ("Markdown", "Markdown");

    // Sidebar
    pub const TODO: (&'static str, &'static str) = ("待办", "Todo");
    pub const ARTIFACTS: (&'static str, &'static str) = ("产物", "Artifacts");
    pub const REFERENCES: (&'static str, &'static str) = ("参考资料", "References");
    pub const CLAUDE_CODE_RUN: (&'static str, &'static str) = ("Claude 运行", "Claude Code Run");
    pub const CLAUDE_CODE: (&'static str, &'static str) = ("Claude Code", "Claude Code");
    pub const CLAUDE_CODE_RUNNING_ELLIPSIS: (&'static str, &'static str) = ("Claude Code 运行中...", "Claude Code is running...");
    pub const CLAUDE_CODE_CONTINUING_ELLIPSIS: (&'static str, &'static str) = ("Claude Code 继续中...", "Claude Code is continuing...");
    pub const WAITING_FOR_CLAUDE_START: (&'static str, &'static str) = ("等待 Claude Code 启动...", "Waiting for Claude Code to start...");
    pub const CONTINUING_CLAUDE_RUN: (&'static str, &'static str) = ("继续运行中", "Continuing Claude Code run");
    pub const USER_ANSWERED: (&'static str, &'static str) = ("用户已回答", "User answered");
    pub const PROCESS_STARTED: (&'static str, &'static str) = ("进程已启动", "Process started");
    pub const STREAMING_RESPONSE: (&'static str, &'static str) = ("正在流式输出", "Streaming response");
    pub const CLAUDE_STARTED_LIVE_CONTENT: (&'static str, &'static str) = ("Claude Code 开始返回实时内容", "Claude Code started returning live content");
    pub const SESSION_UPDATED: (&'static str, &'static str) = ("会话已更新", "Session updated");
    pub const SESSION_ID: (&'static str, &'static str) = ("会话 ID", "session_id");
    pub const QUESTION: (&'static str, &'static str) = ("问题", "Question");
    pub const OPTIONS: (&'static str, &'static str) = ("选项", "Options");
    pub const CLAUDE_CODE_RUNNING: (&'static str, &'static str) = ("Claude Code 运行中", "Claude Code is running");
    pub const CLAUDE_WAITING_FOR_ANSWER: (&'static str, &'static str) = ("等待你的回答", "Claude Code is waiting for your answer");
    pub const CLAUDE_COMPLETED: (&'static str, &'static str) = ("已完成", "Claude Code completed");
    pub const CLAUDE_FAILED: (&'static str, &'static str) = ("失败", "Claude Code failed");
    pub const RUN_QUEUED: (&'static str, &'static str) = ("任务已排队", "Run queued");
    pub const INSTRUCTION_SUBMITTED: (&'static str, &'static str) = ("已提交指令", "Instruction submitted");
    pub const PREVIEW_AFTER_RUN_NOTE: (&'static str, &'static str) = ("预览将在运行完成后准备", "Preview will be prepared after the run completes");
    pub const RUN_COMPLETED: (&'static str, &'static str) = ("运行完成", "Run completed");
    pub const GENERATED_CHARACTERS: (&'static str, &'static str) = ("生成字符数", "Generated characters");
    pub const RUN_FAILED: (&'static str, &'static str) = ("运行失败", "Run failed");
    pub const CLAUDE_EXECUTION_ERROR: (&'static str, &'static str) = ("Claude Code 执行错误：", "Claude Code execution error: ");
    pub const CLAUDE_CODE_TAG: (&'static str, &'static str) = ("[Claude Code]", "[Claude Code]");
    pub const RUN_FAILED_TAG: (&'static str, &'static str) = ("[运行失败]", "[Run failed]");
    pub const PREVIEW_READY_EVENT: (&'static str, &'static str) = ("预览就绪", "Preview ready");
    pub const BROWSER_OPENED: (&'static str, &'static str) = ("已打开浏览器", "Browser opened");
    pub const OPENED_PREVIEW_URL: (&'static str, &'static str) = ("已在外部浏览器打开预览链接", "Opened preview URL in external browser");
    pub const PREVIEW_SKIPPED: (&'static str, &'static str) = ("跳过预览", "Preview skipped");
    pub const PREVIEW_FAILED_EVENT: (&'static str, &'static str) = ("预览失败", "Preview failed");
    pub const SERVING_WORKSPACE_ROOT: (&'static str, &'static str) = ("正在托管工作空间根目录", "Serving workspace root");
    pub const FAILED_TO_START_PREVIEW_SERVER: (&'static str, &'static str) = ("预览服务启动失败", "Failed to start preview server");
    pub const PREVIEW_DIR_MISSING: (&'static str, &'static str) = ("预览目录不存在", "Preview directory does not exist");
    pub const NO_PREVIEWABLE_HTML: (&'static str, &'static str) = ("该工作空间未找到可预览的 HTML 文件。", "No previewable HTML file found in this workspace.");
    pub const PROGRESS: (&'static str, &'static str) = ("进度", "Progress");
    pub const PREVIEW: (&'static str, &'static str) = ("预览", "Preview");
    pub const OPEN_IN_BROWSER: (&'static str, &'static str) = ("在浏览器打开", "Open In Browser");
    pub const OPEN_TASK_FOLDER: (&'static str, &'static str) = ("打开任务目录", "Open Task Folder");
    pub const REVEAL: (&'static str, &'static str) = ("在访达中显示", "Reveal");
    pub const NO_PREVIEW_INFO: (&'static str, &'static str) = ("暂无预览信息", "No preview information");
    pub const ENTRY: (&'static str, &'static str) = ("入口文件", "Entry");
    pub const QUESTIONS: (&'static str, &'static str) = ("问题", "Questions");
    pub const SUBMIT: (&'static str, &'static str) = ("提交", "Submit");
    pub const NO_PENDING_QUESTIONS: (&'static str, &'static str) = ("暂无待回答的问题。", "No pending questions from Claude Code.");
    pub const LIVE_OUTPUT: (&'static str, &'static str) = ("实时输出", "Live Output");
    pub const COMMAND: (&'static str, &'static str) = ("命令", "Command");
    pub const STDERR: (&'static str, &'static str) = ("错误输出", "stderr");
    pub const NO_STDERR_OUTPUT: (&'static str, &'static str) = ("无错误输出", "No stderr output");
    pub const COMMAND_NOT_STARTED: (&'static str, &'static str) = ("Claude 命令尚未开始", "Claude command has not started yet");
    pub const TIMELINE: (&'static str, &'static str) = ("时间线", "Timeline");
    pub const NO_CLAUDE_RUN_YET: (&'static str, &'static str) = ("暂无 Claude 运行记录", "No Claude run yet");
    pub const CLAUDE_PANEL_HINT: (&'static str, &'static str) = ("发送 Claude Code 请求后，这里将显示实时进度、日志和最终结果。", "Send a Claude Code request and this panel will show live progress, logs, and the final result.");
    pub const ANSWER_CLAUDE_QUESTION: (&'static str, &'static str) = ("回答 Claude 的问题...", "Answer Claude's question...");
    pub const WORKDIR: (&'static str, &'static str) = ("工作目录", "Workdir");
    pub const NO_ARTIFACTS_YET: (&'static str, &'static str) = ("该任务暂无产物。", "No artifacts detected for this task yet.");
    pub const STATUS_RUNNING: (&'static str, &'static str) = ("运行中", "Running");
    pub const STATUS_COMPLETED: (&'static str, &'static str) = ("已完成", "Completed");
    pub const STATUS_FAILED: (&'static str, &'static str) = ("失败", "Failed");
    pub const PREVIEW_IDLE: (&'static str, &'static str) = ("空闲", "Idle");
    pub const PREVIEW_READY: (&'static str, &'static str) = ("就绪", "Ready");

    // Terminal
    pub const TERMINAL: (&'static str, &'static str) = ("终端", "Terminal");
    pub const TYPE_COMMAND: (&'static str, &'static str) = ("输入命令...", "Type a command...");

    // Errors
    pub const ERROR: (&'static str, &'static str) = ("错误", "Error");
    pub const SPAWN_ERROR: (&'static str, &'static str) = ("执行错误", "Spawn error");
    pub const TOKIO_ERROR: (&'static str, &'static str) = ("Tokio 错误", "Tokio error");
}

pub fn t(lang: Lang, key: (&'static str, &'static str)) -> &'static str {
    match lang {
        Lang::Zh => key.0,
        Lang::En => key.1,
    }
}
