//! .mcp.json 配置解析
//!
//! 配置文件放在项目根目录 ~/.one/mcp.json，格式：
//! ```json
//! {
//!   "mcpServers": {
//!     "server-name": {
//!       "transport": "stdio",
//!       "command": "python3",
//!       "args": ["script.py"],
//!       "env": {
//!         "KEY": "${ENV_VAR_OR_VALUE}"
//!       }
//!     }
//!   }
//! }
//! ```

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

/// MCP 服务器传输方式
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "transport")]
pub enum TransportConfig {
    #[serde(rename = "stdio")]
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: HashMap<String, String>,
    },
    #[serde(rename = "http")]
    Http {
        url: String,
        #[serde(default)]
        headers: HashMap<String, String>,
    },
}

/// MCP 服务器配置
#[derive(Debug, Clone, Deserialize)]
pub struct McpServerConfig {
    #[serde(flatten)]
    pub transport: TransportConfig,
    /// 自动重启（仅 stdio）
    #[serde(default = "default_true")]
    pub auto_restart: bool,
    /// 连接超时（秒）
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
}

fn default_true() -> bool {
    true
}

fn default_timeout() -> u64 {
    30
}

/// MCP 全局配置
#[derive(Debug, Clone, Deserialize)]
pub struct McpConfig {
    #[serde(default, alias = "mcpServers")]
    pub mcp_servers: HashMap<String, McpServerConfig>,
}

impl McpConfig {
    /// 从文件加载配置
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if !path.exists() {
            return Ok(Self {
                mcp_servers: HashMap::new(),
            });
        }
        let content = std::fs::read_to_string(path)
            .context(format!("Failed to read MCP config: {}", path.display()))?;
        let config: Self = serde_json::from_str(&content)
            .context(format!("Failed to parse MCP config: {}", path.display()))?;
        Ok(config)
    }

    /// 从标准路径加载（依次尝试项目目录和用户目录）
    pub fn load_default() -> Result<Self> {
        // 优先从项目根目录加载
        let project_path = Path::new(".mcp.json");
        if project_path.exists() {
            return Self::load(project_path);
        }
        // 其次从 ~/.one/ 加载
        if let Some(home) = dirs::home_dir() {
            let user_path = home.join(".one").join("mcp.json");
            if user_path.exists() {
                return Self::load(user_path);
            }
        }
        Ok(Self {
            mcp_servers: HashMap::new(),
        })
    }

    /// 执行环境变量替换（${VAR_NAME} 或直接用值）
    pub fn resolve_env(env: &HashMap<String, String>) -> HashMap<String, String> {
        let mut resolved = HashMap::new();
        for (key, value) in env {
            let resolved_value = if value.starts_with("${") && value.ends_with('}') {
                let var_name = &value[2..value.len() - 1];
                std::env::var(var_name).unwrap_or_else(|_| String::new())
            } else {
                value.clone()
            };
            resolved.insert(key.clone(), resolved_value);
        }
        resolved
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_stdio_config() {
        let json = r#"{
            "mcpServers": {
                "test-server": {
                    "transport": "stdio",
                    "command": "python3",
                    "args": ["server.py"],
                    "env": {
                        "API_KEY": "${MY_API_KEY}"
                    }
                }
            }
        }"#;
        let config: McpConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.mcp_servers.len(), 1);
        let server = &config.mcp_servers["test-server"];
        match &server.transport {
            TransportConfig::Stdio { command, args, .. } => {
                assert_eq!(command, "python3");
                assert_eq!(args[0], "server.py");
            }
            _ => panic!("expected stdio transport"),
        }
    }

    #[test]
    fn test_parse_http_config() {
        let json = r#"{
            "mcpServers": {
                "remote": {
                    "transport": "http",
                    "url": "https://example.com/mcp",
                    "headers": {
                        "Authorization": "Bearer token123"
                    }
                }
            }
        }"#;
        let config: McpConfig = serde_json::from_str(json).unwrap();
        let server = &config.mcp_servers["remote"];
        match &server.transport {
            TransportConfig::Http { url, .. } => {
                assert_eq!(url, "https://example.com/mcp");
            }
            _ => panic!("expected http transport"),
        }
    }

    #[test]
    fn test_empty_config() {
        let config: McpConfig = serde_json::from_str("{}").unwrap();
        assert!(config.mcp_servers.is_empty());
    }

    #[test]
    fn test_env_var_resolution() {
        let mut env = HashMap::new();
        env.insert("KEY".to_string(), "${PATH}".to_string());
        let resolved = McpConfig::resolve_env(&env);
        assert!(!resolved["KEY"].is_empty()); // PATH 总是存在的
    }
}
