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

export interface WorkflowLoadMessage {
  type: "workflow:load";
  workflow: CanvasWorkflow | null;
}

export type HostMessage = WorkflowLoadMessage;

export interface NodeSelectedMessage {
  type: "node:selected";
  workflowId: string | null;
  nodeId: string;
}

export interface EdgeCreatedMessage {
  type: "edge:created";
  workflowId: string | null;
  sourceNodeId: string;
  targetNodeId: string;
}

export interface EdgeDeletedMessage {
  type: "edge:deleted";
  workflowId: string | null;
  edgeId: string;
}

export interface WorkflowReadyMessage {
  type: "workflow:ready";
}

export interface WorkflowLoadedMessage {
  type: "workflow:loaded";
  workflowId: string | null;
}

export interface CanvasErrorMessage {
  type: "canvas:error";
  workflowId: string | null;
  message: string;
}

export type CanvasMessage =
  | NodeSelectedMessage
  | EdgeCreatedMessage
  | EdgeDeletedMessage
  | WorkflowReadyMessage
  | WorkflowLoadedMessage
  | CanvasErrorMessage;
