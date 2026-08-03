use gpui::{
    App, Bounds, Element, ElementId, GlobalElementId, InspectorElementId, IntoElement, LayoutId,
    Pixels, Refineable, Style, StyleRefinement, Styled, Window,
};
#[cfg(all(
    feature = "workflow-webview",
    any(target_os = "macos", target_os = "windows")
))]
use serde::Deserialize;
use serde::Serialize;
use std::sync::{Mutex, OnceLock};

use crate::workflows::{WorkflowDefinition, WorkflowNodeKind};

pub(crate) fn workflow_builder_webview(
    state: Option<WorkflowBuilderState>,
) -> WorkflowWebviewElement {
    WorkflowWebviewElement {
        state,
        style: StyleRefinement::default(),
    }
}

pub(crate) fn webview_status_label() -> &'static str {
    native::status_label()
}

#[cfg_attr(
    not(all(
        feature = "workflow-webview",
        any(target_os = "macos", target_os = "windows")
    )),
    allow(dead_code)
)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum WorkflowWebviewEvent {
    AddAgent {
        request_id: Option<String>,
        workflow_id: Option<String>,
    },
    Save {
        request_id: Option<String>,
        workflow_id: Option<String>,
    },
    Run {
        request_id: Option<String>,
        workflow_id: Option<String>,
    },
    Publish {
        request_id: Option<String>,
        workflow_id: Option<String>,
    },
    CopilotGenerate {
        request_id: Option<String>,
        workflow_id: Option<String>,
        brief: String,
    },
    SelectWorkflow {
        request_id: Option<String>,
        workflow_id: String,
    },
    CreateFromTemplate {
        request_id: Option<String>,
        template_id: String,
    },
    UpdateJson {
        request_id: Option<String>,
        workflow_id: Option<String>,
        json: String,
    },
    UpdateAgent {
        request_id: Option<String>,
        workflow_id: Option<String>,
        node_id: String,
        update: WorkflowAgentUpdateView,
    },
    NodeSelected {
        request_id: Option<String>,
        workflow_id: Option<String>,
        node_id: String,
    },
    EdgeCreated {
        request_id: Option<String>,
        workflow_id: Option<String>,
        source_node_id: String,
        target_node_id: String,
    },
    EdgeDeleted {
        request_id: Option<String>,
        workflow_id: Option<String>,
        edge_id: String,
    },
    Error {
        request_id: Option<String>,
        workflow_id: Option<String>,
        message: String,
    },
}

pub(crate) type WorkflowCanvasEvent = WorkflowWebviewEvent;

