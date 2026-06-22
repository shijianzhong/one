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
import type { CanvasNodeData, CanvasWorkflow } from "./types";

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

export function App() {
  const [workflow, setWorkflow] = useState<CanvasWorkflow | null>(null);
  const [nodes, setNodes, onNodesChange] = useNodesState<WorkflowFlowNode>([]);
  const [edges, setEdges, onEdgesChange] = useEdgesState<Edge>([]);
  const workflowIdRef = useRef<string | null>(null);

  useEffect(() => {
    sendToHost({ type: "workflow:ready" });
    const unsubscribeHostMessages = subscribeHostMessages((message) => {
      workflowIdRef.current = message.workflow?.id ?? null;
      setWorkflow(message.workflow);
      setNodes(toFlowNodes(message.workflow));
      setEdges(toFlowEdges(message.workflow));
      sendToHost({
        type: "workflow:loaded",
        workflowId: message.workflow?.id ?? null
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
  const subtitle = useMemo(() => {
    if (!workflow) {
      return "No workflow selected";
    }

    return `${workflow.nodes.length} agents / ${workflow.edges.length} routes`;
  }, [workflow]);
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

  return (
    <main className="workflow-shell">
      <header className="workflow-header">
        <div>
          <span>Workflow Canvas</span>
          <strong>{workflow?.name ?? "Empty workflow"}</strong>
        </div>
        <p>{subtitle}</p>
      </header>
      <section className="workflow-canvas">
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
      </section>
    </main>
  );
}
