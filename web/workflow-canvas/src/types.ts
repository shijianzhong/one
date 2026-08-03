export type CanvasNodeKind =
  | "agent"
  | "skill"
  | "mcp_tool"
  | "condition"
  | "human_approval"
  | "output";

export type CanvasRoutingMode =
  | "sequential"
  | "parallel"
  | "selector"
  | "handoff"
  | "graph"
  | "unknown";

export interface CanvasNode extends Record<string, unknown> {
  id: string;
  title: string;
  description: string;
  kind: CanvasNodeKind;
  badge: string;
  routingMode: CanvasRoutingMode;
  runStatus: string;
  x: number;
  y: number;
}

export type CanvasNodeData = CanvasNode;

export interface CanvasEdge {
  id: string;
  source: string;
  target: string;
  label: string;
  routingMode: CanvasRoutingMode;
}

export interface CanvasWorkflow {
  id: string;
  name: string;
  description: string;
  nodes: CanvasNode[];
  edges: CanvasEdge[];
}

export interface WorkflowEditStateView {
  status: "saved" | "dirty" | "save_failed" | string;
  dirty: boolean;
  reason: string;
  lastError?: string | null;
}

export interface WorkflowSummaryView {
  id: string;
  name: string;
  description: string;
  status: string;
  version: number;
  updatedAt: string;
  editState: WorkflowEditStateView;
}

export interface WorkflowTemplateView {
  id: string;
  name: string;
  description: string;
}

export interface WorkflowActivityView {
  level: "pending" | "success" | "error" | "info" | string;
  message: string;
}

export interface WorkflowAgentUpdateView {
  name: string;
  description: string;
  category: string;
  tags: string;
  version: string;
  modelProvider: string;
  modelName: string;
  temperature: string;
  maxTokens: string;
  timeoutSeconds: string;
  systemPrompt: string;
  instructions: string;
  outputFormat: string;
  outputSchema: string;
  summarizeWithMainagent: string;
  skillsJson: string;
  mcpToolsJson: string;
  systemToolsJson: string;
  codingRuntimesJson: string;
  retry: string;
  settingsTimeoutSeconds: string;
  humanConfirmation: string;
  routingPolicyJson: string;
  permissions: string;
}

export interface WorkflowAgentInspectorView {
  workflowId: string;
  nodeId: string;
  nodeKind: string;
  routingMode: string;
  toolSummary: string;
  update: WorkflowAgentUpdateView;
}

export interface WorkflowBuilderState {
  workflows: WorkflowSummaryView[];
  selectedWorkflowId: string | null;
  workflow: CanvasWorkflow | null;
  workflowJson: string | null;
  selectedAgent: WorkflowAgentInspectorView | null;
  editState: WorkflowEditStateView | null;
  activity: WorkflowActivityView | null;
  templates: WorkflowTemplateView[];
  runStatuses: Record<string, string>;
  webviewStatus: string;
}

export interface WorkflowLoadMessage {
  type: "workflow:load";
  workflow: CanvasWorkflow | null;
}

export interface WorkflowsHydrateMessage {
  type: "workflows:hydrate";
  state: WorkflowBuilderState | null;
}

export interface WorkflowCommandResultMessage {
  type: "workflow:command_result";
  requestId: string;
  ok: boolean;
  message?: string;
  error?: string;
}

export type HostMessage =
  | WorkflowLoadMessage
  | WorkflowsHydrateMessage
  | WorkflowCommandResultMessage;

export interface NodeSelectedMessage {
  type: "node:selected";
  requestId?: string;
  workflowId: string | null;
  nodeId: string;
}

export interface EdgeCreatedMessage {
  type: "edge:created";
  requestId?: string;
  workflowId: string | null;
  sourceNodeId: string;
  targetNodeId: string;
}

export interface EdgeDeletedMessage {
  type: "edge:deleted";
  requestId?: string;
  workflowId: string | null;
  edgeId: string;
}

export interface WorkflowReadyMessage {
  type: "workflow:ready";
  requestId?: string;
}

export interface WorkflowLoadedMessage {
  type: "workflow:loaded";
  requestId?: string;
  workflowId: string | null;
}

export interface CanvasErrorMessage {
  type: "canvas:error";
  requestId?: string;
  workflowId: string | null;
  message: string;
}

export interface WorkflowAddAgentMessage {
  type: "workflow:add_agent";
  requestId?: string;
  workflowId: string | null;
}

export interface WorkflowSaveMessage {
  type: "workflow:save";
  requestId?: string;
  workflowId: string | null;
}

export interface WorkflowRunMessage {
  type: "workflow:run";
  requestId?: string;
  workflowId: string | null;
}

export interface WorkflowPublishMessage {
  type: "workflow:publish";
  requestId?: string;
  workflowId: string | null;
}

export interface WorkflowCopilotGenerateMessage {
  type: "workflow:copilot_generate";
  requestId?: string;
  workflowId: string | null;
  brief: string;
}

export interface WorkflowSelectMessage {
  type: "workflow:select";
  requestId?: string;
  workflowId: string;
}

export interface WorkflowCreateFromTemplateMessage {
  type: "workflow:create_from_template";
  requestId?: string;
  templateId: string;
}

export interface WorkflowUpdateJsonMessage {
  type: "workflow:update_json";
  requestId?: string;
  workflowId: string | null;
  json: string;
}

export interface WorkflowUpdateAgentMessage {
  type: "workflow:update_agent";
  requestId?: string;
  workflowId: string | null;
  nodeId: string;
  update: WorkflowAgentUpdateView;
}

export type CanvasMessage =
  | NodeSelectedMessage
  | EdgeCreatedMessage
  | EdgeDeletedMessage
  | WorkflowReadyMessage
  | WorkflowLoadedMessage
  | CanvasErrorMessage
  | WorkflowAddAgentMessage
  | WorkflowSaveMessage
  | WorkflowRunMessage
  | WorkflowPublishMessage
  | WorkflowCopilotGenerateMessage
  | WorkflowSelectMessage
  | WorkflowCreateFromTemplateMessage
  | WorkflowUpdateJsonMessage
  | WorkflowUpdateAgentMessage;