#[cfg(all(
    feature = "workflow-webview",
    any(target_os = "macos", target_os = "windows")
))]
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub(crate) enum WorkflowWebviewCommand {
    #[serde(rename = "workflow:ready")]
    WorkflowReady,
    #[serde(rename = "workflow:loaded")]
    WorkflowLoaded {
        #[serde(default, rename = "requestId")]
        request_id: Option<String>,
        #[serde(default, rename = "workflowId")]
        workflow_id: Option<String>,
    },
    #[serde(rename = "workflow:add_agent")]
    AddAgent {
        #[serde(default, rename = "requestId")]
        request_id: Option<String>,
        #[serde(default, rename = "workflowId")]
        workflow_id: Option<String>,
    },
    #[serde(rename = "workflow:save")]
    Save {
        #[serde(default, rename = "requestId")]
        request_id: Option<String>,
        #[serde(default, rename = "workflowId")]
        workflow_id: Option<String>,
    },
    #[serde(rename = "workflow:run")]
    Run {
        #[serde(default, rename = "requestId")]
        request_id: Option<String>,
        #[serde(default, rename = "workflowId")]
        workflow_id: Option<String>,
    },
    #[serde(rename = "workflow:publish")]
    Publish {
        #[serde(default, rename = "requestId")]
        request_id: Option<String>,
        #[serde(default, rename = "workflowId")]
        workflow_id: Option<String>,
    },
    #[serde(rename = "workflow:copilot_generate")]
    CopilotGenerate {
        #[serde(default, rename = "requestId")]
        request_id: Option<String>,
        #[serde(default, rename = "workflowId")]
        workflow_id: Option<String>,
        brief: String,
    },
    #[serde(rename = "workflow:select")]
    SelectWorkflow {
        #[serde(default, rename = "requestId")]
        request_id: Option<String>,
        #[serde(rename = "workflowId")]
        workflow_id: String,
    },
    #[serde(rename = "workflow:create_from_template")]
    CreateFromTemplate {
        #[serde(default, rename = "requestId")]
        request_id: Option<String>,
        #[serde(rename = "templateId")]
        template_id: String,
    },
    #[serde(rename = "workflow:update_json")]
    UpdateJson {
        #[serde(default, rename = "requestId")]
        request_id: Option<String>,
        #[serde(default, rename = "workflowId")]
        workflow_id: Option<String>,
        json: String,
    },
    #[serde(rename = "workflow:update_agent")]
    UpdateAgent {
        #[serde(default, rename = "requestId")]
        request_id: Option<String>,
        #[serde(default, rename = "workflowId")]
        workflow_id: Option<String>,
        #[serde(rename = "nodeId")]
        node_id: String,
        update: WorkflowAgentUpdateView,
    },
    #[serde(rename = "node:selected")]
    NodeSelected {
        #[serde(default, rename = "requestId")]
        request_id: Option<String>,
        #[serde(default, rename = "workflowId")]
        workflow_id: Option<String>,
        #[serde(rename = "nodeId")]
        node_id: String,
    },
    #[serde(rename = "edge:created")]
    EdgeCreated {
        #[serde(default, rename = "requestId")]
        request_id: Option<String>,
        #[serde(default, rename = "workflowId")]
        workflow_id: Option<String>,
        #[serde(rename = "sourceNodeId")]
        source_node_id: String,
        #[serde(rename = "targetNodeId")]
        target_node_id: String,
    },
    #[serde(rename = "edge:deleted")]
    EdgeDeleted {
        #[serde(default, rename = "requestId")]
        request_id: Option<String>,
        #[serde(default, rename = "workflowId")]
        workflow_id: Option<String>,
        #[serde(rename = "edgeId")]
        edge_id: String,
    },
    #[serde(rename = "canvas:error")]
    CanvasError {
        #[serde(default, rename = "requestId")]
        request_id: Option<String>,
        #[serde(default, rename = "workflowId")]
        workflow_id: Option<String>,
        message: String,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[allow(dead_code)]
pub(crate) enum WorkflowBuilderHostMessage<'a> {
    #[serde(rename = "workflow:load")]
    WorkflowLoad {
        workflow: Option<&'a CanvasWorkflow>,
    },
    #[serde(rename = "workflows:hydrate")]
    WorkflowsHydrate {
        state: Option<&'a WorkflowBuilderState>,
    },
    #[serde(rename = "workflow:command_result")]
    CommandResult {
        #[serde(rename = "requestId")]
        request_id: String,
        ok: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
}

static WORKFLOW_CANVAS_EVENTS: OnceLock<Mutex<Vec<WorkflowCanvasEvent>>> = OnceLock::new();

pub(crate) fn drain_workflow_canvas_events() -> Vec<WorkflowCanvasEvent> {
    let Some(events) = WORKFLOW_CANVAS_EVENTS.get() else {
        return Vec::new();
    };
    let Ok(mut events) = events.lock() else {
        log::warn!(target: "workflow_webview", "failed to lock workflow canvas event queue");
        return Vec::new();
    };
    events.drain(..).collect()
}

#[cfg(all(
    feature = "workflow-webview",
    any(target_os = "macos", target_os = "windows")
))]
fn push_workflow_canvas_event(event: WorkflowCanvasEvent) {
    let events = WORKFLOW_CANVAS_EVENTS.get_or_init(|| Mutex::new(Vec::new()));
    match events.lock() {
        Ok(mut events) => events.push(event),
        Err(err) => log::warn!(
            target: "workflow_webview",
            "failed to enqueue workflow canvas event: {err}"
        ),
    }
}

