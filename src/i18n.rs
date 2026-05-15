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

impl Translations {
    // Navigation
    pub const NAV_ONE: (&'static str, &'static str) = ("ONE", "ONE");
    pub const NEW_WORKSPACE: (&'static str, &'static str) = ("新建 Workspace", "New Workspace");
    pub const SKILLS: (&'static str, &'static str) = ("技能", "Skills");
    pub const AUTOMATION: (&'static str, &'static str) = ("自动化", "Automation");
    pub const MODEL_CONFIG: (&'static str, &'static str) = ("模型配置", "Model Config");

    // Task list
    pub const WORKSPACES: (&'static str, &'static str) = ("工作空间", "Workspaces");
    pub const NEW_TASK: (&'static str, &'static str) = ("新任务", "New Task");
    pub const DELETE_WORKSPACE: (&'static str, &'static str) = ("删除 Workspace", "Delete Workspace");

    // Chat
    pub const NO_TASK_SELECTED: (&'static str, &'static str) = ("未选择任务", "No task selected");
    pub const TYPE_MESSAGE: (&'static str, &'static str) = ("输入消息...", "Type a message...");
    pub const SEND: (&'static str, &'static str) = ("发送", "Send");
    pub const YOU: (&'static str, &'static str) = ("你", "You");
    pub const ASSISTANT: (&'static str, &'static str) = ("助手", "Assistant");
    pub const THINK: (&'static str, &'static str) = ("思考", "Think");
    pub const EXPORT: (&'static str, &'static str) = ("导出", "Export");

    // Model config dialog
    pub const MODEL_SERVICE_CONFIG: (&'static str, &'static str) = ("模型服务配置", "Model Service Config");
    pub const MODEL_NAME: (&'static str, &'static str) = ("模型名称", "Model Name");
    pub const BASE_URL: (&'static str, &'static str) = ("接口地址", "Base URL");
    pub const API_KEY: (&'static str, &'static str) = ("API Key", "API Key");
    pub const CANCEL: (&'static str, &'static str) = ("取消", "Cancel");
    pub const SAVE: (&'static str, &'static str) = ("保存", "Save");

    // Sidebar
    pub const TODO: (&'static str, &'static str) = ("待办", "Todo");
    pub const ARTIFACTS: (&'static str, &'static str) = ("产物", "Artifacts");
    pub const REFERENCES: (&'static str, &'static str) = ("参考资料", "References");

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