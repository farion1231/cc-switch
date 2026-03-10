import { motion } from "framer-motion";
import {
  Server,
  Cpu,
  Users,
  Wifi,
  Key,
  Gauge,
  TrendingUp,
  Wrench,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { Badge } from "@/components/ui/badge";
import type { GraphNode, DimensionalEntityType } from "./types";

const ENTITY_ICONS: Record<DimensionalEntityType, typeof Server> = {
  provider: Server,
  model: Cpu,
  agent: Users,
  tunnel: Wifi,
  pubkey: Key,
  quota: Gauge,
  ranking: TrendingUp,
  tool: Wrench,
};

const ENTITY_COLORS: Record<DimensionalEntityType, string> = {
  provider: "#3b82f6",
  model: "#8b5cf6",
  agent: "#10b981",
  tunnel: "#f59e0b",
  pubkey: "#ef4444",
  quota: "#06b6d4",
  ranking: "#ec4899",
  tool: "#84cc16",
};

interface EntityNodeProps {
  node: GraphNode;
  isSelected: boolean;
  onClick: () => void;
  onHandleMouseDown: (nodeId: string, handleId: string) => void;
  onHandleMouseUp: (nodeId: string, handleId: string) => void;
  connectingFrom: { nodeId: string; handleId: string } | null;
}

export function EntityNode({
  node,
  isSelected,
  onClick,
  onHandleMouseDown,
  onHandleMouseUp,
  connectingFrom,
}: EntityNodeProps) {
  const Icon = ENTITY_ICONS[node.type];
  const color = ENTITY_COLORS[node.type];
  const name = "name" in node.data ? node.data.name : node.id;

  return (
    <motion.div
      initial={{ opacity: 0, scale: 0.8 }}
      animate={{ opacity: 1, scale: 1 }}
      exit={{ opacity: 0, scale: 0.8 }}
      className={cn(
        "absolute cursor-pointer select-none",
        "w-48 rounded-lg border-2 bg-background shadow-lg",
        "transition-shadow duration-200",
        isSelected && "ring-2 ring-primary ring-offset-2",
      )}
      style={{
        left: node.position.x,
        top: node.position.y,
        borderColor: color,
      }}
      onClick={onClick}
    >
      <div
        className="px-3 py-2 border-b flex items-center gap-2"
        style={{ borderColor: color }}
      >
        <div
          className="w-6 h-6 rounded flex items-center justify-center"
          style={{ backgroundColor: color + "20" }}
        >
          <Icon className="w-4 h-4" style={{ color }} />
        </div>
        <span className="font-medium text-sm truncate flex-1">{name}</span>
      </div>

      <div className="px-3 py-2 text-xs text-muted-foreground">
        <div className="flex justify-between">
          <span>Type</span>
          <Badge variant="secondary" className="text-[10px]">
            {node.type}
          </Badge>
        </div>
        {"health" in node.data && (
          <div className="flex justify-between mt-1">
            <span>Status</span>
            <span
              className={cn(
                node.data.health.status === "healthy" && "text-green-600",
                node.data.health.status === "degraded" && "text-yellow-600",
                node.data.health.status === "down" && "text-red-600",
              )}
            >
              {node.data.health.status}
            </span>
          </div>
        )}
      </div>

      {node.handles.map((handle) => (
        <div
          key={handle.id}
          className={cn(
            "absolute w-3 h-3 rounded-full border-2 bg-background cursor-crosshair",
            "hover:scale-125 transition-transform",
            handle.type === "source" &&
              "right-0 top-1/2 -translate-y-1/2 translate-x-1/2",
            handle.type === "target" &&
              "left-0 top-1/2 -translate-y-1/2 -translate-x-1/2",
          )}
          style={{ borderColor: color }}
          onMouseDown={(e) => {
            e.stopPropagation();
            onHandleMouseDown(node.id, handle.id);
          }}
          onMouseUp={(e) => {
            e.stopPropagation();
            onHandleMouseUp(node.id, handle.id);
          }}
        />
      ))}

      {connectingFrom && connectingFrom.nodeId !== node.id && (
        <div className="absolute inset-0 border-2 border-dashed border-green-500 rounded-lg pointer-events-none animate-pulse" />
      )}
    </motion.div>
  );
}