#[cfg(all(
    feature = "workflow-webview",
    any(target_os = "macos", target_os = "windows")
))]
fn handle_ipc_message(payload: &str) {
    match serde_json::from_str::<WorkflowWebviewCommand>(payload) {
        Ok(WorkflowWebviewCommand::NodeSelected {
            request_id,
            workflow_id,
            node_id,
        }) => {
            log::info!(
                target: "workflow_webview",
                "node selected from canvas workflow_id={:?} node_id={}",
                workflow_id,
                node_id
            );
            push_workflow_canvas_event(WorkflowCanvasEvent::NodeSelected {
                request_id,
                workflow_id,
                node_id,
            });
        }
        Ok(WorkflowWebviewCommand::EdgeCreated {
            request_id,
            workflow_id,
            source_node_id,
            target_node_id,
        }) => {
            log::info!(
                target: "workflow_webview",
                "edge created from canvas workflow_id={:?} source={} target={}",
                workflow_id,
                source_node_id,
                target_node_id
            );
            push_workflow_canvas_event(WorkflowCanvasEvent::EdgeCreated {
                request_id,
                workflow_id,
                source_node_id,
                target_node_id,
            });
        }
        Ok(WorkflowWebviewCommand::EdgeDeleted {
            request_id,
            workflow_id,
            edge_id,
        }) => {
            log::info!(
                target: "workflow_webview",
                "edge deleted from canvas workflow_id={:?} edge_id={}",
                workflow_id,
                edge_id
            );
            push_workflow_canvas_event(WorkflowCanvasEvent::EdgeDeleted {
                request_id,
                workflow_id,
                edge_id,
            });
        }
        Ok(WorkflowWebviewCommand::WorkflowReady) => {
            log::info!(target: "workflow_webview", "workflow builder ready");
        }
        Ok(WorkflowWebviewCommand::WorkflowLoaded {
            request_id,
            workflow_id,
        }) => {
            log::info!(
                target: "workflow_webview",
                "workflow builder loaded request_id={:?} workflow_id={:?}",
                request_id,
                workflow_id
            );
        }
        Ok(WorkflowWebviewCommand::AddAgent {
            request_id,
            workflow_id,
        }) => {
            log::info!(
                target: "workflow_webview",
                "add agent requested workflow_id={:?}",
                workflow_id
            );
            push_workflow_canvas_event(WorkflowCanvasEvent::AddAgent {
                request_id,
                workflow_id,
            });
        }
        Ok(WorkflowWebviewCommand::Save {
            request_id,
            workflow_id,
        }) => {
            log::info!(
                target: "workflow_webview",
                "save requested workflow_id={:?}",
                workflow_id
            );
            push_workflow_canvas_event(WorkflowCanvasEvent::Save {
                request_id,
                workflow_id,
            });
        }
        Ok(WorkflowWebviewCommand::Run {
            request_id,
            workflow_id,
        }) => {
            log::info!(
                target: "workflow_webview",
                "run requested workflow_id={:?}",
                workflow_id
            );
            push_workflow_canvas_event(WorkflowCanvasEvent::Run {
                request_id,
                workflow_id,
            });
        }
        Ok(WorkflowWebviewCommand::Publish {
            request_id,
            workflow_id,
        }) => {
            log::info!(
                target: "workflow_webview",
                "publish requested workflow_id={:?}",
                workflow_id
            );
            push_workflow_canvas_event(WorkflowCanvasEvent::Publish {
                request_id,
                workflow_id,
            });
        }
        Ok(WorkflowWebviewCommand::CopilotGenerate {
            request_id,
            workflow_id,
            brief,
        }) => {
            log::info!(
                target: "workflow_webview",
                "copilot generate requested workflow_id={:?} brief_len={}",
                workflow_id,
                brief.len()
            );
            push_workflow_canvas_event(WorkflowCanvasEvent::CopilotGenerate {
                request_id,
                workflow_id,
                brief,
            });
        }
        Ok(WorkflowWebviewCommand::SelectWorkflow {
            request_id,
            workflow_id,
        }) => {
            log::info!(
                target: "workflow_webview",
                "workflow select requested workflow_id={}",
                workflow_id
            );
            push_workflow_canvas_event(WorkflowCanvasEvent::SelectWorkflow {
                request_id,
                workflow_id,
            });
        }
        Ok(WorkflowWebviewCommand::CreateFromTemplate {
            request_id,
            template_id,
        }) => {
            log::info!(
                target: "workflow_webview",
                "create workflow from template requested template_id={}",
                template_id
            );
            push_workflow_canvas_event(WorkflowCanvasEvent::CreateFromTemplate {
                request_id,
                template_id,
            });
        }
        Ok(WorkflowWebviewCommand::UpdateJson {
            request_id,
            workflow_id,
            json,
        }) => {
            log::info!(
                target: "workflow_webview",
                "workflow json update requested workflow_id={:?} json_len={}",
                workflow_id,
                json.len()
            );
            push_workflow_canvas_event(WorkflowCanvasEvent::UpdateJson {
                request_id,
                workflow_id,
                json,
            });
        }
        Ok(WorkflowWebviewCommand::UpdateAgent {
            request_id,
            workflow_id,
            node_id,
            update,
        }) => {
            log::info!(
                target: "workflow_webview",
                "agent update requested workflow_id={:?} node_id={}",
                workflow_id,
                node_id
            );
            push_workflow_canvas_event(WorkflowCanvasEvent::UpdateAgent {
                request_id,
                workflow_id,
                node_id,
                update,
            });
        }
        Ok(WorkflowWebviewCommand::CanvasError {
            request_id,
            workflow_id,
            message,
        }) => {
            log::warn!(
                target: "workflow_webview",
                "workflow builder reported error workflow_id={:?}: {}",
                workflow_id,
                message
            );
            push_workflow_canvas_event(WorkflowCanvasEvent::Error {
                request_id,
                workflow_id,
                message,
            });
        }
        Err(err) => {
            log::warn!(
                target: "workflow_webview",
                "failed to parse workflow builder ipc payload: {err}; payload={payload}"
            );
            push_workflow_canvas_event(WorkflowCanvasEvent::Error {
                request_id: None,
                workflow_id: None,
                message: format!("Invalid workflow builder IPC payload: {err}"),
            });
        }
    }
}

