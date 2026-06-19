use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowStatus {
    Draft,
    Published,
    Archived,
}

impl Default for WorkflowStatus {
    fn default() -> Self {
        Self::Draft
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowDefinition {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub status: WorkflowStatus,
    #[serde(default = "default_version")]
    pub version: i64,
    #[serde(default)]
    pub nodes: Vec<WorkflowNode>,
    #[serde(default)]
    pub edges: Vec<WorkflowEdge>,
    #[serde(default)]
    pub metadata: Value,
}

impl WorkflowDefinition {
    pub fn new_draft(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            description: String::new(),
            status: WorkflowStatus::Draft,
            version: default_version(),
            nodes: Vec::new(),
            edges: Vec::new(),
            metadata: Value::Null,
        }
    }

    pub fn dependency_summary(&self) -> WorkflowDependencySummary {
        WorkflowDependencySummary::from_definition(self)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkflowDependencySummary {
    pub agents: Vec<String>,
    pub skills: Vec<String>,
    pub mcp_tools: Vec<String>,
    pub condition_nodes: usize,
    pub human_approval_nodes: usize,
    pub output_nodes: usize,
}

impl WorkflowDependencySummary {
    pub fn from_definition(definition: &WorkflowDefinition) -> Self {
        let mut summary = Self::default();
        for node in &definition.nodes {
            match &node.kind {
                WorkflowNodeKind::Agent { agent_id } => {
                    push_unique(&mut summary.agents, agent_id.clone());
                }
                WorkflowNodeKind::Skill { skill_id } => {
                    push_unique(&mut summary.skills, skill_id.clone());
                }
                WorkflowNodeKind::McpTool {
                    server_name,
                    tool_name,
                } => {
                    push_unique(
                        &mut summary.mcp_tools,
                        format!("{}:{}", server_name, tool_name),
                    );
                }
                WorkflowNodeKind::Condition => summary.condition_nodes += 1,
                WorkflowNodeKind::HumanApproval => summary.human_approval_nodes += 1,
                WorkflowNodeKind::Output => summary.output_nodes += 1,
            }
        }
        summary.agents.sort();
        summary.skills.sort();
        summary.mcp_tools.sort();
        summary
    }

    pub fn has_external_dependencies(&self) -> bool {
        !self.agents.is_empty() || !self.skills.is_empty() || !self.mcp_tools.is_empty()
    }
}

fn push_unique(values: &mut Vec<String>, value: String) {
    if !values.iter().any(|existing| existing == &value) {
        values.push(value);
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowNode {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(flatten)]
    pub kind: WorkflowNodeKind,
    #[serde(default)]
    pub config: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkflowNodeKind {
    Agent {
        agent_id: String,
    },
    Skill {
        skill_id: String,
    },
    McpTool {
        server_name: String,
        tool_name: String,
    },
    Condition,
    HumanApproval,
    Output,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowEdge {
    pub id: String,
    pub from_node_id: String,
    pub to_node_id: String,
    #[serde(default)]
    pub condition: String,
}

fn default_version() -> i64 {
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workflow_definition_defaults_to_draft() {
        let definition: WorkflowDefinition = serde_json::from_str(
            r#"{
                "id": "workflow.research",
                "name": "Research Workflow"
            }"#,
        )
        .unwrap();

        assert_eq!(definition.id, "workflow.research");
        assert_eq!(definition.status, WorkflowStatus::Draft);
        assert_eq!(definition.version, 1);
        assert!(definition.nodes.is_empty());
        assert!(definition.edges.is_empty());
    }

    #[test]
    fn workflow_definition_round_trips_node_kinds() {
        let definition = WorkflowDefinition {
            id: "workflow.research".to_string(),
            name: "Research Workflow".to_string(),
            description: "Find and summarize sources".to_string(),
            status: WorkflowStatus::Draft,
            version: 1,
            nodes: vec![
                WorkflowNode {
                    id: "main".to_string(),
                    name: "MainAgent".to_string(),
                    kind: WorkflowNodeKind::Agent {
                        agent_id: "mainagent".to_string(),
                    },
                    config: serde_json::json!({"prompt": "research"}),
                },
                WorkflowNode {
                    id: "summary".to_string(),
                    name: "Summary".to_string(),
                    kind: WorkflowNodeKind::Output,
                    config: Value::Null,
                },
            ],
            edges: vec![WorkflowEdge {
                id: "edge.main.summary".to_string(),
                from_node_id: "main".to_string(),
                to_node_id: "summary".to_string(),
                condition: String::new(),
            }],
            metadata: serde_json::json!({"owner": "system"}),
        };

        let json = serde_json::to_string(&definition).unwrap();
        let parsed: WorkflowDefinition = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed, definition);
    }

    #[test]
    fn dependency_summary_deduplicates_and_counts_nodes() {
        let mut definition = WorkflowDefinition::new_draft("workflow.deps", "Deps");
        definition.nodes.push(WorkflowNode {
            id: "main".to_string(),
            name: "Main".to_string(),
            kind: WorkflowNodeKind::Agent {
                agent_id: "mainagent".to_string(),
            },
            config: Value::Null,
        });
        definition.nodes.push(WorkflowNode {
            id: "main2".to_string(),
            name: "Main Again".to_string(),
            kind: WorkflowNodeKind::Agent {
                agent_id: "mainagent".to_string(),
            },
            config: Value::Null,
        });
        definition.nodes.push(WorkflowNode {
            id: "skill".to_string(),
            name: "Skill".to_string(),
            kind: WorkflowNodeKind::Skill {
                skill_id: "system_cleaner".to_string(),
            },
            config: Value::Null,
        });
        definition.nodes.push(WorkflowNode {
            id: "mcp".to_string(),
            name: "MCP".to_string(),
            kind: WorkflowNodeKind::McpTool {
                server_name: "server".to_string(),
                tool_name: "tool".to_string(),
            },
            config: Value::Null,
        });
        definition.nodes.push(WorkflowNode {
            id: "condition".to_string(),
            name: "Condition".to_string(),
            kind: WorkflowNodeKind::Condition,
            config: Value::Null,
        });
        definition.nodes.push(WorkflowNode {
            id: "approval".to_string(),
            name: "Approval".to_string(),
            kind: WorkflowNodeKind::HumanApproval,
            config: Value::Null,
        });
        definition.nodes.push(WorkflowNode {
            id: "output".to_string(),
            name: "Output".to_string(),
            kind: WorkflowNodeKind::Output,
            config: Value::Null,
        });

        let summary = definition.dependency_summary();

        assert_eq!(summary.agents, vec!["mainagent"]);
        assert_eq!(summary.skills, vec!["system_cleaner"]);
        assert_eq!(summary.mcp_tools, vec!["server:tool"]);
        assert_eq!(summary.condition_nodes, 1);
        assert_eq!(summary.human_approval_nodes, 1);
        assert_eq!(summary.output_nodes, 1);
        assert!(summary.has_external_dependencies());
    }
}
