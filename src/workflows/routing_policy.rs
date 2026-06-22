use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::definition::WorkflowDefinition;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoutingMode {
    Sequential,
    Parallel,
    Selector,
    Handoff,
    Graph,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivationCondition {
    All,
    Any,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutingPolicy {
    pub mode: RoutingMode,
    #[serde(default)]
    pub activation: Option<ActivationCondition>,
    #[serde(default)]
    pub selector_candidates: Vec<String>,
    #[serde(default)]
    pub handoff_targets: Vec<String>,
    #[serde(default)]
    pub max_loops: Option<u32>,
    #[serde(default)]
    pub termination: Option<String>,
}

impl RoutingPolicy {
    pub fn sequential() -> Self {
        Self {
            mode: RoutingMode::Sequential,
            activation: None,
            selector_candidates: Vec::new(),
            handoff_targets: Vec::new(),
            max_loops: None,
            termination: None,
        }
    }

    pub fn from_value(value: &Value) -> anyhow::Result<Self> {
        if value.is_null() {
            return Ok(Self::sequential());
        }
        serde_json::from_value(value.clone())
            .map_err(|err| anyhow::anyhow!("invalid routing policy: {err}"))
    }

    pub fn validate(&self, definition: Option<&WorkflowDefinition>) -> anyhow::Result<()> {
        match self.mode {
            RoutingMode::Sequential | RoutingMode::Graph => {}
            RoutingMode::Parallel => {
                if !matches!(
                    self.activation,
                    Some(ActivationCondition::All | ActivationCondition::Any)
                ) {
                    anyhow::bail!("parallel routing requires activation all or any");
                }
            }
            RoutingMode::Selector => {
                if self.selector_candidates.is_empty() {
                    anyhow::bail!("selector routing requires selector_candidates");
                }
            }
            RoutingMode::Handoff => {
                if self.handoff_targets.is_empty() {
                    anyhow::bail!("handoff routing requires handoff_targets");
                }
            }
        }

        if let Some(max_loops) = self.max_loops {
            if max_loops == 0 {
                anyhow::bail!("max_loops must be greater than zero");
            }
        }

        if let Some(definition) = definition {
            for node_id in self
                .selector_candidates
                .iter()
                .chain(self.handoff_targets.iter())
            {
                if !definition.nodes.iter().any(|node| node.id == *node_id) {
                    anyhow::bail!("routing target node '{}' not found", node_id);
                }
            }
        }

        Ok(())
    }
}

pub fn validate_definition_routing(definition: &WorkflowDefinition) -> anyhow::Result<()> {
    let workflow_policy = definition
        .metadata
        .get("routing")
        .map(RoutingPolicy::from_value)
        .transpose()?
        .unwrap_or_else(RoutingPolicy::sequential);
    workflow_policy.validate(Some(definition))?;

    for node in &definition.nodes {
        if let Some(value) = node.config.get("routing") {
            RoutingPolicy::from_value(value)?.validate(Some(definition))?;
        }
    }

    if let Some(edge_routing) = definition
        .metadata
        .get("edge_routing")
        .and_then(|value| value.as_object())
    {
        for edge in &definition.edges {
            if let Some(value) = edge_routing.get(&edge.id) {
                RoutingPolicy::from_value(value)?.validate(Some(definition))?;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflows::{WorkflowNode, WorkflowNodeKind};

    #[test]
    fn routing_policy_parses_selector() {
        let policy = RoutingPolicy::from_value(&serde_json::json!({
            "mode": "selector",
            "selector_candidates": ["a", "b"],
            "max_loops": 3,
            "termination": "done"
        }))
        .unwrap();

        assert_eq!(policy.mode, RoutingMode::Selector);
        assert_eq!(policy.selector_candidates, vec!["a", "b"]);
        assert_eq!(policy.max_loops, Some(3));
    }

    #[test]
    fn routing_policy_validation_rejects_missing_selector_candidates() {
        let policy = RoutingPolicy::from_value(&serde_json::json!({
            "mode": "selector"
        }))
        .unwrap();

        assert!(policy.validate(None).is_err());
    }

    #[test]
    fn edge_routing_validation_checks_targets() {
        let mut definition = WorkflowDefinition::new_draft("workflow.test", "Test");
        definition.nodes.push(WorkflowNode {
            id: "a".to_string(),
            name: "A".to_string(),
            kind: WorkflowNodeKind::Agent {
                agent_id: "local:a".to_string(),
            },
            config: Value::Null,
        });
        definition.metadata = serde_json::json!({
            "routing": {"mode": "sequential"},
            "edge_routing": {
                "missing_edge": {"mode": "handoff", "handoff_targets": ["missing"]}
            }
        });

        validate_definition_routing(&definition).unwrap();

        definition.edges.push(crate::workflows::WorkflowEdge {
            id: "missing_edge".to_string(),
            from_node_id: "a".to_string(),
            to_node_id: "a".to_string(),
            condition: "always".to_string(),
        });

        assert!(validate_definition_routing(&definition).is_err());
    }
}
