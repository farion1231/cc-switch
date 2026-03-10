export type DimensionalEntityType =
  | "provider"
  | "model"
  | "agent"
  | "tunnel"
  | "pubkey"
  | "quota"
  | "ranking"
  | "tool";

export type EdgeType = "auth" | "data" | "route" | "quota" | "control";

export interface Cluster {
  region?: "us" | "eu" | "asia" | "global";
  tier?: "premium" | "standard" | "budget";
  reliability?: number;
  family?: string;
  capabilities?: string[];
  trust?: "full" | "limited" | "restricted";
  scope?: "admin" | "developer" | "agent" | "readonly";
  type?: string;
  category?: string;
  risk?: "safe" | "moderate" | "dangerous";
  [key: string]: unknown;
}

export interface ProviderData {
  id: string;
  name: string;
  type: "official" | "aggregator" | "third_party" | "custom";
  cluster: Cluster;
  keys: string[];
  models: string[];
  health: {
    latency: number;
    errorRate: number;
    lastCheck: string;
    status: "healthy" | "degraded" | "down";
  };
}

export interface ModelData {
  id: string;
  name: string;
  provider: string;
  cluster: Cluster;
  pricing: {
    inputPer1M: number;
    outputPer1M: number;
  };
  performance: {
    contextWindow: number;
    maxOutput: number;
    avgLatency: number;
  };
}

export interface AgentData {
  id: string;
  name: string;
  type:
    | "claude"
    | "codex"
    | "gemini"
    | "opencode"
    | "openclaw"
    | "iiagent"
    | "custom";
  cluster: Cluster;
  session?: {
    id: string;
    model: string;
    provider: string;
    tokensUsed: number;
    cost: number;
    startTime: string;
  };
  auth: {
    fingerprint: string;
    providers: string[];
    quotaUsed: number;
    quotaAllocated: number;
  };
}

export interface TunnelData {
  id: string;
  name: string;
  type: "quic" | "tcp" | "ssh" | "websocket" | "http2";
  cluster: Cluster;
  status: {
    active: boolean;
    connections: number;
    bandwidth: number;
    latency: number;
  };
}

export interface PubkeyData {
  id: string;
  fingerprint: string;
  comment?: string;
  cluster: Cluster;
  permissions: {
    providers: string[];
    models: string[];
    quotas: Record<string, number>;
    tools: string[];
  };
  sessions: {
    active: string[];
    total: number;
    lastSeen: string;
  };
}

export interface QuotaData {
  id: string;
  name: string;
  cluster: Cluster;
  limits: {
    total: number;
    used: number;
    remaining: number;
    resetDate: string;
  };
  composition: {
    pubkey: string;
    provider: string;
    priority: number;
  }[];
}

export interface RankingData {
  id: string;
  name: string;
  cluster: Cluster;
  scoring: {
    weights: {
      latency: number;
      cost: number;
      reliability: number;
    };
    algorithm: "weighted" | "round_robin" | "least_connections";
  };
  rankings: {
    entityId: string;
    score: number;
    rank: number;
  }[];
}

export interface ToolData {
  id: string;
  name: string;
  type: "mcp" | "action" | "function" | "webhook";
  cluster: Cluster;
  definition: {
    description: string;
    parameters: Record<string, unknown>;
  };
  access: {
    pubkeys: string[];
    agents: string[];
    requiresApproval: boolean;
  };
}

export type EntityData =
  | ProviderData
  | ModelData
  | AgentData
  | TunnelData
  | PubkeyData
  | QuotaData
  | RankingData
  | ToolData;

export interface GraphHandle {
  id: string;
  type: "source" | "target";
  position: "top" | "right" | "bottom" | "left";
  label?: string;
}

export interface GraphNode {
  id: string;
  type: DimensionalEntityType;
  position: { x: number; y: number };
  data: EntityData;
  style?: {
    background?: string;
    borderColor?: string;
    icon?: string;
  };
  handles: GraphHandle[];
}

export interface GraphEdge {
  id: string;
  source: string;
  target: string;
  sourceHandle: string;
  targetHandle: string;
  type: EdgeType;
  animated: boolean;
  data?: {
    throughput?: number;
    latency?: number;
    status?: "active" | "idle" | "error";
  };
}

export interface GraphCluster {
  id: string;
  label: string;
  nodeIds: string[];
  style?: {
    background?: string;
    borderColor?: string;
  };
}

export interface PatchCordGraph {
  nodes: GraphNode[];
  edges: GraphEdge[];
  clusters: GraphCluster[];
  viewport: { x: number; y: number; zoom: number };
}

export interface Dimension {
  name: string;
  values: string[];
  selected?: string[];
}

export interface FilterState {
  dimensions: Dimension[];
  search: string;
  showOrphans: boolean;
}
