use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{WorkflowDefinition, WorkflowEdge, WorkflowNode, WorkflowNodeKind};

#[derive(Debug, Clone, Default)]
pub struct WorkflowCopilotContext {
    pub available_skills: Vec<String>,
    pub available_mcp_tools: Vec<String>,
    pub available_system_tools: Vec<String>,
    pub available_coding_runtimes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowDesignerDraft {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub routing: Value,
    #[serde(default)]
    pub agents: Vec<WorkflowDesignerAgent>,
    #[serde(default)]
    pub edges: Vec<WorkflowDesignerEdge>,
    #[serde(default)]
    pub output_schema: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowDesignerAgent {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub system_prompt: String,
    #[serde(default)]
    pub instructions: String,
    #[serde(default)]
    pub skills: Vec<String>,
    #[serde(default)]
    pub mcp_tools: Vec<String>,
    #[serde(default)]
    pub system_tools: Vec<String>,
    #[serde(default)]
    pub coding_runtimes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowDesignerEdge {
    pub from: String,
    pub to: String,
    #[serde(default = "default_edge_condition")]
    pub condition: String,
}

pub async fn design_workflow_from_brief(
    brief: &str,
    context: WorkflowCopilotContext,
) -> Result<WorkflowDefinition> {
    let brief = brief.trim();
    if brief.is_empty() {
        anyhow::bail!("workflow brief cannot be empty");
    }

    let config = crate::services::config::load_config();
    if !config.model_api_key.trim().is_empty() {
        match design_with_model(brief, &context, &config).await {
            Ok(definition) => return Ok(definition),
            Err(err) => {
                log::warn!("[WorkflowCopilot] model workflow design failed: {}", err);
            }
        }
    }

    workflow_from_designer_draft(&deterministic_designer_draft(brief, &context))
}

pub fn workflow_from_designer_draft(draft: &WorkflowDesignerDraft) -> Result<WorkflowDefinition> {
    validate_designer_draft(draft)?;

    let mut definition = WorkflowDefinition::new_draft(
        format!(
            "workflow.copilot.{}",
            chrono::Local::now().timestamp_millis()
        ),
        draft.name.trim(),
    );
    definition.description = draft.description.trim().to_string();
    definition.metadata = if draft.routing.is_null() {
        serde_json::json!({ "routing": { "mode": "sequential" } })
    } else {
        serde_json::json!({ "routing": draft.routing.clone() })
    };

    for agent in &draft.agents {
        definition.nodes.push(WorkflowNode {
            id: sanitize_node_id(&agent.id),
            name: agent.name.trim().to_string(),
            kind: WorkflowNodeKind::Agent {
                agent_id: format!("local:{}", sanitize_node_id(&agent.id)),
            },
            config: local_agent_config(agent),
        });
    }

    definition.nodes.push(WorkflowNode {
        id: "output".to_string(),
        name: "Output".to_string(),
        kind: WorkflowNodeKind::Output,
        config: if draft.output_schema.is_null() {
            serde_json::Value::Null
        } else {
            serde_json::json!({
                "schema": draft.output_schema,
            })
        },
    });

    if draft.edges.is_empty() {
        for pair in draft.agents.windows(2) {
            let from = sanitize_node_id(&pair[0].id);
            let to = sanitize_node_id(&pair[1].id);
            definition.edges.push(WorkflowEdge {
                id: format!("{}_to_{}", from, to),
                from_node_id: from,
                to_node_id: to,
                condition: "always".to_string(),
            });
        }
        if let Some(last_agent) = draft.agents.last() {
            let from = sanitize_node_id(&last_agent.id);
            definition.edges.push(WorkflowEdge {
                id: format!("{}_to_output", from),
                from_node_id: from,
                to_node_id: "output".to_string(),
                condition: "always".to_string(),
            });
        }
    } else {
        for edge in &draft.edges {
            definition.edges.push(WorkflowEdge {
                id: format!(
                    "{}_to_{}",
                    sanitize_node_id(&edge.from),
                    sanitize_node_id(&edge.to)
                ),
                from_node_id: sanitize_node_id(&edge.from),
                to_node_id: sanitize_node_id(&edge.to),
                condition: edge.condition.trim().to_string(),
            });
        }
        let has_output_edge = definition
            .edges
            .iter()
            .any(|edge| edge.to_node_id == "output");
        if !has_output_edge {
            let from = sanitize_node_id(&draft.agents.last().expect("validated").id);
            definition.edges.push(WorkflowEdge {
                id: format!("{}_to_output", from),
                from_node_id: from,
                to_node_id: "output".to_string(),
                condition: "always".to_string(),
            });
        }
    }

    super::validate_definition_routing(&definition)?;
    Ok(definition)
}

pub fn validate_designer_draft(draft: &WorkflowDesignerDraft) -> Result<()> {
    if draft.name.trim().is_empty() {
        anyhow::bail!("designer draft requires name");
    }
    if draft.agents.is_empty() {
        anyhow::bail!("designer draft requires at least one agent");
    }

    let mut ids = std::collections::HashSet::new();
    for agent in &draft.agents {
        let id = sanitize_node_id(&agent.id);
        if id.is_empty() {
            anyhow::bail!("designer agent requires id");
        }
        if !ids.insert(id.clone()) {
            anyhow::bail!("duplicate designer agent id '{}'", id);
        }
        if agent.name.trim().is_empty() {
            anyhow::bail!("designer agent '{}' requires name", id);
        }
        if agent.system_prompt.trim().is_empty() && agent.instructions.trim().is_empty() {
            anyhow::bail!("designer agent '{}' requires prompt", id);
        }
    }

    for edge in &draft.edges {
        let from = sanitize_node_id(&edge.from);
        let to = sanitize_node_id(&edge.to);
        if !ids.contains(&from) {
            anyhow::bail!("designer edge references missing source '{}'", from);
        }
        if to != "output" && !ids.contains(&to) {
            anyhow::bail!("designer edge references missing target '{}'", to);
        }
    }

    Ok(())
}

async fn design_with_model(
    brief: &str,
    context: &WorkflowCopilotContext,
    config: &crate::services::config::Config,
) -> Result<WorkflowDefinition> {
    let model = config
        .system_model
        .as_deref()
        .or(config.light_model.as_deref())
        .unwrap_or(&config.model_name);
    let system_prompt = workflow_designer_system_prompt(context);
    let user_prompt = format!("用户需求：\n{}\n\n只返回 JSON，不要 Markdown。", brief);
    let messages = vec![
        crate::memory::types::ChatMessage::new("system", &system_prompt),
        crate::memory::types::ChatMessage::new("user", &user_prompt),
    ];

    let raw = crate::services::api::call_chat_api_once(
        &config.model_base_url,
        &config.model_api_key,
        model,
        &messages,
    )
    .await
    .map_err(|err| anyhow::anyhow!(err))?;

    match parse_designer_draft(&raw).and_then(|draft| workflow_from_designer_draft(&draft)) {
        Ok(definition) => Ok(definition),
        Err(first_err) => {
            let repair_prompt = format!(
                "上一次输出无法通过校验：{}\n\n原始输出：\n{}\n\n请修复并只返回符合 schema 的 JSON。",
                first_err, raw
            );
            let repair_messages = vec![
                crate::memory::types::ChatMessage::new("system", &system_prompt),
                crate::memory::types::ChatMessage::new("user", &repair_prompt),
            ];
            let repaired = crate::services::api::call_chat_api_once(
                &config.model_base_url,
                &config.model_api_key,
                model,
                &repair_messages,
            )
            .await
            .map_err(|err| anyhow::anyhow!(err))?;
            let draft = parse_designer_draft(&repaired)
                .with_context(|| format!("model repair failed after: {}", first_err))?;
            workflow_from_designer_draft(&draft)
        }
    }
}

fn workflow_designer_system_prompt(context: &WorkflowCopilotContext) -> String {
    format!(
        "你是 WorkflowDesignerAgent。根据用户需求设计多 Agent 工作流草稿，只输出 JSON。\n\
         JSON schema: {{\"name\":string,\"description\":string,\"routing\":object,\"agents\":[{{\"id\":string,\"name\":string,\"description\":string,\"system_prompt\":string,\"instructions\":string,\"skills\":string[],\"mcp_tools\":string[],\"system_tools\":string[],\"coding_runtimes\":string[]}}],\"edges\":[{{\"from\":string,\"to\":string,\"condition\":string}}],\"output_schema\":object|null}}\n\
         规则：至少 1 个 agent；agent id 使用 snake_case；edges 只能引用 agent id 或 output；routing 第一阶段优先 sequential 或 graph；不要发布，只生成 draft。\n\
         可用 skills: {}\n可用 MCP tools: {}\n可用 system tools: {}\n可用 coding runtimes: {}",
        context.available_skills.join(", "),
        context.available_mcp_tools.join(", "),
        context.available_system_tools.join(", "),
        context.available_coding_runtimes.join(", ")
    )
}

fn parse_designer_draft(raw: &str) -> Result<WorkflowDesignerDraft> {
    let json = extract_json_object(raw).unwrap_or_else(|| raw.trim().to_string());
    let draft: WorkflowDesignerDraft =
        serde_json::from_str(&json).with_context(|| "failed to parse workflow designer JSON")?;
    validate_designer_draft(&draft)?;
    Ok(draft)
}

fn deterministic_designer_draft(
    brief: &str,
    context: &WorkflowCopilotContext,
) -> WorkflowDesignerDraft {
    let uses_code = contains_any(brief, &["代码", "应用", "开发", "app", "code", "website"]);
    let mut agents = vec![
        WorkflowDesignerAgent {
            id: "planner".to_string(),
            name: "需求分析 Agent".to_string(),
            description: "拆解用户目标、边界和验收标准。".to_string(),
            system_prompt: "你是需求分析 Agent，负责把用户目标拆解为可执行计划。".to_string(),
            instructions: format!("围绕以下需求输出任务拆解、风险和验收标准：{}", brief),
            skills: Vec::new(),
            mcp_tools: Vec::new(),
            system_tools: Vec::new(),
            coding_runtimes: Vec::new(),
        },
        WorkflowDesignerAgent {
            id: if uses_code { "builder" } else { "worker" }.to_string(),
            name: if uses_code {
                "实现 Agent".to_string()
            } else {
                "执行 Agent".to_string()
            },
            description: "根据计划执行主体工作。".to_string(),
            system_prompt: "你是执行 Agent，负责基于计划产出可交付结果。".to_string(),
            instructions: format!("根据上游计划完成用户需求：{}", brief),
            skills: context.available_skills.iter().take(3).cloned().collect(),
            mcp_tools: context
                .available_mcp_tools
                .iter()
                .take(2)
                .cloned()
                .collect(),
            system_tools: Vec::new(),
            coding_runtimes: if uses_code {
                context
                    .available_coding_runtimes
                    .iter()
                    .take(1)
                    .cloned()
                    .collect()
            } else {
                Vec::new()
            },
        },
        WorkflowDesignerAgent {
            id: "reviewer".to_string(),
            name: "审查 Agent".to_string(),
            description: "审查输出质量并形成最终总结。".to_string(),
            system_prompt: "你是审查 Agent，负责检查结果是否满足需求并给出最终输出。".to_string(),
            instructions: "检查上游结果的完整性、一致性和风险，输出最终可交付结果。".to_string(),
            skills: Vec::new(),
            mcp_tools: Vec::new(),
            system_tools: Vec::new(),
            coding_runtimes: Vec::new(),
        },
    ];
    if !uses_code && brief.chars().count() < 18 {
        agents.remove(1);
    }

    WorkflowDesignerDraft {
        name: brief_to_workflow_name(brief),
        description: format!("AI Copilot generated draft for: {}", brief),
        routing: serde_json::json!({ "mode": "sequential" }),
        agents,
        edges: Vec::new(),
        output_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "summary": { "type": "string" },
                "artifacts": { "type": "array", "items": { "type": "string" } },
                "risks": { "type": "array", "items": { "type": "string" } }
            },
            "required": ["summary"]
        }),
    }
}

