import {
  Background,
  BackgroundVariant,
  Controls,
  MiniMap,
  ReactFlow,
  type Connection,
  type Edge,
  type EdgeChange,
  type Node,
  type NodeMouseHandler,
  useEdgesState,
  useNodesState
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import { useEffect, useMemo, useRef, useState } from "react";
import { AgentNode } from "./AgentNode";
import { sendToHost, subscribeHostMessages } from "./bridge";
import type {
  CanvasNodeData,
  CanvasWorkflow,
  WorkflowAgentUpdateView,
  WorkflowBuilderState
} from "./types";

const nodeTypes = {
  agent: AgentNode
};

type WorkflowFlowNode = Node<CanvasNodeData, "agent">;

function toFlowNodes(workflow: CanvasWorkflow | null): WorkflowFlowNode[] {
  if (!workflow) {
    return [];
  }

  return workflow.nodes.map((node) => ({
    id: node.id,
    type: "agent",
    position: { x: node.x, y: node.y },
    data: node
  }));
}

function toFlowEdges(workflow: CanvasWorkflow | null): Edge[] {
  if (!workflow) {
    return [];
  }

  return workflow.edges.map((edge) => ({
    id: edge.id,
    source: edge.source,
    target: edge.target,
    label: edge.label,
    type: "smoothstep",
    animated: edge.routingMode === "handoff" || edge.routingMode === "selector",
    className: `workflow-edge routing-${edge.routingMode}`
  }));
}

type AgentFieldKey = keyof WorkflowAgentUpdateView;

const agentFieldGroups: Array<{
  title: string;
  fields: Array<{ key: AgentFieldKey; label: string; multiline?: boolean }>;
}> = [
  {
    title: "Basic",
    fields: [
      { key: "name", label: "Name" },
      { key: "description", label: "Description", multiline: true },
      { key: "category", label: "Category" },
      { key: "tags", label: "Tags" },
      { key: "version", label: "Version" }
    ]
  },
  {
    title: "Model",
    fields: [
      { key: "modelProvider", label: "Provider" },
      { key: "modelName", label: "Model" },
      { key: "temperature", label: "Temperature" },
      { key: "maxTokens", label: "Max tokens" },
      { key: "timeoutSeconds", label: "Timeout seconds" }
    ]
  },
  {
    title: "Prompt",
    fields: [
      { key: "systemPrompt", label: "System", multiline: true },
      { key: "instructions", label: "Instructions", multiline: true }
    ]
  },
  {
    title: "Output",
    fields: [
      { key: "outputFormat", label: "Format" },
      { key: "outputSchema", label: "Schema JSON", multiline: true },
      { key: "summarizeWithMainagent", label: "Summarize with MainAgent" }
    ]
  },
  {
    title: "Tools",
    fields: [
      { key: "skillsJson", label: "Skills JSON", multiline: true },
      { key: "mcpToolsJson", label: "MCP tools JSON", multiline: true },
      { key: "systemToolsJson", label: "System tools JSON", multiline: true },
      { key: "codingRuntimesJson", label: "Coding runtimes JSON", multiline: true }
    ]
  },
  {
    title: "Settings",
    fields: [
      { key: "retry", label: "Retry" },
      { key: "settingsTimeoutSeconds", label: "Timeout seconds" },
      { key: "humanConfirmation", label: "Human confirmation" },
      { key: "routingPolicyJson", label: "Routing policy JSON", multiline: true },
      { key: "permissions", label: "Permissions" }
    ]
  }
];

export function App() {
  const [builderState, setBuilderState] = useState<WorkflowBuilderState | null>(null);
  const [workflow, setWorkflow] = useState<CanvasWorkflow | null>(null);
  const [copilotBrief, setCopilotBrief] = useState("");
  const [agentForm, setAgentForm] = useState<WorkflowAgentUpdateView | null>(null);
  const [workflowJson, setWorkflowJson] = useState("");
  const [nodes, setNodes, onNodesChange] = useNodesState<WorkflowFlowNode>([]);
  const [edges, setEdges, onEdgesChange] = useEdgesState<Edge>([]);
  const workflowIdRef = useRef<string | null>(null);

  useEffect(() => {
    sendToHost({ type: "workflow:ready" });
    const unsubscribeHostMessages = subscribeHostMessages((message) => {
      if (message.type === "workflow:command_result") {
        return;
      }

      const nextState =
        message.type === "workflows:hydrate" ? message.state : null;
      const nextWorkflow =
        message.type === "workflows:hydrate"
          ? message.state?.workflow ?? null
          : message.workflow;

      if (message.type === "workflows:hydrate") {
        setBuilderState(nextState);
      }

      workflowIdRef.current = nextWorkflow?.id ?? null;
      setWorkflow(nextWorkflow);
      setNodes(toFlowNodes(nextWorkflow));
      setEdges(toFlowEdges(nextWorkflow));
      sendToHost({
        type: "workflow:loaded",
        workflowId: nextWorkflow?.id ?? null
      });
    });

    const reportError = (message: string) => {
      sendToHost({
        type: "canvas:error",
        workflowId: workflowIdRef.current,
        message
      });
    };
    const handleError = (event: ErrorEvent) => {
      reportError(event.message || "Unknown canvas error");
    };
    const handleUnhandledRejection = (event: PromiseRejectionEvent) => {
      const reason = event.reason;
      reportError(reason instanceof Error ? reason.message : String(reason));
    };
    window.addEventListener("error", handleError);
    window.addEventListener("unhandledrejection", handleUnhandledRejection);

    return () => {
      unsubscribeHostMessages();
      window.removeEventListener("error", handleError);
      window.removeEventListener("unhandledrejection", handleUnhandledRejection);
    };
  }, [setEdges, setNodes]);

  const showMiniMap = nodes.length > 6;
  const workflowList = builderState?.workflows ?? [];
  const selectedWorkflowId = builderState?.selectedWorkflowId ?? workflow?.id ?? null;
  const selectedAgent = builderState?.selectedAgent ?? null;
  const editState = builderState?.editState ?? null;
  const activity = builderState?.activity ?? null;
  const runStatusEntries = Object.entries(builderState?.runStatuses ?? {});
  const canUseWorkflowCommand = Boolean(selectedWorkflowId);
  useEffect(() => {
    setAgentForm(selectedAgent ? { ...selectedAgent.update } : null);
  }, [selectedAgent?.workflowId, selectedAgent?.nodeId, selectedAgent?.update]);
  useEffect(() => {
    setWorkflowJson(builderState?.workflowJson ?? "");
  }, [builderState?.selectedWorkflowId, builderState?.workflowJson]);
  const subtitle = useMemo(() => {
    if (!workflow) {
      return builderState
        ? `${workflowList.length} draft workflows`
        : "No workflow selected";
    }

    return `${workflow.nodes.length} agents / ${workflow.edges.length} routes`;
  }, [builderState, workflow, workflowList.length]);
  const handleNodeClick: NodeMouseHandler<WorkflowFlowNode> = (_, node) => {
    sendToHost({
      type: "node:selected",
      workflowId: workflow?.id ?? null,
      nodeId: node.id
    });
  };
  const handleConnect = (connection: Connection) => {
    if (!connection.source || !connection.target) {
      return;
    }
    sendToHost({
      type: "edge:created",
      workflowId: workflow?.id ?? null,
      sourceNodeId: connection.source,
      targetNodeId: connection.target
    });
  };
  const handleEdgesChange = (changes: EdgeChange[]) => {
    onEdgesChange(changes);
    for (const change of changes) {
      if (change.type === "remove") {
        sendToHost({
          type: "edge:deleted",
          workflowId: workflow?.id ?? null,
          edgeId: change.id
        });
      }
    }
  };
  const sendWorkflowCommand = (
    type:
      | "workflow:add_agent"
      | "workflow:save"
      | "workflow:run"
      | "workflow:publish"
  ) => {
    sendToHost({
      type,
      workflowId: selectedWorkflowId
    });
  };
  const handleCopilotGenerate = () => {
    sendToHost({
      type: "workflow:copilot_generate",
      workflowId: selectedWorkflowId,
      brief: copilotBrief
    });
  };
  const handleWorkflowSelect = (workflowId: string) => {
    sendToHost({
      type: "workflow:select",
      workflowId
    });
  };
  const handleTemplateCreate = (templateId: string) => {
    sendToHost({
      type: "workflow:create_from_template",
      templateId
    });
  };
  const updateAgentField = (key: AgentFieldKey, value: string) => {
    setAgentForm((current) => (current ? { ...current, [key]: value } : current));
  };
  const handleAgentSave = () => {
    if (!selectedAgent || !agentForm) {
      return;
    }
    sendToHost({
      type: "workflow:update_agent",
      workflowId: selectedAgent.workflowId,
      nodeId: selectedAgent.nodeId,
      update: agentForm
    });
  };
  const handleWorkflowJsonSave = () => {
    if (!selectedWorkflowId) {
      return;
    }
    sendToHost({
      type: "workflow:update_json",
      workflowId: selectedWorkflowId,
      json: workflowJson
    });
  };

  return (
    <main className="workflow-shell">
      <header className="workflow-header">
        <div>
          <span>Workflow Builder</span>
          <strong>{workflow?.name ?? "Empty workflow"}</strong>
        </div>
        <p>{subtitle}</p>
        <nav className="workflow-toolbar" aria-label="Workflow actions">
          <button
            type="button"
            disabled={!canUseWorkflowCommand}
            onClick={() => sendWorkflowCommand("workflow:add_agent")}
          >
            Add Agent
          </button>
          <button
            type="button"
            disabled={!canUseWorkflowCommand}
            onClick={() => sendWorkflowCommand("workflow:save")}
          >
            Save
          </button>
          <button
            type="button"
            disabled={!canUseWorkflowCommand}
            onClick={() => sendWorkflowCommand("workflow:run")}
          >
            Run
          </button>
          <button
            type="button"
            disabled={!canUseWorkflowCommand}
            onClick={() => sendWorkflowCommand("workflow:publish")}
          >
            Publish
          </button>
        </nav>
      </header>
      <section className="workflow-builder-body">
        <aside className="workflow-sidebar">
          <div className="sidebar-section-title">AI Copilot</div>
          <div className="copilot-panel">
            <textarea
              value={copilotBrief}
              placeholder="Describe the workflow to generate"
              onChange={(event) => setCopilotBrief(event.target.value)}
            />
            <button
              type="button"
              disabled={copilotBrief.trim().length === 0}
              onClick={handleCopilotGenerate}
            >
              Generate
            </button>
          </div>
          <div className="sidebar-section-title">Drafts</div>
          {workflowList.length === 0 ? (
            <div className="sidebar-empty">No draft workflows</div>
          ) : (
            <div className="workflow-list">
              {workflowList.map((item) => (
                <button
                  key={item.id}
                  className={`workflow-list-item ${
                    item.id === selectedWorkflowId ? "selected" : ""
                  }`}
                  type="button"
                  title={item.description || item.id}
                  onClick={() => handleWorkflowSelect(item.id)}
                >
                  <span>{item.name || item.id}</span>
                  <small>
                    v{item.version} · {item.editState.status}
                  </small>
                </button>
              ))}
            </div>
          )}
          <div className="sidebar-section-title">Templates</div>
          <div className="template-list">
            {(builderState?.templates ?? []).map((template) => (
              <button
                key={template.id}
                className="template-item"
                type="button"
                onClick={() => handleTemplateCreate(template.id)}
              >
                <span>{template.name}</span>
                <small>{template.description}</small>
              </button>
            ))}
          </div>
          <div className="sidebar-section-title">Run Status</div>
          {runStatusEntries.length === 0 ? (
            <div className="sidebar-empty">No active run state</div>
          ) : (
            <div className="run-status-list">
              {runStatusEntries.map(([nodeId, status]) => (
                <div key={nodeId} className="run-status-item">
                  <span>{nodeId}</span>
                  <small>{status}</small>
                </div>
              ))}
            </div>
          )}
        </aside>
        <div className="workflow-canvas">
          {nodes.length === 0 ? (
            <div className="empty-state">
              <strong>Add an agent to start</strong>
              <span>Workflow nodes and routing edges will appear here.</span>
            </div>
          ) : (
            <ReactFlow
              nodes={nodes}
              edges={edges}
              nodeTypes={nodeTypes}
              onNodesChange={onNodesChange}
              onEdgesChange={handleEdgesChange}
              onNodeClick={handleNodeClick}
              onConnect={handleConnect}
              fitView
              fitViewOptions={{ padding: 0.24 }}
              minZoom={0.25}
              maxZoom={1.8}
            >
              <Background
                color="#dbe2ec"
                gap={24}
                size={1}
                variant={BackgroundVariant.Lines}
              />
              <Controls showInteractive={false} />
              {showMiniMap ? <MiniMap pannable zoomable /> : null}
            </ReactFlow>
          )}
        </div>
        <aside className="workflow-inspector">
          <section className="inspector-section workflow-status-section">
            <h2>Workflow Status</h2>
            {editState ? (
              <div className={`workflow-status status-${editState.status}`}>
                <strong>{editState.status}</strong>
                <span>{editState.lastError ?? editState.reason}</span>
              </div>
            ) : (
              <div className="workflow-status">
                <strong>No workflow selected</strong>
                <span>Create or select a draft workflow.</span>
              </div>
            )}
            {activity ? (
              <div className={`workflow-activity activity-${activity.level}`}>
                <strong>{activity.level}</strong>
                <span>{activity.message}</span>
              </div>
            ) : null}
            {runStatusEntries.length > 0 ? (
              <div className="workflow-run-summary">
                {runStatusEntries.map(([nodeId, status]) => (
                  <span key={nodeId}>
                    {nodeId}: {status}
                  </span>
                ))}
              </div>
            ) : null}
          </section>
          {selectedAgent && agentForm ? (
            <>
              <div className="inspector-heading">
                <span>Agent Inspector</span>
                <strong>{agentForm.name || selectedAgent.nodeId}</strong>
                <small>{selectedAgent.nodeKind}</small>
              </div>
              <div className="inspector-meta">
                <div>
                  <span>Node</span>
                  <strong>{selectedAgent.nodeId}</strong>
                </div>
                <div>
                  <span>Routing</span>
                  <strong>{selectedAgent.routingMode}</strong>
                </div>
                <div>
                  <span>Tools</span>
                  <strong>{selectedAgent.toolSummary}</strong>
                </div>
              </div>
              <div className="inspector-form">
                {agentFieldGroups.map((group) => (
                  <section key={group.title} className="inspector-section">
                    <h2>{group.title}</h2>
                    {group.fields.map((field) => (
                      <label key={field.key} className="inspector-field">
                        <span>{field.label}</span>
                        {field.multiline ? (
                          <textarea
                            value={agentForm[field.key]}
                            onChange={(event) =>
                              updateAgentField(field.key, event.target.value)
                            }
                          />
                        ) : (
                          <input
                            value={agentForm[field.key]}
                            onChange={(event) =>
                              updateAgentField(field.key, event.target.value)
                            }
                          />
                        )}
                      </label>
                    ))}
                  </section>
                ))}
              </div>
              <button className="inspector-save" type="button" onClick={handleAgentSave}>
                Save Agent
              </button>
            </>
          ) : (
            <div className="inspector-empty">
              <strong>Workflow Inspector</strong>
              <span>Select an Agent node to edit its workflow-local configuration.</span>
            </div>
          )}
          {selectedWorkflowId ? (
            <section className="inspector-section workflow-json-section">
              <h2>Advanced JSON</h2>
              <label className="inspector-field">
                <span>Workflow Definition</span>
                <textarea
                  className="workflow-json-editor"
                  spellCheck={false}
                  value={workflowJson}
                  onChange={(event) => setWorkflowJson(event.target.value)}
                />
              </label>
              <button
                className="inspector-save"
                type="button"
                disabled={workflowJson.trim().length === 0}
                onClick={handleWorkflowJsonSave}
              >
                Save JSON
              </button>
            </section>
          ) : null}
        </aside>
      </section>
    </main>
  );
}
