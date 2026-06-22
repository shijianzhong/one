use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::i18n::Lang;
use crate::ui_theme::ThemeMode;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub model_base_url: String,
    pub model_api_key: String,
    pub model_name: String,
    /// 轻量模型，用于快速分类/闲聊 (可选，默认同 model_name)
    #[serde(default)]
    pub light_model: Option<String>,
    /// 代码 Agent 专用模型 (可选，默认同 model_name)
    #[serde(default)]
    pub coding_model: Option<String>,
    /// 系统 Agent 专用模型 (可选，默认同 model_name)
    #[serde(default)]
    pub system_model: Option<String>,
    pub lang: Lang,
    #[serde(default)]
    pub theme_mode: ThemeMode,
    /// Telegram Bot Token，用于远程触发
    #[serde(default)]
    pub telegram_bot_token: Option<String>,
    /// Telegram 绑定的 chat_id，白名单校验用
    #[serde(default)]
    pub telegram_chat_id: Option<String>,
    /// Telegram 绑定的时间
    #[serde(default)]
    pub telegram_bound_at: Option<String>,
    #[serde(default)]
    pub last_workspace_id: Option<usize>,
    #[serde(default)]
    pub last_task_id: Option<usize>,
    /// 可用的持久 coding CLI provider。
    /// 留空时使用默认 claude/codex。
    #[serde(default = "default_coding_agents")]
    pub coding_agents: Vec<CodingAgentConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CodingAgentConfig {
    pub id: String,
    pub label: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub install_command: Option<String>,
    #[serde(default)]
    pub install_instructions: Option<String>,
}

pub fn default_coding_agents() -> Vec<CodingAgentConfig> {
    vec![
        CodingAgentConfig {
            id: "claude".to_string(),
            label: "Claude".to_string(),
            command: "claude".to_string(),
            args: Vec::new(),
            install_command: Some("curl -fsSL https://claude.ai/install.sh | bash".to_string()),
            install_instructions: Some(
                "Claude Code 官方安装：macOS/Linux/WSL 可运行 `curl -fsSL https://claude.ai/install.sh | bash`，或 macOS 使用 `brew install --cask claude-code`。安装后在项目目录运行 `claude` 并按提示登录。文档：https://code.claude.com/docs"
                    .to_string(),
            ),
        },
        CodingAgentConfig {
            id: "codex".to_string(),
            label: "Codex".to_string(),
            command: "codex".to_string(),
            args: Vec::new(),
            install_command: None,
            install_instructions: Some(
                "未配置 Codex CLI 自动安装命令。请先安装并确保 `codex` 在 PATH 中可用。"
                    .to_string(),
            ),
        },
    ]
}

impl Default for Config {
    fn default() -> Self {
        Self {
            model_base_url: "https://api.openai.com/v1".to_string(),
            model_api_key: "".to_string(),
            model_name: "gpt-4".to_string(),
            light_model: None,
            coding_model: None,
            system_model: None,
            lang: Lang::Zh,
            theme_mode: ThemeMode::Dark,
            telegram_bot_token: None,
            telegram_chat_id: None,
            telegram_bound_at: None,
            last_workspace_id: None,
            last_task_id: None,
            coding_agents: default_coding_agents(),
        }
    }
}

fn get_config_path() -> PathBuf {
    let config_dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".one");
    std::fs::create_dir_all(&config_dir).ok();
    config_dir.join("config.json")
}

pub fn load_config() -> Config {
    let path = get_config_path();
    if path.exists() {
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(config) = serde_json::from_str(&content) {
                return config;
            }
        }
    }
    Config::default()
}

pub fn save_config(config: &Config) -> anyhow::Result<()> {
    let path = get_config_path();
    let content = serde_json::to_string_pretty(config)?;
    std::fs::write(&path, content)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_without_last_open_fields_defaults_to_none() {
        let raw = r#"{
            "model_base_url": "https://api.openai.com/v1",
            "model_api_key": "",
            "model_name": "gpt-4",
            "lang": "Zh",
            "theme_mode": "Dark"
        }"#;

        let config: Config = serde_json::from_str(raw).unwrap();

        assert_eq!(config.last_workspace_id, None);
        assert_eq!(config.last_task_id, None);
        assert_eq!(config.coding_agents, default_coding_agents());
    }
}