fn local_agent_config(agent: &WorkflowDesignerAgent) -> Value {
    serde_json::json!({
        "description": agent.description,
        "model": {
            "provider": "default",
            "model": "default",
            "temperature": 0.2,
            "max_tokens": 4096,
            "timeout_seconds": 120
        },
        "prompt": {
            "system": agent.system_prompt,
            "instructions": agent.instructions,
            "context_rules": []
        },
        "tools": {
            "skills": agent.skills,
            "mcp_tools": agent.mcp_tools,
            "system_tools": agent.system_tools,
            "coding_runtimes": agent.coding_runtimes
        },
        "output": {
            "schema": null,
            "format": "text",
            "summarize_with_mainagent": true
        },
        "settings": {
            "retry": 0,
            "timeout_seconds": 120,
            "human_confirmation": false,
            "permissions": "ask"
        },
        "routing": {
            "mode": "sequential"
        }
    })
}

fn sanitize_node_id(raw: &str) -> String {
    let mut id = raw
        .trim()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>();
    while id.contains("__") {
        id = id.replace("__", "_");
    }
    id.trim_matches('_').to_string()
}

fn brief_to_workflow_name(brief: &str) -> String {
    let mut name = brief.trim().chars().take(24).collect::<String>();
    if name.trim().is_empty() {
        name = "AI Copilot Workflow".to_string();
    }
    format!("{} Workflow", name.trim())
}

