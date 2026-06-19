use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::agents::core::AgentTrait;
use crate::services::config::Config;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentDefinition {
    pub id: String,
    pub name: String,
    pub system_prompt: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub api_base: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub tool_filter: Vec<String>,
    #[serde(default)]
    pub metadata: Value,
}

impl AgentDefinition {
    pub fn from_node_config(agent_id: &str, config: &Value) -> Result<Option<Self>> {
        if let Some(definition) = config.get("agent_definition") {
            let mut parsed: AgentDefinition = serde_json::from_value(definition.clone())
                .with_context(|| format!("failed to parse agent definition '{}'", agent_id))?;
            parsed.normalize(agent_id)?;
            return Ok(Some(parsed));
        }

        let system_prompt = config
            .get("system_prompt")
            .or_else(|| config.get("soul_prompt"))
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty());

        let Some(system_prompt) = system_prompt else {
            return Ok(None);
        };

        let mut definition = AgentDefinition {
            id: config
                .get("id")
                .and_then(|value| value.as_str())
                .unwrap_or(agent_id)
                .to_string(),
            name: config
                .get("name")
                .and_then(|value| value.as_str())
                .unwrap_or(agent_id)
                .to_string(),
            system_prompt: system_prompt.to_string(),
            model: string_field(config, "model"),
            api_base: string_field(config, "api_base"),
            api_key: string_field(config, "api_key"),
            tool_filter: string_array_field(config, "tool_filter"),
            metadata: config.get("metadata").cloned().unwrap_or(Value::Null),
        };
        definition.normalize(agent_id)?;
        Ok(Some(definition))
    }

    fn normalize(&mut self, fallback_id: &str) -> Result<()> {
        self.id = self.id.trim().to_string();
        if self.id.is_empty() {
            self.id = fallback_id.trim().to_string();
        }
        self.name = self.name.trim().to_string();
        if self.name.is_empty() {
            self.name = self.id.clone();
        }
        self.system_prompt = self.system_prompt.trim().to_string();
        if self.id.is_empty() {
            anyhow::bail!("agent id cannot be empty");
        }
        if self.system_prompt.is_empty() {
            anyhow::bail!("agent '{}' system_prompt cannot be empty", self.id);
        }
        self.tool_filter = self
            .tool_filter
            .iter()
            .map(|item| item.trim().to_string())
            .filter(|item| !item.is_empty())
            .collect();
        Ok(())
    }

    pub fn into_agent(self, app_config: &Config) -> ConfiguredAgent {
        ConfiguredAgent {
            id: self.id,
            name: self.name,
            system_prompt: self.system_prompt,
            model: self.model.unwrap_or_else(|| app_config.model_name.clone()),
            api_base: self
                .api_base
                .unwrap_or_else(|| app_config.model_base_url.clone()),
            api_key: self
                .api_key
                .unwrap_or_else(|| app_config.model_api_key.clone()),
            tool_filter: if self.tool_filter.is_empty() {
                None
            } else {
                Some(self.tool_filter)
            },
        }
    }
}

pub struct ConfiguredAgent {
    id: String,
    name: String,
    system_prompt: String,
    model: String,
    api_base: String,
    api_key: String,
    tool_filter: Option<Vec<String>>,
}

#[async_trait]
impl AgentTrait for ConfiguredAgent {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
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

    fn tool_filter(&self) -> Option<Vec<String>> {
        self.tool_filter.clone()
    }
}

fn string_field(config: &Value, key: &str) -> Option<String> {
    config
        .get(key)
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn string_array_field(config: &Value, key: &str) -> Vec<String> {
    config
        .get(key)
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str())
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_inline_agent_definition() {
        let definition = AgentDefinition::from_node_config(
            "researcher",
            &serde_json::json!({
                "agent_definition": {
                    "id": "researcher",
                    "name": "Researcher",
                    "system_prompt": "Research carefully.",
                    "tool_filter": ["run_capability", " ", "run_system_task"]
                }
            }),
        )
        .unwrap()
        .unwrap();

        assert_eq!(definition.id, "researcher");
        assert_eq!(definition.name, "Researcher");
        assert_eq!(definition.system_prompt, "Research carefully.");
        assert_eq!(
            definition.tool_filter,
            vec!["run_capability", "run_system_task"]
        );
    }

    #[test]
    fn parses_top_level_agent_definition_fields() {
        let definition = AgentDefinition::from_node_config(
            "summarizer",
            &serde_json::json!({
                "name": "Summarizer",
                "system_prompt": "Summarize tersely.",
                "model": "test-model",
                "metadata": { "role": "summary" }
            }),
        )
        .unwrap()
        .unwrap();

        assert_eq!(definition.id, "summarizer");
        assert_eq!(definition.name, "Summarizer");
        assert_eq!(definition.model.as_deref(), Some("test-model"));
        assert_eq!(definition.metadata["role"], "summary");
    }

    #[test]
    fn missing_prompt_means_no_custom_definition() {
        let definition =
            AgentDefinition::from_node_config("researcher", &serde_json::json!({})).unwrap();
        assert!(definition.is_none());
    }
}
