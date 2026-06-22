use std::collections::{HashMap, HashSet};

use super::{validate_definition_routing, RoutingPolicy, WorkflowDefinition, WorkflowNodeKind};

pub fn validate_publish_ready(definition: &WorkflowDefinition) -> anyhow::Result<()> {
    validate_definition_routing(definition)?;
    if definition.nodes.is_empty() {
        anyhow::bail!("workflow must contain at least one node before publishing");
    }
    if !has_output_boundary(definition) {
        anyhow::bail!("workflow must contain an output node or terminal sink before publishing");
    }
    validate_local_agents(definition)?;
    validate_selector_publish_readiness(definition)?;
    validate_graph_cycles(definition)?;
    Ok(())
}

fn has_output_boundary(definition: &WorkflowDefinition) -> bool {
    if definition
        .nodes
        .iter()
        .any(|node| matches!(node.kind, WorkflowNodeKind::Output))
    {
        return true;
    }
    let outgoing = definition
        .edges
        .iter()
        .map(|edge| edge.from_node_id.as_str())
        .collect::<HashSet<_>>();
    definition
        .nodes
        .iter()
        .any(|node| !outgoing.contains(node.id.as_str()))
}

fn validate_local_agents(definition: &WorkflowDefinition) -> anyhow::Result<()> {
    for node in &definition.nodes {
        let WorkflowNodeKind::Agent { agent_id } = &node.kind else {
            continue;
        };
        if !agent_id.starts_with("local:") {
            continue;
        }
        if node.name.trim().is_empty() {
            anyhow::bail!("local Agent node '{}' requires a name", node.id);
        }
        let model = node
            .config
            .get("model")
            .and_then(|value| value.as_object())
            .ok_or_else(|| {
                anyhow::anyhow!("local Agent node '{}' requires model config", node.id)
            })?;
        let provider = model
            .get("provider")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .trim();
        let model_name = model
            .get("model")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .trim();
        if provider.is_empty() || model_name.is_empty() {
            anyhow::bail!("local Agent node '{}' requires provider and model", node.id);
        }
        let prompt = node
            .config
            .get("prompt")
            .and_then(|value| value.as_object());
        let has_prompt = prompt
            .and_then(|prompt| {
                prompt
                    .get("system")
                    .or_else(|| prompt.get("instructions"))
                    .and_then(|value| value.as_str())
            })
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false);
        if !has_prompt {
            anyhow::bail!(
                "local Agent node '{}' requires system or instructions prompt",
                node.id
            );
        }
    }
    Ok(())
}

fn validate_selector_publish_readiness(definition: &WorkflowDefinition) -> anyhow::Result<()> {
    if let Some(routing) = definition.metadata.get("routing") {
        validate_selector_policy_value("workflow", routing)?;
    }
    for node in &definition.nodes {
        if let Some(routing) = node.config.get("routing") {
            validate_selector_policy_value(&format!("node '{}'", node.id), routing)?;
        }
    }
    if let Some(edge_routing) = definition
        .metadata
        .get("edge_routing")
        .and_then(|value| value.as_object())
    {
        for (edge_id, routing) in edge_routing {
            validate_selector_policy_value(&format!("edge '{}'", edge_id), routing)?;
        }
    }
    Ok(())
}

fn validate_selector_policy_value(scope: &str, value: &serde_json::Value) -> anyhow::Result<()> {
    let mode = value
        .get("mode")
        .and_then(|mode| mode.as_str())
        .unwrap_or_default();
    if mode != "selector" {
        return Ok(());
    }
    if selector_has_usable_model(value) || selector_has_deterministic_fallback(value) {
        return Ok(());
    }
    anyhow::bail!(
        "{} selector routing requires a selector model or deterministic fallback before publishing",
        scope
    )
}