pub(crate) struct WorkflowWebviewElement {
    state: Option<WorkflowBuilderState>,
    style: StyleRefinement,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkflowBuilderState {
    workflows: Vec<WorkflowSummaryView>,
    selected_workflow_id: Option<String>,
    workflow: Option<CanvasWorkflow>,
    workflow_json: Option<String>,
    selected_agent: Option<WorkflowAgentInspectorView>,
    edit_state: Option<WorkflowEditStateView>,
    activity: Option<WorkflowActivityView>,
    templates: Vec<WorkflowTemplateView>,
    run_statuses: std::collections::HashMap<String, String>,
    webview_status: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkflowSummaryView {
    id: String,
    name: String,
    description: String,
    status: String,
    version: i64,
    updated_at: String,
    edit_state: WorkflowEditStateView,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkflowEditStateView {
    status: String,
    dirty: bool,
    reason: String,
    last_error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkflowTemplateView {
    id: String,
    name: String,
    description: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkflowActivityView {
    level: String,
    message: String,
}

#[cfg_attr(
    all(
        feature = "workflow-webview",
        any(target_os = "macos", target_os = "windows")
    ),
    derive(Deserialize)
)]
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkflowAgentUpdateView {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) category: String,
    pub(crate) tags: String,
    pub(crate) version: String,
    pub(crate) model_provider: String,
    pub(crate) model_name: String,
    pub(crate) temperature: String,
    pub(crate) max_tokens: String,
    pub(crate) timeout_seconds: String,
    pub(crate) system_prompt: String,
    pub(crate) instructions: String,
    pub(crate) output_format: String,
    pub(crate) output_schema: String,
    pub(crate) summarize_with_mainagent: String,
    pub(crate) skills_json: String,
    pub(crate) mcp_tools_json: String,
    pub(crate) system_tools_json: String,
    pub(crate) coding_runtimes_json: String,
    pub(crate) retry: String,
    pub(crate) settings_timeout_seconds: String,
    pub(crate) human_confirmation: String,
    pub(crate) routing_policy_json: String,
    pub(crate) permissions: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkflowAgentInspectorView {
    workflow_id: String,
    node_id: String,
    node_kind: String,
    routing_mode: String,
    tool_summary: String,
    update: WorkflowAgentUpdateView,
}

impl WorkflowBuilderState {
    pub(crate) fn new(
        workflows: Vec<WorkflowSummaryView>,
        selected_workflow_id: Option<String>,
        workflow: Option<CanvasWorkflow>,
        workflow_json: Option<String>,
        selected_agent: Option<WorkflowAgentInspectorView>,
        edit_state: Option<WorkflowEditStateView>,
        activity: Option<WorkflowActivityView>,
        templates: Vec<WorkflowTemplateView>,
        run_statuses: std::collections::HashMap<String, String>,
    ) -> Self {
        Self {
            workflows,
            selected_workflow_id,
            workflow,
            workflow_json,
            selected_agent,
            edit_state,
            activity,
            templates,
            run_statuses,
            webview_status: webview_status_label().to_string(),
        }
    }
}

impl WorkflowSummaryView {
    pub(crate) fn new(
        summary: &crate::workflows::WorkflowSummary,
        edit_state: WorkflowEditStateView,
    ) -> Self {
        Self {
            id: summary.id.clone(),
            name: summary.name.clone(),
            description: summary.description.clone(),
            status: format!("{:?}", summary.status),
            version: summary.version,
            updated_at: summary.updated_at.clone(),
            edit_state,
        }
    }
}

impl WorkflowEditStateView {
    pub(crate) fn saved() -> Self {
        Self {
            status: "saved".to_string(),
            dirty: false,
            reason: String::new(),
            last_error: None,
        }
    }

    pub(crate) fn from_parts(dirty: bool, reason: String, last_error: Option<String>) -> Self {
        let status = if dirty && last_error.is_some() {
            "save_failed"
        } else if dirty {
            "dirty"
        } else if last_error.is_some() {
            "save_failed"
        } else {
            "saved"
        };

        Self {
            status: status.to_string(),
            dirty,
            reason,
            last_error,
        }
    }
}

impl WorkflowTemplateView {
    pub(crate) fn new(id: &'static str, name: &'static str, description: &'static str) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            description: description.to_string(),
        }
    }
}

impl WorkflowActivityView {
    pub(crate) fn new(level: String, message: String) -> Self {
        Self { level, message }
    }
}

impl WorkflowAgentInspectorView {
    pub(crate) fn new(
        workflow_id: String,
        node_id: String,
        node_kind: String,
        routing_mode: String,
        tool_summary: String,
        update: WorkflowAgentUpdateView,
    ) -> Self {
        Self {
            workflow_id,
            node_id,
            node_kind,
            routing_mode,
            tool_summary,
            update,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CanvasWorkflow {
    id: String,
    name: String,
    description: String,
    nodes: Vec<CanvasNode>,
    edges: Vec<CanvasEdge>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct CanvasNode {
    id: String,
    title: String,
    description: String,
    kind: &'static str,
    badge: String,
    routing_mode: String,
    run_status: String,
    x: f32,
    y: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct CanvasEdge {
    id: String,
    source: String,
    target: String,
    label: String,
    routing_mode: String,
}

impl CanvasWorkflow {
    pub(crate) fn from_definition_with_statuses(
        definition: &WorkflowDefinition,
        statuses: Option<&std::collections::HashMap<String, String>>,
    ) -> Self {
        let nodes = definition
            .nodes
            .iter()
            .enumerate()
            .map(|(index, node)| {
                let column = (index % 4) as f32;
                let row = (index / 4) as f32;
                let routing_mode = node
                    .config
                    .get("routing")
                    .and_then(|routing| routing.get("mode"))
                    .or_else(|| {
                        definition
                            .metadata
                            .get("routing")
                            .and_then(|r| r.get("mode"))
                    })
                    .and_then(|mode| mode.as_str())
                    .unwrap_or("sequential")
                    .to_string();

                CanvasNode {
                    id: node.id.clone(),
                    title: if node.name.is_empty() {
                        node.id.clone()
                    } else {
                        node.name.clone()
                    },
                    description: node
                        .config
                        .get("description")
                        .and_then(|value| value.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    kind: canvas_node_kind(&node.kind),
                    badge: canvas_node_badge(&node.kind),
                    routing_mode,
                    run_status: statuses
                        .and_then(|statuses| statuses.get(&node.id))
                        .cloned()
                        .unwrap_or_else(|| "pending".to_string()),
                    x: 72.0 + column * 280.0,
                    y: 72.0 + row * 190.0,
                }
            })
            .collect();

        let edges = definition
            .edges
            .iter()
            .map(|edge| CanvasEdge {
                id: edge.id.clone(),
                source: edge.from_node_id.clone(),
                target: edge.to_node_id.clone(),
                label: if edge.condition.is_empty() {
                    "always".to_string()
                } else {
                    edge.condition.clone()
                },
                routing_mode: definition
                    .metadata
                    .get("edge_routing")
                    .and_then(|edge_routing| edge_routing.get(&edge.id))
                    .and_then(|routing| routing.get("mode"))
                    .and_then(|mode| mode.as_str())
                    .unwrap_or("graph")
                    .to_string(),
            })
            .collect();

        Self {
            id: definition.id.clone(),
            name: definition.name.clone(),
            description: definition.description.clone(),
            nodes,
            edges,
        }
    }
}

fn canvas_node_kind(kind: &WorkflowNodeKind) -> &'static str {
    match kind {
        WorkflowNodeKind::Agent { .. } => "agent",
        WorkflowNodeKind::Skill { .. } => "skill",
        WorkflowNodeKind::McpTool { .. } => "mcp_tool",
        WorkflowNodeKind::Condition => "condition",
        WorkflowNodeKind::HumanApproval => "human_approval",
        WorkflowNodeKind::Output => "output",
    }
}

fn canvas_node_badge(kind: &WorkflowNodeKind) -> String {
    match kind {
        WorkflowNodeKind::Agent { agent_id } => format!("agent:{agent_id}"),
        WorkflowNodeKind::Skill { skill_id } => format!("skill:{skill_id}"),
        WorkflowNodeKind::McpTool {
            server_name,
            tool_name,
        } => format!("{server_name}:{tool_name}"),
        WorkflowNodeKind::Condition => "condition".to_string(),
        WorkflowNodeKind::HumanApproval => "approval".to_string(),
        WorkflowNodeKind::Output => "output".to_string(),
    }
}

impl IntoElement for WorkflowWebviewElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for WorkflowWebviewElement {
    type RequestLayoutState = Style;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        Some("workflow-builder-webview".into())
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.refine(&self.style);
        let layout_id = window.request_layout(style.clone(), [], cx);
        (layout_id, style)
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        _cx: &mut App,
    ) -> Self::PrepaintState {
        if let Some(id) = id {
            let state_payload = self.state.clone();
            window.with_element_state::<native::WorkflowWebviewState, _>(id, |state, window| {
                let mut state = state.unwrap_or_default();
                state.sync(bounds, state_payload, window);
                ((), state)
            });
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        style: &mut Self::RequestLayoutState,
        _prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        style.paint(bounds, window, cx, |_window, _cx| {});
    }
}

impl Styled for WorkflowWebviewElement {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

#[allow(dead_code)]
fn fallback_html(workflow_count: usize) -> String {
    format!(
        r#"<!doctype html>
<html>
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width,initial-scale=1">
  <style>
    :root {{
      color-scheme: light dark;
      font-family: Inter, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
      background: #f7f8fb;
      color: #182033;
    }}
    body {{
      margin: 0;
      min-height: 100vh;
      background:
        linear-gradient(#e4e8f1 1px, transparent 1px),
        linear-gradient(90deg, #e4e8f1 1px, transparent 1px),
        #f7f8fb;
      background-size: 24px 24px;
      overflow: hidden;
    }}
    .canvas {{
      position: relative;
      width: 100vw;
      height: 100vh;
    }}
    .node {{
      position: absolute;
      width: 184px;
      padding: 14px;
      border: 1px solid #cfd7e6;
      border-radius: 8px;
      background: rgba(255,255,255,.92);
      box-shadow: 0 10px 32px rgba(37, 49, 79, .12);
    }}
    .node strong {{
      display: block;
      font-size: 13px;
      line-height: 1.3;
      margin-bottom: 6px;
    }}
    .node span {{
      display: block;
      color: #607086;
      font-size: 11px;
      line-height: 1.45;
    }}
    .badge {{
      display: inline-block;
      margin-top: 10px;
      padding: 3px 7px;
      border-radius: 999px;
      background: #eef4ff;
      color: #2456b3;
      font-size: 10px;
      font-weight: 700;
      letter-spacing: .02em;
    }}
    .edge {{
      position: absolute;
      left: 274px;
      top: 128px;
      width: 170px;
      height: 1px;
      background: #8ea1bf;
    }}
    .edge::after {{
      content: "";
      position: absolute;
      right: -1px;
      top: -4px;
      border-left: 8px solid #8ea1bf;
      border-top: 4px solid transparent;
      border-bottom: 4px solid transparent;
    }}
  </style>
</head>
<body>
  <main class="canvas">
    <section class="node" style="left:64px;top:78px" data-node-id="agent.main">
      <strong>MainAgent Planner</strong>
      <span>Receives user intent and prepares agent handoff.</span>
      <b class="badge">selector</b>
    </section>
    <div class="edge"></div>
    <section class="node" style="left:448px;top:78px" data-node-id="agent.builder">
      <strong>Builder Agent</strong>
      <span>Executes selected implementation steps.</span>
      <b class="badge">handoff</b>
    </section>
  </main>
  <script>
    const post = (payload) => {{
      if (window.ipc && window.ipc.postMessage) {{
        window.ipc.postMessage(JSON.stringify(payload));
      }}
    }};
    window.addEventListener("DOMContentLoaded", () => {{
      post({{type:"workflow:ready", workflowCount:{workflow_count}}});
    }});
    window.addEventListener("one-message", (event) => {{
      if (event.detail && event.detail.type === "workflow:load") {{
        document.body.dataset.workflowCount = String(event.detail.workflowCount ?? "");
        post({{type:"workflow:loaded", workflowCount:event.detail.workflowCount}});
      }}
    }});
    document.querySelectorAll("[data-node-id]").forEach((node) => {{
      node.addEventListener("click", () => {{
        post({{type:"node:selected", nodeId:node.dataset.nodeId}});
      }});
    }});
  </script>
</body>
</html>"#
    )
}

#[cfg(all(
    feature = "workflow-webview",
    any(target_os = "macos", target_os = "windows")
))]
mod native {
    use super::*;
    use wry::{Rect, WebView};

    #[derive(Default)]
    pub(crate) struct WorkflowWebviewState {
        webview: Option<WebView>,
        last_bounds: Option<Bounds<Pixels>>,
        last_state_json: Option<String>,
    }

    pub(crate) fn status_label() -> &'static str {
        "WebView builder"
    }

    impl WorkflowWebviewState {
        pub(crate) fn sync(
            &mut self,
            bounds: Bounds<Pixels>,
            state: Option<WorkflowBuilderState>,
            window: &mut Window,
        ) {
            let state_json = serialize_builder_state_message(state.as_ref());
            let workflow_count = state
                .as_ref()
                .and_then(|state| state.workflow.as_ref())
                .map(|workflow| workflow.nodes.len())
                .unwrap_or(0);

            if self.webview.is_none() || self.last_state_json.as_deref() != Some(&state_json) {
                let html = super::fallback_html(workflow_count);
                match build_webview(window, &html, &state_json, workflow_count) {
                    Ok(webview) => {
                        log::info!(
                            target: "workflow_webview",
                            "initialized workflow builder WebView with workflow_count={workflow_count}"
                        );
                        self.webview = Some(webview);
                        self.last_bounds = None;
                        self.last_state_json = Some(state_json);
                    }
                    Err(err) => {
                        log::error!(target: "workflow_webview", "failed to initialize WebView: {err}");
                        return;
                    }
                }
            }

            if self.last_bounds != Some(bounds) {
                if let Some(webview) = self.webview.as_ref() {
                    if let Err(err) = webview.set_bounds(bounds_to_rect(bounds)) {
                        log::warn!(target: "workflow_webview", "failed to resize WebView: {err}");
                    } else {
                        self.last_bounds = Some(bounds);
                    }
                }
            }
        }
    }

    fn build_webview(
        window: &mut Window,
        html: &str,
        state_json: &str,
        workflow_count: usize,
    ) -> wry::Result<WebView> {
        let builder = wry::WebViewBuilder::new().with_ipc_handler(|request| {
            let payload = request.body();
            log::info!(target: "workflow_webview", "ipc event: {}", payload);
            handle_ipc_message(payload);
        });
        let builder = if let Some(url) = workflow_canvas_dist_url() {
            log::info!(target: "workflow_webview", "loading workflow builder dist: {url}");
            builder.with_url(url)
        } else {
            log::info!(target: "workflow_webview", "loading embedded workflow builder fallback html");
            builder.with_html(html)
        };
        let webview = builder.build_as_child(window)?;

        let script = format!(
            "window.dispatchEvent(new CustomEvent('one-message', {{ detail: {} }}));",
            state_json
        );
        if let Err(err) = webview.evaluate_script(&script) {
            log::warn!(target: "workflow_webview", "failed to send workflows:hydrate: {err}");
        } else {
            log::info!(
                target: "workflow_webview",
                "sent workflows:hydrate to WebView with workflow_count={workflow_count}"
            );
        }

        Ok(webview)
    }

    fn serialize_builder_state_message(state: Option<&WorkflowBuilderState>) -> String {
        serde_json::to_string(&WorkflowBuilderHostMessage::WorkflowsHydrate { state })
            .unwrap_or_else(|_| r#"{"type":"workflows:hydrate","state":null}"#.to_string())
    }

    fn workflow_canvas_dist_url() -> Option<String> {
        let index = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("web")
            .join("workflow-canvas")
            .join("dist")
            .join("index.html");
        if index.exists() {
            Some(format!("file://{}", index.to_string_lossy()))
        } else {
            None
        }
    }

    fn bounds_to_rect(bounds: Bounds<Pixels>) -> Rect {
        Rect {
            position: wry::dpi::Position::Logical(wry::dpi::LogicalPosition::new(
                f64::from(bounds.origin.x),
                f64::from(bounds.origin.y),
            )),
            size: wry::dpi::Size::Logical(wry::dpi::LogicalSize::new(
                f64::from(bounds.size.width),
                f64::from(bounds.size.height),
            )),
        }
    }
}

#[cfg(not(all(
    feature = "workflow-webview",
    any(target_os = "macos", target_os = "windows")
)))]
mod native {
    use super::*;

    #[derive(Default)]
    pub(crate) struct WorkflowWebviewState {
        logged: bool,
    }

    pub(crate) fn status_label() -> &'static str {
        if cfg!(feature = "workflow-webview") {
            "WebView unsupported"
        } else {
            "WebView disabled"
        }
    }

    impl WorkflowWebviewState {
        pub(crate) fn sync(
            &mut self,
            bounds: Bounds<Pixels>,
            state: Option<WorkflowBuilderState>,
            _window: &mut Window,
        ) {
            if !self.logged {
                let workflow_count = state
                    .as_ref()
                    .and_then(|state| state.workflow.as_ref())
                    .map(|workflow| workflow.nodes.len())
                    .unwrap_or(0);
                log::info!(
                    target: "workflow_webview",
                    "workflow builder WebView fallback active; bounds={:?}, workflow_count={workflow_count}",
                    bounds
                );
                self.logged = true;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn fallback_html_contains_minimum_ipc_events() {
        let html = super::fallback_html(3);

        assert!(html.contains("workflow:ready"));
        assert!(html.contains("workflow:load"));
        assert!(html.contains("workflow:loaded"));
        assert!(html.contains("node:selected"));
        assert!(html.contains("workflowCount:3"));
    }
}
