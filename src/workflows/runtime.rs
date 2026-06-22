use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;

use super::definition::{WorkflowDefinition, WorkflowEdge, WorkflowNode, WorkflowNodeKind};
use super::routing_policy::{ActivationCondition, RoutingMode, RoutingPolicy};
use crate::agents::core::{
    tool_dispatcher::ToolDispatcher, tool_registry, AgentDefinition, AgentRunContext, AgentRuntime,
    AgentTrait, MainAgent, OrchestratorEvent,
};
use crate::memory::types::ChatMessage;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowRunStatus {
    Running,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone)]
pub struct WorkflowRun {
    pub id: usize,
    pub workflow_id: String,
    pub workflow_version: i64,
    pub status: WorkflowRunStatus,
}

pub struct WorkflowRuntime;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeRunStatus {
    Pending,
    Ready,
    Running,
    WaitingUser,
    Succeeded,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Default)]
pub struct GraphExecutionState {
    pub node_status: std::collections::HashMap<String, NodeRunStatus>,
    pub node_outputs: std::collections::HashMap<String, Value>,
}

impl GraphExecutionState {
    fn new(definition: &WorkflowDefinition) -> Self {
        Self {
            node_status: definition
                .nodes
                .iter()
                .map(|node| (node.id.clone(), NodeRunStatus::Pending))
                .collect(),
            node_outputs: std::collections::HashMap::new(),
        }
    }

    fn set_status(&mut self, node_id: &str, status: NodeRunStatus) {
        self.node_status.insert(node_id.to_string(), status);
    }

    fn set_output(&mut self, node_id: &str, output: Value) {
        self.node_outputs.insert(node_id.to_string(), output);
    }
}

pub struct WorkflowScheduler<'a> {
    definition: &'a WorkflowDefinition,
    state: GraphExecutionState,
}

impl<'a> WorkflowScheduler<'a> {
    pub fn new(definition: &'a WorkflowDefinition) -> Self {
        Self {
            definition,
            state: GraphExecutionState::new(definition),
        }
    }

    pub fn state(&self) -> &GraphExecutionState {
        &self.state
    }

    pub async fn execute(&mut self, input: Value) -> Result<Value> {
        if self.definition.nodes.is_empty() {
            return Ok(input);
        }

        let policy = self.workflow_policy()?;
        match policy.mode {
            RoutingMode::Parallel => self.execute_parallel(input, policy).await,
            RoutingMode::Graph | RoutingMode::Selector | RoutingMode::Handoff => {
                if self.definition.edges.is_empty() {
                    self.execute_linear(&self.definition.nodes, input).await
                } else {
                    self.execute_graph(input).await
                }
            }
            RoutingMode::Sequential => {
                if self.definition.edges.is_empty() {
                    self.execute_linear(&self.definition.nodes, input).await
                } else {
                    self.execute_graph(input).await
                }
            }
        }
    }

    async fn execute_linear(&mut self, nodes: &[WorkflowNode], input: Value) -> Result<Value> {
        let mut current = input;
        for node in nodes {
            current = self.execute_node_with_status(node, current).await?;
            if is_awaiting_human_approval(&current) {
                return Ok(current);
            }
            if should_terminate(self.definition, &current) {
                return Ok(termination_result(self.definition, current));
            }
        }
        Ok(current)
    }

    async fn execute_graph(&mut self, input: Value) -> Result<Value> {
        let current_node_id = first_graph_node_id(self.definition)?;
        self.execute_graph_from_node(current_node_id, input).await
    }

    async fn execute_graph_from_node(
        &mut self,
        mut current_node_id: String,
        input: Value,
    ) -> Result<Value> {
        let mut current = input;
        let max_steps = self
            .definition
            .metadata
            .get("max_steps")
            .and_then(|value| value.as_u64())
            .map(|value| value as usize)
            .unwrap_or_else(|| {
                self.definition
                    .nodes
                    .len()
                    .saturating_mul(4)
                    .saturating_add(16)
            });

        for _ in 0..max_steps {
            let node = self
                .definition
                .nodes
                .iter()
                .find(|node| node.id == current_node_id)
                .ok_or_else(|| anyhow::anyhow!("workflow node '{}' not found", current_node_id))?;
            current = self.execute_node_with_status(node, current).await?;
            if is_awaiting_human_approval(&current) {
                return Ok(current);
            }
            if should_terminate(self.definition, &current) {
                return Ok(termination_result(self.definition, current));
            }

            let Some(next_node_id) =
                select_next_node_id(&self.definition.edges, &node.id, &current)
            else {
                return Ok(current);
            };
            current_node_id = next_node_id;
        }

        anyhow::bail!(
            "workflow '{}' exceeded max_steps while executing graph",
            self.definition.id
        );
    }