fn selector_has_usable_model(value: &serde_json::Value) -> bool {
    has_model_config(value.get("selector_model"))
        || has_model_config(value.get("model"))
        || value
            .get("selector")
            .map(|selector| {
                let selector_type = selector
                    .get("type")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default();
                selector_type == "model"
                    && (has_model_config(selector.get("model"))
                        || has_model_config(selector.get("selector_model")))
            })
            .unwrap_or(false)
}

fn has_model_config(value: Option<&serde_json::Value>) -> bool {
    match value {
        Some(serde_json::Value::String(model)) => !model.trim().is_empty(),
        Some(serde_json::Value::Object(model)) => model
            .get("model")
            .or_else(|| model.get("name"))
            .and_then(|value| value.as_str())
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false),
        _ => false,
    }
}

fn selector_has_deterministic_fallback(value: &serde_json::Value) -> bool {
    let fallback = value
        .get("fallback")
        .or_else(|| value.get("fallback_node_id"))
        .or_else(|| value.get("default_candidate"))
        .and_then(|value| value.as_str())
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false);
    if fallback {
        return true;
    }

    value
        .get("selector")
        .map(|selector| {
            let selector_type = selector
                .get("type")
                .and_then(|value| value.as_str())
                .unwrap_or_default();
            matches!(selector_type, "rule" | "deterministic" | "fallback")
                && (selector
                    .get("fallback")
                    .or_else(|| selector.get("fallback_node_id"))
                    .or_else(|| selector.get("default_candidate"))
                    .and_then(|value| value.as_str())
                    .map(|value| !value.trim().is_empty())
                    .unwrap_or(false)
                    || selector
                        .get("strategy")
                        .and_then(|value| value.as_str())
                        .map(|value| matches!(value, "first_available" | "round_robin" | "ordered"))
                        .unwrap_or(false))
        })
        .unwrap_or(false)
}

fn validate_graph_cycles(definition: &WorkflowDefinition) -> anyhow::Result<()> {
    if definition.edges.is_empty() || allows_bounded_or_terminated_loop(definition)? {
        return Ok(());
    }

    let mut graph: HashMap<&str, Vec<&str>> = HashMap::new();
    for edge in &definition.edges {
        graph
            .entry(edge.from_node_id.as_str())
            .or_default()
            .push(edge.to_node_id.as_str());
    }

    let mut visiting = HashSet::new();
    let mut visited = HashSet::new();
    for node in &definition.nodes {
        if has_cycle(node.id.as_str(), &graph, &mut visiting, &mut visited) {
            anyhow::bail!(
                "workflow graph contains a cycle; configure max_steps, routing.max_loops, or termination_condition before publishing"
            );
        }
    }
    Ok(())
}

fn allows_bounded_or_terminated_loop(definition: &WorkflowDefinition) -> anyhow::Result<bool> {
    if definition
        .metadata
        .get("max_steps")
        .and_then(|value| value.as_u64())
        .is_some()
    {
        return Ok(true);
    }
    if definition
        .metadata
        .get("termination_condition")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .is_some()
    {
        return Ok(true);
    }
    let policy = definition
        .metadata
        .get("routing")
        .map(RoutingPolicy::from_value)
        .transpose()?
        .unwrap_or_else(RoutingPolicy::sequential);
    Ok(policy.max_loops.is_some() || policy.termination.is_some())
}

