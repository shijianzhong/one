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
