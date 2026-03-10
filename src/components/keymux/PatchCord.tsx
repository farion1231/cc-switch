import type { GraphEdge, GraphNode, EdgeType } from "./types";

const EDGE_COLORS: Record<EdgeType, string> = {
  auth: "#ef4444",
  data: "#3b82f6",
  route: "#10b981",
  quota: "#06b6d4",
  control: "#f59e0b",
};

interface PatchCordProps {
  edge: GraphEdge;
  nodes: GraphNode[];
  isSelected: boolean;
  onClick: () => void;
}

export function PatchCord({
  edge,
  nodes,
  isSelected,
  onClick,
}: PatchCordProps) {
  const sourceNode = nodes.find((n) => n.id === edge.source);
  const targetNode = nodes.find((n) => n.id === edge.target);

  if (!sourceNode || !targetNode) return null;

  const x1 = sourceNode.position.x + 192;
  const y1 = sourceNode.position.y + 30;
  const x2 = targetNode.position.x;
  const y2 = targetNode.position.y + 30;

  const controlOffset = Math.abs(x2 - x1) * 0.5;

  const path = `M ${x1} ${y1} C ${x1 + controlOffset} ${y1}, ${x2 - controlOffset} ${y2}, ${x2} ${y2}`;

  const color = EDGE_COLORS[edge.type];
  const statusColor =
    edge.data?.status === "active"
      ? "#10b981"
      : edge.data?.status === "error"
        ? "#ef4444"
        : color;

  return (
    <g className="cursor-pointer" onClick={onClick}>
      <path
        d={path}
        fill="none"
        stroke={isSelected ? statusColor : "#888"}
        strokeWidth={isSelected ? 3 : 2}
        strokeDasharray={edge.data?.status === "idle" ? "5,5" : undefined}
        markerEnd="url(#arrowhead)"
        className="transition-colors"
        style={{ pointerEvents: "stroke" }}
      />
      {edge.animated && (
        <path
          d={path}
          fill="none"
          stroke={statusColor}
          strokeWidth={2}
          strokeDasharray="10,10"
          className="animate-dash"
        />
      )}
    </g>
  );
}