fn extract_json_object(raw: &str) -> Option<String> {
    let start = raw.find('{')?;
    let end = raw.rfind('}')?;
    (end > start).then(|| raw[start..=end].to_string())
}

fn contains_any(text: &str, needles: &[&str]) -> bool {
    let lower = text.to_lowercase();
    needles.iter().any(|needle| lower.contains(needle))
}

fn default_edge_condition() -> String {
    "always".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_designer_draft_schema() {
        let draft = WorkflowDesignerDraft {
            name: "Demo".to_string(),
            description: String::new(),
            routing: serde_json::json!({ "mode": "sequential" }),
            agents: vec![WorkflowDesignerAgent {
                id: "planner".to_string(),
                name: "Planner".to_string(),
                description: String::new(),
                system_prompt: "Plan".to_string(),
                instructions: String::new(),
                skills: Vec::new(),
                mcp_tools: Vec::new(),
                system_tools: Vec::new(),
                coding_runtimes: Vec::new(),
            }],
            edges: Vec::new(),
            output_schema: serde_json::Value::Null,
        };

        validate_designer_draft(&draft).unwrap();
    }

    #[test]
    fn rejects_invalid_designer_draft_schema() {
        let draft = WorkflowDesignerDraft {
            name: "Demo".to_string(),
            description: String::new(),
            routing: serde_json::json!({ "mode": "sequential" }),
            agents: Vec::new(),
            edges: Vec::new(),
            output_schema: serde_json::Value::Null,
        };

        let err = validate_designer_draft(&draft).unwrap_err();

        assert!(err.to_string().contains("at least one agent"));
    }

    #[test]
    fn imports_designer_draft_as_workflow_draft() {
        let definition = workflow_from_designer_draft(&deterministic_designer_draft(
            "帮我开发一个会员管理系统应用",
            &WorkflowCopilotContext {
                available_coding_runtimes: vec!["claude".to_string()],
                ..WorkflowCopilotContext::default()
            },
        ))
        .unwrap();

        assert!(definition.id.starts_with("workflow.copilot."));
        assert_eq!(definition.status, super::super::WorkflowStatus::Draft);
        assert!(definition.nodes.len() >= 2);
        assert!(definition
            .nodes
            .iter()
            .any(|node| matches!(node.kind, WorkflowNodeKind::Output)));
        assert!(definition
            .nodes
            .iter()
            .any(|node| node.config["tools"]["coding_runtimes"][0] == "claude"));
    }
}