    async fn execute_parallel(&mut self, input: Value, policy: RoutingPolicy) -> Result<Value> {
        let activation = policy.activation.unwrap_or(ActivationCondition::All);
        let node_ids = parallel_entry_node_ids(self.definition);
        for node_id in &node_ids {
            self.state.set_status(node_id, NodeRunStatus::Running);
        }

        let futures = node_ids.iter().filter_map(|node_id| {
            self.definition
                .nodes
                .iter()
                .find(|node| node.id == *node_id)
                .map(|node| async {
                    let result = execute_node(node, input.clone()).await;
                    (node.id.clone(), result)
                })
        });
        let results = futures::future::join_all(futures).await;

        let mut output = serde_json::Map::new();
        let mut success_count = 0usize;
        let mut first_error = None;
        for (node_id, result) in results {
            match result {
                Ok(value) => {
                    if is_awaiting_human_approval(&value) {
                        self.state.set_status(&node_id, NodeRunStatus::WaitingUser);
                    } else {
                        self.state.set_status(&node_id, NodeRunStatus::Succeeded);
                    }
                    self.state.set_output(&node_id, value.clone());
                    output.insert(node_id, value);
                    success_count += 1;
                }
                Err(err) => {
                    self.state.set_status(&node_id, NodeRunStatus::Failed);
                    output.insert(
                        node_id,
                        serde_json::json!({
                            "status": "failed",
                            "error": err.to_string(),
                        }),
                    );
                    first_error.get_or_insert_with(|| err.to_string());
                }
            }
        }

        match activation {
            ActivationCondition::All if success_count != output.len() => {
                anyhow::bail!(
                    "parallel workflow '{}' failed: {}",
                    self.definition.id,
                    first_error.unwrap_or_else(|| "one or more nodes failed".to_string())
                );
            }
            ActivationCondition::Any if success_count == 0 => {
                anyhow::bail!(
                    "parallel workflow '{}' failed: {}",
                    self.definition.id,
                    first_error.unwrap_or_else(|| "all nodes failed".to_string())
                );
            }
            _ => {}
        }

        Ok(serde_json::json!({
            "status": "parallel_completed",
            "activation": activation,
            "results": Value::Object(output),
        }))
    }

    async fn execute_node_with_status(
        &mut self,
        node: &WorkflowNode,
        input: Value,
    ) -> Result<Value> {
        self.state.set_status(&node.id, NodeRunStatus::Running);
        match execute_node(node, input).await {
            Ok(value) => {
                if is_awaiting_human_approval(&value) {
                    self.state.set_status(&node.id, NodeRunStatus::WaitingUser);
                } else {
                    self.state.set_status(&node.id, NodeRunStatus::Succeeded);
                }
                self.state.set_output(&node.id, value.clone());
                Ok(value)
            }
            Err(err) => {
                self.state.set_status(&node.id, NodeRunStatus::Failed);
                Err(err)
            }
        }
    }

    fn workflow_policy(&self) -> Result<RoutingPolicy> {
        let policy = self
            .definition
            .metadata
            .get("routing")
            .map(RoutingPolicy::from_value)
            .transpose()?
            .unwrap_or_else(RoutingPolicy::sequential);
        policy.validate(Some(self.definition))?;
        Ok(policy)
    }
}

impl WorkflowRuntime {
    pub fn new() -> Self {
        Self
    }

    pub async fn run_definition(
        &self,
        definition: &WorkflowDefinition,
        input: Value,
    ) -> Result<Value> {
        let (result, state) = match workflow_timeout(definition) {
            Some(timeout) => {
                tokio::time::timeout(timeout, self.execute_scheduled_nodes(definition, input))
                    .await
                    .map_err(|_| {
                        anyhow::anyhow!(
                            "workflow '{}' timed out after {}ms",
                            definition.id,
                            timeout.as_millis()
                        )
                    })??
            }
            None => self.execute_scheduled_nodes(definition, input).await?,
        };
        Ok(serde_json::json!({
            "status": "succeeded",
            "workflow_id": definition.id,
            "workflow_version": definition.version,
            "node_status": node_status_json(&state),
            "result": result,
        }))
    }