fn has_cycle<'a>(
    node_id: &'a str,
    graph: &HashMap<&'a str, Vec<&'a str>>,
    visiting: &mut HashSet<&'a str>,
    visited: &mut HashSet<&'a str>,
) -> bool {
    if visited.contains(node_id) {
        return false;
    }
    if !visiting.insert(node_id) {
        return true;
    }
    for next in graph.get(node_id).into_iter().flatten() {
        if has_cycle(next, graph, visiting, visited) {
            return true;
        }
    }
    visiting.remove(node_id);
    visited.insert(node_id);
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflows::{WorkflowEdge, WorkflowNode};

    fn output_node(id: &str) -> WorkflowNode {
        WorkflowNode {
            id: id.to_string(),
            name: "Output".to_string(),
            kind: WorkflowNodeKind::Output,
            config: serde_json::json!({ "value": { "ok": true } }),
        }
    }

    #[test]
    fn publish_validation_rejects_empty_workflow() {
        let definition = WorkflowDefinition::new_draft("workflow.empty", "Empty");

        let err = validate_publish_ready(&definition).unwrap_err();

        assert!(err.to_string().contains("at least one node"));
    }

    #[test]
    fn publish_validation_accepts_output_workflow() {
        let mut definition = WorkflowDefinition::new_draft("workflow.output", "Output");
        definition.nodes.push(output_node("output"));

        validate_publish_ready(&definition).unwrap();
    }

    #[test]
    fn publish_validation_rejects_unbounded_cycle() {
        let mut definition = WorkflowDefinition::new_draft("workflow.loop", "Loop");
        definition.nodes.push(output_node("a"));
        definition.nodes.push(output_node("b"));
        definition.edges.push(WorkflowEdge {
            id: "a_to_b".to_string(),
            from_node_id: "a".to_string(),
            to_node_id: "b".to_string(),
            condition: "always".to_string(),
        });
        definition.edges.push(WorkflowEdge {
            id: "b_to_a".to_string(),
            from_node_id: "b".to_string(),
            to_node_id: "a".to_string(),
            condition: "always".to_string(),
        });

        let err = validate_publish_ready(&definition).unwrap_err();

        assert!(err.to_string().contains("cycle"));
    }

    #[test]
    fn publish_validation_allows_bounded_cycle() {
        let mut definition = WorkflowDefinition::new_draft("workflow.loop", "Loop");
        definition.metadata = serde_json::json!({ "max_steps": 4 });
        definition.nodes.push(output_node("a"));
        definition.nodes.push(output_node("b"));
        definition.edges.push(WorkflowEdge {
            id: "a_to_b".to_string(),
            from_node_id: "a".to_string(),
            to_node_id: "b".to_string(),
            condition: "always".to_string(),
        });
        definition.edges.push(WorkflowEdge {
            id: "b_to_a".to_string(),
            from_node_id: "b".to_string(),
            to_node_id: "a".to_string(),
            condition: "always".to_string(),
        });

        validate_publish_ready(&definition).unwrap();
    }

    #[test]
    fn publish_validation_rejects_selector_without_model_or_fallback() {
        let mut definition = WorkflowDefinition::new_draft("workflow.selector", "Selector");
        definition.metadata = serde_json::json!({
            "routing": {
                "mode": "selector",
                "selector_candidates": ["a", "b"]
            }
        });
        definition.nodes.push(output_node("a"));
        definition.nodes.push(output_node("b"));

        let err = validate_publish_ready(&definition).unwrap_err();

        assert!(err.to_string().contains("selector model"));
    }

    #[test]
    fn publish_validation_allows_selector_with_model() {
        let mut definition = WorkflowDefinition::new_draft("workflow.selector", "Selector");
        definition.metadata = serde_json::json!({
            "routing": {
                "mode": "selector",
                "selector_candidates": ["a", "b"],
                "selector": {
                    "type": "model",
                    "model": {
                        "provider": "openai",
                        "model": "gpt-4.1"
                    }
                }
            }
        });
        definition.nodes.push(output_node("a"));
        definition.nodes.push(output_node("b"));

        validate_publish_ready(&definition).unwrap();
    }

    #[test]
    fn publish_validation_allows_selector_with_deterministic_fallback() {
        let mut definition = WorkflowDefinition::new_draft("workflow.selector", "Selector");
        definition.metadata = serde_json::json!({
            "routing": {
                "mode": "selector",
                "selector_candidates": ["a", "b"],
                "selector": {
                    "type": "deterministic",
                    "strategy": "first_available"
                }
            }
        });
        definition.nodes.push(output_node("a"));
        definition.nodes.push(output_node("b"));

        validate_publish_ready(&definition).unwrap();
    }
}
