#![allow(dead_code)]

use anyhow::Result;
use async_trait::async_trait;
use std::sync::{Arc, Mutex};

use super::{
    Agent, AgentConfig, AgentInstance, AgentStatus, BusinessAgentConfig, BusinessCapability,
};
use crate::memory::types::ChatMessage;
use crate::task_db;

pub struct BusinessAgent {
    pub id: usize,
    pub name: String,
    pub description: String,
    pub capabilities: Vec<BusinessCapability>,
    pub tools: Vec<String>,
    conversation_history: Arc<Mutex<Vec<ChatMessage>>>,
}

impl BusinessAgent {
    pub fn new(config: BusinessAgentConfig) -> Self {
        Self {
            id: 0,
            name: config.name,
            description: config.description,
            capabilities: config.capabilities,
            tools: config.tools,
            conversation_history: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn match_capability(&self, query: &str) -> Option<&BusinessCapability> {
        let query_lower = query.to_lowercase();
        for cap in &self.capabilities {
            for trigger in &cap.trigger_queries {
                if query_lower.contains(&trigger.to_lowercase()) {
                    return Some(cap);
                }
            }
        }
        None
    }

    pub fn generate_response(&self, capability: &BusinessCapability, query: &str) -> String {
        let mut response = capability.response_template.clone();
        response = response.replace("{query}", query);
        response
    }

    pub fn get_follow_up(&self, capability: &BusinessCapability) -> Vec<String> {
        capability.follow_up_questions.clone()
    }

    pub fn build_context(&self) -> String {
        let mut context = format!("Agent: {}\nDescription: {}\n", self.name, self.description);
        context.push_str("Capabilities:\n");
        for cap in &self.capabilities {
            context.push_str(&format!("- {}: {}\n", cap.name, cap.description));
        }
        context
    }
}

#[async_trait]
impl Agent for BusinessAgent {
    fn agent_type(&self) -> &str {
        "business"
    }

    fn agent_name(&self) -> &str {
        &self.name
    }

    async fn spawn(&self, _config: AgentConfig) -> Result<AgentInstance> {
        let instance = AgentInstance {
            id: 0,
            agent_id: self.id,
            task_id: None,
            status: AgentStatus::Idle,
            session_state: serde_json::json!({
                "name": self.name,
                "description": self.description,
            }),
        };
        Ok(instance)
    }

    async fn send_message(&self, instance: &mut AgentInstance, msg: &str) -> Result<String> {
        instance.status = AgentStatus::Running;

        {
            let mut history = self.conversation_history.lock().unwrap();
            history.push(ChatMessage::new("user", msg));
        }

        let response = if let Some(cap) = self.match_capability(msg) {
            self.generate_response(cap, msg)
        } else {
            format!("I understand you want help with: {}. Could you please provide more details about what you need?", msg)
        };

        {
            let mut history = self.conversation_history.lock().unwrap();
            history.push(ChatMessage::new("assistant", &response));
        }

        instance.status = AgentStatus::Idle;
        Ok(response)
    }

    async fn get_status(&self, _instance: &AgentInstance) -> AgentStatus {
        AgentStatus::Idle
    }

    async fn pause(&self, instance: &mut AgentInstance) -> Result<()> {
        instance.status = AgentStatus::Paused;
        Ok(())
    }

    async fn resume(&self, instance: &mut AgentInstance) -> Result<()> {
        instance.status = AgentStatus::Running;
        Ok(())
    }

    async fn destroy(&self, instance: &mut AgentInstance) -> Result<()> {
        instance.status = AgentStatus::Terminated;
        Ok(())
    }
}

pub struct BusinessAgentGenerator {
    db: Arc<task_db::Database>,
}

impl BusinessAgentGenerator {
    pub fn new(db: Arc<task_db::Database>) -> Self {
        Self { db }
    }

    pub async fn create_agent_from_conversation(
        &self,
        name: &str,
        description: &str,
        capabilities: Vec<BusinessCapability>,
        _tools: Vec<String>,
    ) -> Result<usize> {
        let capabilities_json = serde_json::to_string(&capabilities)?;

        let agent_id = task_db::insert_agent(
            &self.db.conn,
            name,
            "business",
            Some(description),
            Some(&capabilities_json),
            None,
        )?;

        for cap in capabilities {
            task_db::insert_agent_capability(
                &self.db.conn,
                agent_id,
                &cap.name,
                Some(&cap.response_template),
                None,
            )?;
        }

        Ok(agent_id)
    }

    pub async fn generate_agent_spec(
        &self,
        conversation: &[ChatMessage],
        base_url: &str,
        api_key: &str,
        model: &str,
    ) -> Result<BusinessAgentConfig> {
        // Use LLM to analyze conversation and generate agent spec
        let conversation_str = conversation
            .iter()
            .map(|m| format!("{}: {}", m.role, m.content))
            .collect::<Vec<_>>()
            .join("\n");

        let prompt = format!(
            "请分析以下对话，生成一个业务智能体的配置。要求用中文输出JSON格式，包含：name(智能体名称), description(描述), capabilities(能力列表，每个能力包含name、description、trigger_queries触发关键词数组、response_template回复模板、follow_up_questions跟进问题数组), tools(工具列表)。\n对话内容：\n{}\n\n只返回JSON，不要其他内容：",
            conversation_str
        );

        let result = crate::services::api::call_chat_api_sync(
            base_url,
            api_key,
            model,
            &[ChatMessage::new("user", &prompt)],
        )
        .map_err(|e| anyhow::anyhow!("API call failed: {}", e))?;

        let spec: BusinessAgentConfig = serde_json::from_str(&result)
            .map_err(|e| anyhow::anyhow!("Failed to parse agent config: {}", e))?;

        Ok(spec)
    }
}
