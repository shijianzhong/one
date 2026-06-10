use std::collections::HashMap;

use super::agent::{AgentTrait, AgentBuilder};
use crate::agents::core::Agent as CoreAgent;

/// Agent 注册表：管理所有 Agent 类型的注册和实例化。
///
/// 每个 Agent 类型在启动时注册一个 AgentBuilder，
/// 运行时通过 create() 创建具体的 Agent 实例。
pub struct AgentRegistry {
    builders: HashMap<String, Box<dyn AgentBuilder>>,
}

#[derive(Debug, Clone)]
pub struct AgentDescriptor {
    pub id: String,
    pub name: String,
}

impl AgentRegistry {
    pub fn new() -> Self {
        Self {
            builders: HashMap::new(),
        }
    }

    /// 注册一个 Agent 类型
    pub fn register(&mut self, builder: Box<dyn AgentBuilder>) {
        let id = builder.agent_id().to_string();
        self.builders.insert(id, builder);
    }

    /// 创建指定类型的 Agent 实例
    pub fn create(
        &self,
        id: &str,
        config: &crate::services::Config,
        workspace: &str,
    ) -> Option<Box<dyn AgentTrait>> {
        self.builders
            .get(id)
            .map(|builder| builder.build(config, workspace))
    }

    /// 获取所有已注册的 Agent 描述
    pub fn all_agents(&self) -> Vec<AgentDescriptor> {
        self.builders
            .values()
            .map(|b| AgentDescriptor {
                id: b.agent_id().to_string(),
                name: b.agent_name().to_string(),
            })
            .collect()
    }

    /// 检查指定 Agent 是否已注册
    pub fn has_agent(&self, id: &str) -> bool {
        self.builders.contains_key(id)
    }

    /// 注册默认的 MainAgent
    pub fn register_default(&mut self) {
        self.register(Box::new(crate::agents::core::MainAgentBuilder));
    }
}

impl Default for AgentRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全局单例 ────────────────────────────────────────────────────────────────────

use std::sync::OnceLock;

static AGENT_REGISTRY: OnceLock<std::sync::Mutex<AgentRegistry>> = OnceLock::new();

pub fn agent_registry() -> &'static std::sync::Mutex<AgentRegistry> {
    AGENT_REGISTRY.get_or_init(|| std::sync::Mutex::new(AgentRegistry::new()))
}

/// 初始化 AgentRegistry（注册默认 Agent）
pub fn init_agent_registry() {
    let mut registry = agent_registry().lock().expect("AgentRegistry lock");
    registry.register_default();
}