    pub async fn resume_definition(
        &self,
        definition: &WorkflowDefinition,
        paused_node_id: &str,
        resume_input: Value,
    ) -> Result<Value> {
        let result = match workflow_timeout(definition) {
            Some(timeout) => tokio::time::timeout(
                timeout,
                self.resume_nodes(definition, paused_node_id, resume_input),
            )
            .await
            .map_err(|_| {
                anyhow::anyhow!(
                    "workflow '{}' timed out after {}ms while resuming",
                    definition.id,
                    timeout.as_millis()
                )
            })??,
            None => {
                self.resume_nodes(definition, paused_node_id, resume_input)
                    .await?
            }
        };
        Ok(serde_json::json!({
            "status": "succeeded",
            "workflow_id": definition.id,
            "workflow_version": definition.version,
            "resumed_from": paused_node_id,
            "result": result,
        }))
    }

    async fn execute_scheduled_nodes(
        &self,
        definition: &WorkflowDefinition,
        input: Value,
    ) -> Result<(Value, GraphExecutionState)> {
        let mut scheduler = WorkflowScheduler::new(definition);
        let result = scheduler.execute(input).await?;
        Ok((result, scheduler.state().clone()))
    }

    async fn resume_nodes(
        &self,
        definition: &WorkflowDefinition,
        paused_node_id: &str,
        resume_input: Value,
    ) -> Result<Value> {
        if definition.nodes.is_empty() {
            return Ok(resume_input);
        }

        if !definition.edges.is_empty() {
            let Some(next_node_id) =
                select_next_node_id(&definition.edges, paused_node_id, &resume_input)
            else {
                return Ok(resume_input);
            };
            return execute_graph_from_node(definition, next_node_id, resume_input).await;
        }

        let Some(paused_index) = definition
            .nodes
            .iter()
            .position(|node| node.id == paused_node_id)
        else {
            anyhow::bail!("paused workflow node '{}' not found", paused_node_id);
        };
        let remaining = definition
            .nodes
            .get(paused_index.saturating_add(1)..)
            .unwrap_or_default();
        execute_linear(definition, remaining, resume_input).await
    }
}

impl Default for WorkflowRuntime {
    fn default() -> Self {
        Self::new()
    }
}

async fn execute_linear(
    definition: &WorkflowDefinition,
    nodes: &[WorkflowNode],
    input: Value,
) -> Result<Value> {
    let mut current = input;
    for node in nodes {
        current = execute_node(node, current).await?;
        if is_awaiting_human_approval(&current) {
            return Ok(current);
        }
        if should_terminate(definition, &current) {
            return Ok(termination_result(definition, current));
        }
    }
    Ok(current)
}

async fn execute_graph_from_node(
    definition: &WorkflowDefinition,
    mut current_node_id: String,
    input: Value,
) -> Result<Value> {
    let mut current = input;
    let max_steps = definition
        .metadata
        .get("max_steps")
        .and_then(|value| value.as_u64())
        .map(|value| value as usize)
        .unwrap_or_else(|| definition.nodes.len().saturating_mul(4).saturating_add(16));

    for _ in 0..max_steps {
        let node = definition
            .nodes
            .iter()
            .find(|node| node.id == current_node_id)
            .ok_or_else(|| anyhow::anyhow!("workflow node '{}' not found", current_node_id))?;
        current = execute_node(node, current).await?;
        if is_awaiting_human_approval(&current) {
            return Ok(current);
        }
        if should_terminate(definition, &current) {
            return Ok(termination_result(definition, current));
        }

        let Some(next_node_id) = select_next_node_id(&definition.edges, &node.id, &current) else {
            return Ok(current);
        };
        current_node_id = next_node_id;
    }

    anyhow::bail!(
        "workflow '{}' exceeded max_steps while executing graph",
        definition.id
    );
}

fn workflow_timeout(definition: &WorkflowDefinition) -> Option<Duration> {
    let milliseconds = definition
        .metadata
        .get("timeout_ms")
        .and_then(|value| value.as_u64())
        .or_else(|| {
            definition
                .metadata
                .get("timeout_secs")
                .and_then(|value| value.as_u64())
                .map(|seconds| seconds.saturating_mul(1000))
        })?;
    if milliseconds == 0 {
        None
    } else {
        Some(Duration::from_millis(milliseconds))
    }
}

fn should_terminate(definition: &WorkflowDefinition, value: &Value) -> bool {
    definition
        .metadata
        .get("termination_condition")
        .and_then(|condition| condition.as_str())
        .and_then(|condition| condition_matches(condition, value))
        .unwrap_or(false)
}

