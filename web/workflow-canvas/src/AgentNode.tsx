import type { Node, NodeProps } from "@xyflow/react";
import { Handle, Position } from "@xyflow/react";
import type { CanvasNodeData } from "./types";

type AgentFlowNode = Node<CanvasNodeData, "agent">;

export function AgentNode({ data, selected }: NodeProps<AgentFlowNode>) {
  return (
    <section className={`agent-node ${selected ? "is-selected" : ""}`}>
      <Handle type="target" position={Position.Left} className="node-handle" />
      <div className="agent-node__top">
        <span className={`agent-node__kind kind-${data.kind}`}>{data.badge}</span>
        <span className={`agent-node__routing routing-${data.routingMode}`}>
          {data.routingMode}
        </span>
      </div>
      <strong>{data.title}</strong>
      <span className={`agent-node__status status-${data.runStatus}`}>
        {data.runStatus}
      </span>
      <p>{data.description || "No description yet."}</p>
      <Handle type="source" position={Position.Right} className="node-handle" />
    </section>
  );
}
