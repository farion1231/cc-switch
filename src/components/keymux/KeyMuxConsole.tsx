import { useState, useCallback, useMemo } from "react";
import { AnimatePresence } from "framer-motion";
import {
  Box,
  Server,
  Cpu,
  Users,
  Key,
  Gauge,
  TrendingUp,
  Wrench,
  Wifi,
  Plus,
  ZoomIn,
  ZoomOut,
  Maximize2,
  Search,
  Layers,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import type {
  PatchCordGraph,
  GraphNode,
  GraphEdge,
  DimensionalEntityType,
  FilterState,
} from "./types";
import { EntityNode } from "./EntityNode";
import { PatchCord } from "./PatchCord";
import { EntityPanel } from "./EntityPanel";
import { DimensionFilter } from "./DimensionFilter";

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

interface KeyMuxConsoleProps {
  graph: PatchCordGraph;
  onNodeSelect?: (node: GraphNode) => void;
  onNodeCreate?: (type: DimensionalEntityType) => void;
  onNodeDelete?: (nodeId: string) => void;
  onEdgeCreate?: (source: string, target: string) => void;
  onEdgeDelete?: (edgeId: string) => void;
}

export function KeyMuxConsole({
  graph,
  onNodeSelect,
  onNodeCreate,
  onNodeDelete,
  onEdgeCreate,
  onEdgeDelete,
}: KeyMuxConsoleProps) {
  const [selectedNode, setSelectedNode] = useState<GraphNode | null>(null);
  const [selectedEdge, setSelectedEdge] = useState<GraphEdge | null>(null);
  const [viewport, setViewport] = useState(graph.viewport);
  const [filterState, setFilterState] = useState<FilterState>({
    dimensions: [
      {
        name: "Type",
        values: [
          "provider",
          "model",
          "agent",
          "tunnel",
          "pubkey",
          "quota",
          "ranking",
          "tool",
        ],
      },
      {
        name: "Status",
        values: ["healthy", "degraded", "down", "active", "idle"],
      },
      { name: "Region", values: ["us", "eu", "asia", "global"] },
    ],
    search: "",
    showOrphans: true,
  });
  const [activeTab, setActiveTab] = useState<string>("graph");
  const [isDragging, setIsDragging] = useState(false);
  const [dragStart, setDragStart] = useState({ x: 0, y: 0 });
  const [connectingFrom, setConnectingFrom] = useState<{
    nodeId: string;
    handleId: string;
  } | null>(null);

  const filteredNodes = useMemo(() => {
    return graph.nodes.filter((node) => {
      if (filterState.search) {
        const search = filterState.search.toLowerCase();
        const name = "name" in node.data ? node.data.name.toLowerCase() : "";
        if (!name.includes(search)) return false;
      }
      return true;
    });
  }, [graph.nodes, filterState]);

  const filteredEdges = useMemo(() => {
    const nodeIds = new Set(filteredNodes.map((n) => n.id));
    return graph.edges.filter(
      (edge) => nodeIds.has(edge.source) && nodeIds.has(edge.target),
    );
  }, [graph.edges, filteredNodes]);

  const handleNodeClick = useCallback(
    (node: GraphNode) => {
      setSelectedNode(node);
      setSelectedEdge(null);
      onNodeSelect?.(node);
    },
    [onNodeSelect],
  );

  const handleEdgeClick = useCallback((edge: GraphEdge) => {
    setSelectedEdge(edge);
    setSelectedNode(null);
  }, []);

  const handleMouseDown = useCallback(
    (e: React.MouseEvent) => {
      if (e.target === e.currentTarget) {
        setIsDragging(true);
        setDragStart({ x: e.clientX - viewport.x, y: e.clientY - viewport.y });
      }
    },
    [viewport],
  );

  const handleMouseMove = useCallback(
    (e: React.MouseEvent) => {
      if (isDragging) {
        setViewport((prev) => ({
          ...prev,
          x: e.clientX - dragStart.x,
          y: e.clientY - dragStart.y,
        }));
      }
    },
    [isDragging, dragStart],
  );

  const handleMouseUp = useCallback(() => {
    setIsDragging(false);
  }, []);

  const handleZoom = useCallback((delta: number) => {
    setViewport((prev) => ({
      ...prev,
      zoom: Math.max(0.25, Math.min(2, prev.zoom + delta)),
    }));
  }, []);

  const handleFitView = useCallback(() => {
    if (filteredNodes.length === 0) return;

    const xs = filteredNodes.map((n) => n.position.x);
    const ys = filteredNodes.map((n) => n.position.y);
    const minX = Math.min(...xs);
    const maxX = Math.max(...xs);
    const minY = Math.min(...ys);
    const maxY = Math.max(...ys);

    const width = maxX - minX + 200;
    const height = maxY - minY + 200;
    const zoom = Math.min(1, Math.min(800 / width, 600 / height));

    setViewport({
      x: -minX * zoom + 100,
      y: -minY * zoom + 100,
      zoom,
    });
  }, [filteredNodes]);

  const handleHandleMouseDown = useCallback(
    (nodeId: string, handleId: string) => {
      setConnectingFrom({ nodeId, handleId });
    },
    [],
  );

  const handleHandleMouseUp = useCallback(
    (nodeId: string, _handleId: string) => {
      if (connectingFrom && connectingFrom.nodeId !== nodeId) {
        onEdgeCreate?.(connectingFrom.nodeId, nodeId);
      }
      setConnectingFrom(null);
    },
    [connectingFrom, onEdgeCreate],
  );

  const entityCounts = useMemo(() => {
    const counts: Record<DimensionalEntityType, number> = {
      provider: 0,
      model: 0,
      agent: 0,
      tunnel: 0,
      pubkey: 0,
      quota: 0,
      ranking: 0,
      tool: 0,
    };
    graph.nodes.forEach((node) => {
      counts[node.type]++;
    });
    return counts;
  }, [graph.nodes]);

  return (
    <div className="flex h-full">
      <div className="flex-1 flex flex-col">
        <div className="border-b bg-background/95 backdrop-blur px-4 py-2 flex items-center justify-between">
          <div className="flex items-center gap-4">
            <div className="flex items-center gap-2">
              <Box className="w-5 h-5 text-primary" />
              <span className="font-semibold">KeyMux Console</span>
            </div>

            <div className="flex items-center gap-1">
              {Object.entries(entityCounts).map(([type, count]) => {
                const Icon = ENTITY_ICONS[type as DimensionalEntityType];
                return (
                  <Badge
                    key={type}
                    variant="outline"
                    className="gap-1"
                    style={{
                      borderColor: ENTITY_COLORS[type as DimensionalEntityType],
                    }}
                  >
                    <Icon className="w-3 h-3" />
                    {count}
                  </Badge>
                );
              })}
            </div>
          </div>

          <div className="flex items-center gap-2">
            <div className="relative">
              <Search className="w-4 h-4 absolute left-2 top-1/2 -translate-y-1/2 text-muted-foreground" />
              <Input
                className="pl-8 w-48"
                placeholder="Search entities..."
                value={filterState.search}
                onChange={(e) =>
                  setFilterState((prev) => ({
                    ...prev,
                    search: e.target.value,
                  }))
                }
              />
            </div>

            <Select value={activeTab} onValueChange={setActiveTab}>
              <SelectTrigger className="w-32">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="graph">Graph</SelectItem>
                <SelectItem value="list">List</SelectItem>
                <SelectItem value="matrix">Matrix</SelectItem>
              </SelectContent>
            </Select>

            <Button
              variant="outline"
              size="icon"
              onClick={() => handleZoom(0.1)}
            >
              <ZoomIn className="w-4 h-4" />
            </Button>
            <Button
              variant="outline"
              size="icon"
              onClick={() => handleZoom(-0.1)}
            >
              <ZoomOut className="w-4 h-4" />
            </Button>
            <Button variant="outline" size="icon" onClick={handleFitView}>
              <Maximize2 className="w-4 h-4" />
            </Button>
            <Button variant="outline" size="icon">
              <Layers className="w-4 h-4" />
            </Button>
            <Button size="sm" onClick={() => onNodeCreate?.("provider")}>
              <Plus className="w-4 h-4 mr-1" />
              Add
            </Button>
          </div>
        </div>

        <div
          className="flex-1 relative overflow-hidden bg-muted/30"
          onMouseDown={handleMouseDown}
          onMouseMove={handleMouseMove}
          onMouseUp={handleMouseUp}
          onMouseLeave={handleMouseUp}
        >
          <svg className="absolute inset-0 w-full h-full pointer-events-none">
            <defs>
              <marker
                id="arrowhead"
                markerWidth="10"
                markerHeight="7"
                refX="9"
                refY="3.5"
                orient="auto"
              >
                <polygon points="0 0, 10 3.5, 0 7" fill="#888" />
              </marker>
            </defs>
            <g
              style={{
                transform: `translate(${viewport.x}px, ${viewport.y}px) scale(${viewport.zoom})`,
                transformOrigin: "0 0",
              }}
            >
              {filteredEdges.map((edge) => (
                <PatchCord
                  key={edge.id}
                  edge={edge}
                  nodes={filteredNodes}
                  isSelected={selectedEdge?.id === edge.id}
                  onClick={() => handleEdgeClick(edge)}
                />
              ))}
            </g>
          </svg>

          <div
            className="absolute inset-0"
            style={{
              transform: `translate(${viewport.x}px, ${viewport.y}px) scale(${viewport.zoom})`,
              transformOrigin: "0 0",
            }}
          >
            <AnimatePresence>
              {filteredNodes.map((node) => (
                <EntityNode
                  key={node.id}
                  node={node}
                  isSelected={selectedNode?.id === node.id}
                  onClick={() => handleNodeClick(node)}
                  onHandleMouseDown={handleHandleMouseDown}
                  onHandleMouseUp={handleHandleMouseUp}
                  connectingFrom={connectingFrom}
                />
              ))}
            </AnimatePresence>
          </div>

          {connectingFrom && (
            <div className="absolute bottom-4 left-4 bg-background/90 backdrop-blur px-3 py-2 rounded-lg border text-sm">
              Connecting from {connectingFrom.nodeId}...
            </div>
          )}
        </div>
      </div>

      <div className="w-80 border-l bg-background flex flex-col">
        <Tabs
          value={activeTab}
          onValueChange={setActiveTab}
          className="flex-1 flex flex-col"
        >
          <TabsList className="grid w-full grid-cols-2">
            <TabsTrigger value="details">Details</TabsTrigger>
            <TabsTrigger value="filter">Filter</TabsTrigger>
          </TabsList>

          <TabsContent value="details" className="flex-1 overflow-auto p-4">
            {selectedNode ? (
              <EntityPanel
                node={selectedNode}
                onDelete={() => onNodeDelete?.(selectedNode.id)}
              />
            ) : selectedEdge ? (
              <Card>
                <CardHeader>
                  <CardTitle className="text-sm">Connection</CardTitle>
                </CardHeader>
                <CardContent className="space-y-2 text-sm">
                  <div className="flex justify-between">
                    <span className="text-muted-foreground">From</span>
                    <span>{selectedEdge.source}</span>
                  </div>
                  <div className="flex justify-between">
                    <span className="text-muted-foreground">To</span>
                    <span>{selectedEdge.target}</span>
                  </div>
                  <div className="flex justify-between">
                    <span className="text-muted-foreground">Type</span>
                    <Badge variant="outline">{selectedEdge.type}</Badge>
                  </div>
                  {selectedEdge.data?.latency && (
                    <div className="flex justify-between">
                      <span className="text-muted-foreground">Latency</span>
                      <span>{selectedEdge.data.latency}ms</span>
                    </div>
                  )}
                  <Button
                    variant="destructive"
                    size="sm"
                    className="w-full mt-4"
                    onClick={() => onEdgeDelete?.(selectedEdge.id)}
                  >
                    Delete Connection
                  </Button>
                </CardContent>
              </Card>
            ) : (
              <div className="text-center text-muted-foreground py-8">
                Select a node or connection to view details
              </div>
            )}
          </TabsContent>

          <TabsContent value="filter" className="flex-1 overflow-auto p-4">
            <DimensionFilter
              dimensions={filterState.dimensions}
              onChange={(dimensions) =>
                setFilterState((prev) => ({ ...prev, dimensions }))
              }
            />
          </TabsContent>
        </Tabs>
      </div>
    </div>
  );
}