fn termination_result(definition: &WorkflowDefinition, value: Value) -> Value {
    serde_json::json!({
        "status": "terminated",
        "workflow_id": definition.id,
        "condition": definition
            .metadata
            .get("termination_condition")
            .and_then(|condition| condition.as_str())
            .unwrap_or_default(),
        "result": value,
    })
}

fn is_awaiting_human_approval(value: &Value) -> bool {
    value
        .get("status")
        .and_then(|status| status.as_str())
        .map(|status| status == "awaiting_human_approval")
        .unwrap_or(false)
}

fn node_status_json(state: &GraphExecutionState) -> Value {
    serde_json::to_value(&state.node_status).unwrap_or_else(|_| serde_json::json!({}))
}

async fn execute_node(node: &WorkflowNode, input: Value) -> Result<Value> {
    match &node.kind {
        WorkflowNodeKind::Agent { agent_id } => {
            execute_agent_node(agent_id, node.config.clone(), input).await
        }
        WorkflowNodeKind::Skill { skill_id } => {
            execute_skill_node(skill_id, node.config.clone(), input).await
        }
        WorkflowNodeKind::McpTool {
            server_name,
            tool_name,
        } => Ok(serde_json::json!({
            "status": "not_ready",
            "node_id": node.id,
            "server_name": server_name,
            "tool_name": tool_name,
            "message": "MCP workflow nodes are deferred.",
            "input": input,
        })),
        WorkflowNodeKind::Condition => Ok(input),
        WorkflowNodeKind::HumanApproval => Ok(serde_json::json!({
            "status": "awaiting_human_approval",
            "node_id": node.id,
            "input": input,
        })),
        WorkflowNodeKind::Output => Ok(node.config.get("value").cloned().unwrap_or(input)),
    }
}

fn first_graph_node_id(definition: &WorkflowDefinition) -> Result<String> {
    let incoming: std::collections::HashSet<&str> = definition
        .edges
        .iter()
        .map(|edge| edge.to_node_id.as_str())
        .collect();
    definition
        .nodes
        .iter()
        .find(|node| !incoming.contains(node.id.as_str()))
        .or_else(|| definition.nodes.first())
        .map(|node| node.id.clone())
        .ok_or_else(|| anyhow::anyhow!("workflow '{}' has no nodes", definition.id))
}

fn parallel_entry_node_ids(definition: &WorkflowDefinition) -> Vec<String> {
    if definition.edges.is_empty() {
        return definition
            .nodes
            .iter()
            .map(|node| node.id.clone())
            .collect();
    }

    let incoming: std::collections::HashSet<&str> = definition
        .edges
        .iter()
        .map(|edge| edge.to_node_id.as_str())
        .collect();
    let ids = definition
        .nodes
        .iter()
        .filter(|node| !incoming.contains(node.id.as_str()))
        .map(|node| node.id.clone())
        .collect::<Vec<_>>();
    if ids.is_empty() {
        definition
            .nodes
            .first()
            .map(|node| vec![node.id.clone()])
            .unwrap_or_default()
    } else {
        ids
    }
}

fn select_next_node_id(
    edges: &[WorkflowEdge],
    from_node_id: &str,
    value: &Value,
) -> Option<String> {
    let outgoing = edges
        .iter()
        .filter(|edge| edge.from_node_id == from_node_id)
        .collect::<Vec<_>>();
    outgoing
        .iter()
        .find(|edge| condition_matches(&edge.condition, value) == Some(true))
        .or_else(|| {
            outgoing
                .iter()
                .find(|edge| condition_matches(&edge.condition, value).is_none())
        })
        .map(|edge| edge.to_node_id.clone())
}

fn condition_matches(condition: &str, value: &Value) -> Option<bool> {
    let condition = condition.trim();
    if condition.is_empty()
        || condition.eq_ignore_ascii_case("default")
        || condition.eq_ignore_ascii_case("else")
    {
        return None;
    }
    if condition.eq_ignore_ascii_case("true") || condition.eq_ignore_ascii_case("always") {
        return Some(true);
    }
    if condition.eq_ignore_ascii_case("false") || condition.eq_ignore_ascii_case("never") {
        return Some(false);
    }

    for operator in ["==", "!="] {
        if let Some((path, expected)) = condition.split_once(operator) {
            let actual = value_at_path(value, path.trim());
            let expected = parse_condition_literal(expected.trim());
            let equals = actual
                .map(|actual| values_equal(actual, &expected))
                .unwrap_or(false);
            return Some(if operator == "==" { equals } else { !equals });
        }
    }

    value_at_path(value, condition).and_then(|actual| actual.as_bool())
}

fn value_at_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    let path = path.trim().trim_start_matches("$.").trim_start_matches('.');
    if path.is_empty() {
        return Some(value);
    }
    let mut current = value;
    for part in path.split('.') {
        current = current.get(part)?;
    }
    Some(current)
}

fn parse_condition_literal(raw: &str) -> Value {
    let raw = raw.trim();
    if let Ok(value) = serde_json::from_str(raw) {
        return value;
    }
    Value::String(raw.trim_matches('"').trim_matches('\'').to_string())
}

fn values_equal(actual: &Value, expected: &Value) -> bool {
    if actual == expected {
        return true;
    }
    match (actual, expected) {
        (Value::String(actual), Value::String(expected)) => actual == expected,
        (Value::String(actual), other) => actual == &other.to_string(),
        (other, Value::String(expected)) => &other.to_string() == expected,
        _ => false,
    }
}

async fn execute_skill_node(skill_id: &str, config: Value, input: Value) -> Result<Value> {
    let apply = config
        .get("apply")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let args = config.get("args").cloned().unwrap_or(input);

    let Some(skill) = crate::skills::find_skill(skill_id) else {
        anyhow::bail!("workflow skill '{}' not found", skill_id);
    };

    if apply {
        let exec = skill.execute(args, None).await?;
        Ok(serde_json::json!({
            "stage": "execute",
            "skill_id": skill_id,
            "summary": exec.summary,
            "denied": exec.denied,
            "freed_bytes": exec.freed_bytes,
            "success": exec.success_items,
            "failed": exec.failed_items.iter()
                .map(|(item, error)| serde_json::json!({ "item": item, "error": error }))
                .collect::<Vec<_>>(),
        }))
    } else {
        let preview = skill.preview(args).await?;
        Ok(serde_json::json!({
            "stage": "preview",
            "skill_id": skill_id,
            "summary": preview.summary,
            "estimated_bytes": preview.estimated_bytes,
            "items": preview.items.iter().map(|item| serde_json::json!({
                "label": item.label,
                "detail": item.detail,
                "bytes": item.bytes,
            })).collect::<Vec<_>>(),
            "warnings": preview.warnings,
        }))
    }
}

async fn execute_agent_node(agent_id: &str, config: Value, input: Value) -> Result<Value> {
    let normalized = agent_id.trim().to_ascii_lowercase();
    let is_main_agent =
        normalized == "main" || normalized == "mainagent" || normalized == "main_agent";
    let agent_definition = AgentDefinition::from_node_config(agent_id, &config)?;
    if !is_main_agent && agent_definition.is_none() {
        return Ok(serde_json::json!({
            "status": "not_ready",
            "agent_id": agent_id,
            "message": "Agent node requires config.agent_definition or top-level system_prompt for custom agents.",
            "input": input,
        }));
    }

    let app_config = crate::services::config::load_config();
    let workspace = config
        .get("workspace")
        .and_then(|value| value.as_str())
        .unwrap_or("workflow")
        .to_string();
    let prompt = config
        .get("prompt")
        .and_then(|value| value.as_str())
        .map(|value| value.to_string())
        .unwrap_or_else(|| {
            serde_json::to_string_pretty(&input).unwrap_or_else(|_| String::from("{}"))
        });

    tool_registry::init_tool_registry(&workspace);
    let agent: Arc<dyn AgentTrait> = match agent_definition {
        Some(definition) => Arc::new(definition.into_agent(&app_config)),
        None => Arc::new(MainAgent::with_workspace(
            app_config.model_name,
            app_config.model_base_url,
            app_config.model_api_key,
            workspace,
        )),
    };
    let runtime = AgentRuntime::new(agent, ToolDispatcher::new(None));
    let mut context = AgentRunContext::new("workflow-agent-node".to_string());
    context.add_message(ChatMessage::new("user", &prompt));
    let answer = runtime.run(context, |_event: OrchestratorEvent| {}).await?;

    Ok(serde_json::json!({
        "status": "succeeded",
        "agent_id": agent_id,
        "answer": answer,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflows::definition::{WorkflowEdge, WorkflowNode, WorkflowNodeKind};

    #[tokio::test]
    async fn runtime_executes_output_only_workflow() {
        let runtime = WorkflowRuntime::new();
        let mut definition = WorkflowDefinition::new_draft("workflow.echo", "Echo");
        definition.nodes.push(WorkflowNode {
            id: "output".to_string(),
            name: "Output".to_string(),
            kind: WorkflowNodeKind::Output,
            config: serde_json::json!({ "value": { "ok": true } }),
        });

        let result = runtime
            .run_definition(&definition, serde_json::json!({ "ignored": true }))
            .await
            .unwrap();

        assert_eq!(result["status"], "succeeded");
        assert_eq!(result["result"], serde_json::json!({ "ok": true }));
    }

    #[tokio::test]
    async fn runtime_marks_unknown_agent_node_not_ready() {
        let runtime = WorkflowRuntime::new();
        let mut definition = WorkflowDefinition::new_draft("workflow.agent", "Agent");
        definition.nodes.push(WorkflowNode {
            id: "agent".to_string(),
            name: "Unknown Agent".to_string(),
            kind: WorkflowNodeKind::Agent {
                agent_id: "researcher".to_string(),
            },
            config: serde_json::Value::Null,
        });

        let result = runtime
            .run_definition(&definition, serde_json::json!({ "task": "summarize" }))
            .await
            .unwrap();

        assert_eq!(result["status"], "succeeded");
        assert_eq!(result["result"]["status"], "not_ready");
        assert_eq!(result["result"]["agent_id"], "researcher");
    }

    #[tokio::test]
    async fn runtime_routes_graph_by_edge_conditions() {
        let runtime = WorkflowRuntime::new();
        let mut definition = WorkflowDefinition::new_draft("workflow.route", "Route");
        definition.nodes.push(WorkflowNode {
            id: "condition".to_string(),
            name: "Condition".to_string(),
            kind: WorkflowNodeKind::Condition,
            config: serde_json::Value::Null,
        });
        definition.nodes.push(WorkflowNode {
            id: "a".to_string(),
            name: "A".to_string(),
            kind: WorkflowNodeKind::Output,
            config: serde_json::json!({ "value": { "branch": "a" } }),
        });
        definition.nodes.push(WorkflowNode {
            id: "b".to_string(),
            name: "B".to_string(),
            kind: WorkflowNodeKind::Output,
            config: serde_json::json!({ "value": { "branch": "b" } }),
        });
        definition.edges.push(WorkflowEdge {
            id: "condition_to_a".to_string(),
            from_node_id: "condition".to_string(),
            to_node_id: "a".to_string(),
            condition: "route == 'a'".to_string(),
        });
        definition.edges.push(WorkflowEdge {
            id: "condition_to_b".to_string(),
            from_node_id: "condition".to_string(),
            to_node_id: "b".to_string(),
            condition: "route == 'b'".to_string(),
        });

        let result = runtime
            .run_definition(&definition, serde_json::json!({ "route": "b" }))
            .await
            .unwrap();

        assert_eq!(result["result"], serde_json::json!({ "branch": "b" }));
    }

    #[tokio::test]
    async fn scheduler_tracks_sequential_node_status() {
        let mut definition = WorkflowDefinition::new_draft("workflow.sequential", "Sequential");
        definition.nodes.push(WorkflowNode {
            id: "first".to_string(),
            name: "First".to_string(),
            kind: WorkflowNodeKind::Condition,
            config: serde_json::Value::Null,
        });
        definition.nodes.push(WorkflowNode {
            id: "output".to_string(),
            name: "Output".to_string(),
            kind: WorkflowNodeKind::Output,
            config: serde_json::json!({ "value": { "ok": true } }),
        });
        let mut scheduler = WorkflowScheduler::new(&definition);

        let result = scheduler
            .execute(serde_json::json!({ "ok": false }))
            .await
            .unwrap();

        assert_eq!(result, serde_json::json!({ "ok": true }));
        assert_eq!(
            scheduler.state().node_status["first"],
            NodeRunStatus::Succeeded
        );
        assert_eq!(
            scheduler.state().node_status["output"],
            NodeRunStatus::Succeeded
        );
    }

    #[tokio::test]
    async fn scheduler_executes_parallel_all_join() {
        let mut definition = WorkflowDefinition::new_draft("workflow.parallel", "Parallel");
        definition.metadata = serde_json::json!({
            "routing": {
                "mode": "parallel",
                "activation": "all"
            }
        });
        definition.nodes.push(WorkflowNode {
            id: "a".to_string(),
            name: "A".to_string(),
            kind: WorkflowNodeKind::Output,
            config: serde_json::json!({ "value": { "branch": "a" } }),
        });
        definition.nodes.push(WorkflowNode {
            id: "b".to_string(),
            name: "B".to_string(),
            kind: WorkflowNodeKind::Output,
            config: serde_json::json!({ "value": { "branch": "b" } }),
        });
        let mut scheduler = WorkflowScheduler::new(&definition);

        let result = scheduler.execute(serde_json::json!({})).await.unwrap();

        assert_eq!(result["status"], "parallel_completed");
        assert_eq!(result["activation"], "all");
        assert_eq!(result["results"]["a"], serde_json::json!({ "branch": "a" }));
        assert_eq!(result["results"]["b"], serde_json::json!({ "branch": "b" }));
        assert_eq!(scheduler.state().node_status["a"], NodeRunStatus::Succeeded);
        assert_eq!(scheduler.state().node_status["b"], NodeRunStatus::Succeeded);
    }

    #[tokio::test]
    async fn scheduler_executes_parallel_any_join_with_partial_failure() {
        let mut definition = WorkflowDefinition::new_draft("workflow.parallel_any", "Parallel Any");
        definition.metadata = serde_json::json!({
            "routing": {
                "mode": "parallel",
                "activation": "any"
            }
        });
        definition.nodes.push(WorkflowNode {
            id: "ok".to_string(),
            name: "OK".to_string(),
            kind: WorkflowNodeKind::Output,
            config: serde_json::json!({ "value": { "branch": "ok" } }),
        });
        definition.nodes.push(WorkflowNode {
            id: "missing_skill".to_string(),
            name: "Missing Skill".to_string(),
            kind: WorkflowNodeKind::Skill {
                skill_id: "missing.skill".to_string(),
            },
            config: serde_json::Value::Null,
        });
        let mut scheduler = WorkflowScheduler::new(&definition);

        let result = scheduler.execute(serde_json::json!({})).await.unwrap();

        assert_eq!(result["status"], "parallel_completed");
        assert_eq!(
            result["results"]["ok"],
            serde_json::json!({ "branch": "ok" })
        );
        assert_eq!(
            scheduler.state().node_status["missing_skill"],
            NodeRunStatus::Failed
        );
    }

    #[tokio::test]
    async fn runtime_stops_graph_after_max_steps() {
        let runtime = WorkflowRuntime::new();
        let mut definition = WorkflowDefinition::new_draft("workflow.loop", "Loop");
        definition.metadata = serde_json::json!({ "max_steps": 2 });
        definition.nodes.push(WorkflowNode {
            id: "condition".to_string(),
            name: "Condition".to_string(),
            kind: WorkflowNodeKind::Condition,
            config: serde_json::Value::Null,
        });
        definition.edges.push(WorkflowEdge {
            id: "loop".to_string(),
            from_node_id: "condition".to_string(),
            to_node_id: "condition".to_string(),
            condition: "always".to_string(),
        });

        let err = runtime
            .run_definition(&definition, serde_json::json!({ "route": "b" }))
            .await
            .unwrap_err();

        assert!(err.to_string().contains("exceeded max_steps"));
    }

    #[tokio::test]
    async fn runtime_pauses_at_human_approval_node() {
        let runtime = WorkflowRuntime::new();
        let mut definition = WorkflowDefinition::new_draft("workflow.approval", "Approval");
        definition.nodes.push(WorkflowNode {
            id: "approval".to_string(),
            name: "Approval".to_string(),
            kind: WorkflowNodeKind::HumanApproval,
            config: serde_json::Value::Null,
        });
        definition.nodes.push(WorkflowNode {
            id: "after".to_string(),
            name: "After".to_string(),
            kind: WorkflowNodeKind::Output,
            config: serde_json::json!({ "value": { "should_not_run": true } }),
        });

        let result = runtime
            .run_definition(&definition, serde_json::json!({ "draft": true }))
            .await
            .unwrap();

        assert_eq!(result["result"]["status"], "awaiting_human_approval");
        assert_eq!(result["result"]["node_id"], "approval");
        assert_eq!(
            result["result"]["input"],
            serde_json::json!({ "draft": true })
        );
    }

    #[tokio::test]
    async fn runtime_stops_when_termination_condition_matches() {
        let runtime = WorkflowRuntime::new();
        let mut definition = WorkflowDefinition::new_draft("workflow.terminate", "Terminate");
        definition.metadata = serde_json::json!({
            "termination_condition": "done == true"
        });
        definition.nodes.push(WorkflowNode {
            id: "first".to_string(),
            name: "First".to_string(),
            kind: WorkflowNodeKind::Output,
            config: serde_json::json!({ "value": { "done": true } }),
        });
        definition.nodes.push(WorkflowNode {
            id: "after".to_string(),
            name: "After".to_string(),
            kind: WorkflowNodeKind::Output,
            config: serde_json::json!({ "value": { "should_not_run": true } }),
        });

        let result = runtime
            .run_definition(&definition, serde_json::json!({}))
            .await
            .unwrap();

        assert_eq!(result["result"]["status"], "terminated");
        assert_eq!(result["result"]["condition"], "done == true");
        assert_eq!(
            result["result"]["result"],
            serde_json::json!({ "done": true })
        );
    }

    #[test]
    fn workflow_timeout_reads_milliseconds_and_seconds() {
        let mut definition = WorkflowDefinition::new_draft("workflow.timeout", "Timeout");
        definition.metadata = serde_json::json!({ "timeout_ms": 250 });
        assert_eq!(
            workflow_timeout(&definition),
            Some(std::time::Duration::from_millis(250))
        );

        definition.metadata = serde_json::json!({ "timeout_secs": 2 });
        assert_eq!(
            workflow_timeout(&definition),
            Some(std::time::Duration::from_millis(2000))
        );

        definition.metadata = serde_json::json!({ "timeout_ms": 0 });
        assert_eq!(workflow_timeout(&definition), None);
    }

    #[tokio::test]
    async fn runtime_resumes_linear_workflow_after_human_approval() {
        let runtime = WorkflowRuntime::new();
        let mut definition = WorkflowDefinition::new_draft("workflow.resume", "Resume");
        definition.nodes.push(WorkflowNode {
            id: "approval".to_string(),
            name: "Approval".to_string(),
            kind: WorkflowNodeKind::HumanApproval,
            config: serde_json::Value::Null,
        });
        definition.nodes.push(WorkflowNode {
            id: "after".to_string(),
            name: "After".to_string(),
            kind: WorkflowNodeKind::Output,
            config: serde_json::json!({ "value": { "continued": true } }),
        });

        let paused = runtime
            .run_definition(&definition, serde_json::json!({ "draft": true }))
            .await
            .unwrap();
        assert_eq!(paused["result"]["status"], "awaiting_human_approval");

        let resumed = runtime
            .resume_definition(
                &definition,
                "approval",
                serde_json::json!({ "approved": true }),
            )
            .await
            .unwrap();

        assert_eq!(resumed["resumed_from"], "approval");
        assert_eq!(resumed["result"], serde_json::json!({ "continued": true }));
    }

    #[tokio::test]
    async fn runtime_resumes_graph_workflow_from_approval_edge() {
        let runtime = WorkflowRuntime::new();
        let mut definition = WorkflowDefinition::new_draft("workflow.resume_graph", "Resume Graph");
        definition.nodes.push(WorkflowNode {
            id: "approval".to_string(),
            name: "Approval".to_string(),
            kind: WorkflowNodeKind::HumanApproval,
            config: serde_json::Value::Null,
        });
        definition.nodes.push(WorkflowNode {
            id: "approved".to_string(),
            name: "Approved".to_string(),
            kind: WorkflowNodeKind::Output,
            config: serde_json::json!({ "value": { "branch": "approved" } }),
        });
        definition.nodes.push(WorkflowNode {
            id: "rejected".to_string(),
            name: "Rejected".to_string(),
            kind: WorkflowNodeKind::Output,
            config: serde_json::json!({ "value": { "branch": "rejected" } }),
        });
        definition.edges.push(WorkflowEdge {
            id: "approval_to_approved".to_string(),
            from_node_id: "approval".to_string(),
            to_node_id: "approved".to_string(),
            condition: "approved == true".to_string(),
        });
        definition.edges.push(WorkflowEdge {
            id: "approval_to_rejected".to_string(),
            from_node_id: "approval".to_string(),
            to_node_id: "rejected".to_string(),
            condition: "approved == false".to_string(),
        });

        let resumed = runtime
            .resume_definition(
                &definition,
                "approval",
                serde_json::json!({ "approved": false }),
            )
            .await
            .unwrap();

        assert_eq!(
            resumed["result"],
            serde_json::json!({ "branch": "rejected" })
        );
    }
}
