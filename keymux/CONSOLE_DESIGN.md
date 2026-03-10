# KeyMux Console - Dimensionalized Architecture

## Overview

A visual console for KeyMux with n8n-style patch cord views, dimensionalized entity management, and real-time session monitoring.

## Dimensionalized Entities

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           DIMENSIONAL MODEL                                  │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│  PROVIDERS          MODELS           AGENTS           TUNNELS              │
│  ┌─────────┐       ┌─────────┐      ┌─────────┐      ┌─────────┐           │
│  │ OpenAI  │──────▶│ gpt-4   │      │ Claude  │      │ QUIC    │           │
│  │ Anthropic│──────▶│ claude-3│─────▶│ Codex   │──────│ TCP     │           │
│  │ Google  │       │ gemini  │      │ Custom  │      │ SSH     │           │
│  │ DeepSeek│       │ deepseek│      │ Remote  │      │ WebSocket│          │
│  └─────────┘       └─────────┘      └─────────┘      └─────────┘           │
│       │                │                │                │                  │
│       ▼                ▼                ▼                ▼                  │
│  ┌─────────┐       ┌─────────┐      ┌─────────┐      ┌─────────┐           │
│  │ PUBKEYS │◀──────│ QUOTAS  │──────│ RANKINGS│──────│ TOOLS   │           │
│  │ SHA256: │       │ $/month │      │ Priority│      │ Embodiment│          │
│  │ allowed │       │ tokens  │      │ Health  │      │ MCP      │          │
│  │ providers│      │ rate    │      │ Latency │      │ Actions  │          │
│  └─────────┘       └─────────┘      └─────────┘      └─────────┘           │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘
```

## Entity Definitions

### 1. Providers (API Endpoints)

```typescript
interface DimensionalProvider {
  id: string;
  name: string;
  type: "official" | "aggregator" | "third_party" | "custom";

  // Cluster: geographic/infrastructure grouping
  cluster: {
    region: "us" | "eu" | "asia" | "global";
    tier: "premium" | "standard" | "budget";
    reliability: number; // 0-1
  };

  // Keys attached to this provider
  keys: string[]; // Key IDs

  // Models this provider offers
  models: string[]; // Model IDs

  // Health metrics
  health: {
    latency: number;
    errorRate: number;
    lastCheck: Date;
    status: "healthy" | "degraded" | "down";
  };

  // Patch cord connections
  connections: {
    models: string[];
    tunnels: string[];
    agents: string[];
  };
}
```

### 2. Models (AI Models)

```typescript
interface DimensionalModel {
  id: string;
  name: string;
  provider: string; // Provider ID

  // Cluster: capability grouping
  cluster: {
    family: "gpt" | "claude" | "gemini" | "deepseek" | "other";
    tier: "flagship" | "standard" | "lightweight";
    capabilities: ("chat" | "vision" | "tools" | "streaming" | "embedding")[];
  };

  // Cost dimensions
  pricing: {
    inputPer1M: number;
    outputPer1M: number;
    cacheRead?: number;
    cacheWrite?: number;
  };

  // Performance dimensions
  performance: {
    contextWindow: number;
    maxOutput: number;
    avgLatency: number;
    throughput: number; // tokens/sec
  };

  // Patch cord connections
  connections: {
    providers: string[];
    agents: string[];
    routes: string[];
  };
}
```

### 3. Agents (AI Agent Sessions)

```typescript
interface DimensionalAgent {
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

  // Cluster: usage pattern grouping
  cluster: {
    purpose: "coding" | "analysis" | "creative" | "general";
    intensity: "heavy" | "moderate" | "light";
    autonomy: "autonomous" | "assisted" | "interactive";
  };

  // Current session
  session?: {
    id: string;
    model: string;
    provider: string;
    tokensUsed: number;
    cost: number;
    startTime: Date;
  };

  // Pubkey authentication
  auth: {
    fingerprint: string;
    providers: string[];
    quotaAllocated: number;
    quotaUsed: number;
  };

  // Patch cord connections
  connections: {
    models: string[];
    providers: string[];
    pubkeys: string[];
    tools: string[];
  };
}
```

### 4. Tunnels (Transport Layer)

```typescript
interface DimensionalTunnel {
  id: string;
  name: string;
  type: "quic" | "tcp" | "ssh" | "websocket" | "http2";

  // Cluster: network grouping
  cluster: {
    layer: "transport" | "application";
    encryption: "tls" | "quic" | "ssh" | "none";
    multiplexed: boolean;
  };

  // Status
  status: {
    active: boolean;
    connections: number;
    bandwidth: number;
    latency: number;
  };

  // Patch cord connections
  connections: {
    providers: string[];
    agents: string[];
    endpoints: string[];
  };
}
```

### 5. Pubkeys (SSH Authentication)

```typescript
interface DimensionalPubkey {
  id: string;
  fingerprint: string; // SHA256:xxx
  comment?: string;

  // Cluster: trust grouping
  cluster: {
    trust: "full" | "limited" | "restricted";
    scope: "admin" | "developer" | "agent" | "readonly";
  };

  // Permissions
  permissions: {
    providers: string[]; // Which providers can access
    models: string[]; // Which models can use
    quotas: Record<string, number>; // provider -> quota limit
    tools: string[]; // Which tools can use
  };

  // Sessions
  sessions: {
    active: string[];
    total: number;
    lastSeen: Date;
  };

  // Patch cord connections
  connections: {
    providers: string[];
    agents: string[];
    quotas: string[];
  };
}
```

### 6. Quotas (Resource Limits)

```typescript
interface DimensionalQuota {
  id: string;
  name: string;

  // Cluster: resource grouping
  cluster: {
    type: "monthly" | "daily" | "per_session";
    unit: "tokens" | "dollars" | "requests";
  };

  // Limits
  limits: {
    total: number;
    used: number;
    remaining: number;
    resetDate: Date;
  };

  // Composition (how quota is distributed)
  composition: {
    pubkey: string;
    provider: string;
    priority: number;
  }[];

  // Patch cord connections
  connections: {
    pubkeys: string[];
    providers: string[];
    agents: string[];
  };
}
```

### 7. Rankings (Priority/Health)

```typescript
interface DimensionalRanking {
  id: string;
  name: string;

  // Cluster: ranking criteria
  cluster: {
    criteria: "latency" | "cost" | "reliability" | "custom";
    scope: "provider" | "model" | "route";
  };

  // Scoring
  scoring: {
    weights: {
      latency: number;
      cost: number;
      reliability: number;
      custom: number;
    };
    algorithm: "weighted" | "round_robin" | "least_connections" | "adaptive";
  };

  // Current rankings
  rankings: {
    entityId: string;
    score: number;
    rank: number;
  }[];

  // Patch cord connections
  connections: {
    providers: string[];
    models: string[];
    routes: string[];
  };
}
```

### 8. Embodiment Tools (MCP/Actions)

```typescript
interface DimensionalTool {
  id: string;
  name: string;
  type: "mcp" | "action" | "function" | "webhook";

  // Cluster: capability grouping
  cluster: {
    category: "filesystem" | "network" | "database" | "ai" | "automation";
    risk: "safe" | "moderate" | "dangerous";
  };

  // Tool definition
  definition: {
    description: string;
    parameters: Record<string, any>;
    returns: string;
  };

  // Access control
  access: {
    pubkeys: string[];
    agents: string[];
    requiresApproval: boolean;
  };

  // Patch cord connections
  connections: {
    agents: string[];
    pubkeys: string[];
    models: string[];
  };
}
```

## Patch Cord Graph Structure

```typescript
interface PatchCordGraph {
  nodes: GraphNode[];
  edges: GraphEdge[];
  clusters: GraphCluster[];
  viewport: { x: number; y: number; zoom: number };
}

interface GraphNode {
  id: string;
  type:
    | "provider"
    | "model"
    | "agent"
    | "tunnel"
    | "pubkey"
    | "quota"
    | "ranking"
    | "tool";
  position: { x: number; y: number };
  data: DimensionalProvider | DimensionalModel | /* ... */ DimensionalTool;

  // Visual properties
  style?: {
    background?: string;
    borderColor?: string;
    icon?: string;
  };

  // Handles for patch cords
  handles: {
    id: string;
    type: "source" | "target";
    position: "top" | "right" | "bottom" | "left";
    label?: string;
  }[];
}

interface GraphEdge {
  id: string;
  source: string;
  target: string;
  sourceHandle: string;
  targetHandle: string;

  // Edge type determines visual style
  type: "auth" | "data" | "route" | "quota" | "control";

  // Animated edges show active connections
  animated: boolean;

  // Edge metadata
  data?: {
    throughput?: number;
    latency?: number;
    status?: "active" | "idle" | "error";
  };
}

interface GraphCluster {
  id: string;
  label: string;
  nodeIds: string[];
  style?: {
    background?: string;
    borderColor?: string;
  };
}
```

## What Can Go Wrong?

### 1. Security Risks

| Risk                  | Impact                                            | Mitigation                                                          |
| --------------------- | ------------------------------------------------- | ------------------------------------------------------------------- |
| **Pubkey compromise** | Attacker gains access to all authorized providers | Per-pubkey quotas, IP restrictions, session timeouts, audit logging |
| **Key leakage**       | API keys exposed in logs/UI                       | Keys stored encrypted, masked in UI, never logged                   |
| **Session hijacking** | Attacker takes over authenticated session         | Short-lived session tokens, re-auth on sensitive ops, IP binding    |
| **Quota bypass**      | Agent exceeds allocated resources                 | Hard limits at proxy level, real-time monitoring, automatic cutoff  |

### 2. Availability Risks

| Risk                 | Impact                              | Mitigation                                                       |
| -------------------- | ----------------------------------- | ---------------------------------------------------------------- |
| **Provider outage**  | All agents using that provider fail | Failover rankings, circuit breakers, health checks               |
| **Tunnel failure**   | QUIC/TCP connection drops           | Automatic reconnection, connection migration, fallback protocols |
| **Quota exhaustion** | Agents blocked mid-task             | Quota warnings, graceful degradation, priority preemption        |
| **Graph rendering**  | UI freezes with large graphs        | Virtualization, clustering, lazy loading, web workers            |

### 3. Data Integrity Risks

| Risk                  | Impact                          | Mitigation                                                         |
| --------------------- | ------------------------------- | ------------------------------------------------------------------ |
| **Race conditions**   | Concurrent edits corrupt state  | Optimistic locking, conflict resolution, atomic operations         |
| **Graph desync**      | UI shows stale connections      | WebSocket updates, polling fallback, version stamps                |
| **Orphaned entities** | Nodes without valid connections | Cascade deletion, referential integrity checks, garbage collection |

### 4. Performance Risks

| Risk                  | Impact                           | Mitigation                                         |
| --------------------- | -------------------------------- | -------------------------------------------------- |
| **Graph explosion**   | Too many nodes/edges to render   | Clustered view, filtering, pagination              |
| **Real-time updates** | WebSocket spam overwhelms client | Debouncing, throttling, delta updates              |
| **Metric collection** | Monitoring slows proxy           | Sampling, async collection, background aggregation |

### 5. UX Risks

| Risk                     | Impact                                 | Mitigation                                      |
| ------------------------ | -------------------------------------- | ----------------------------------------------- |
| **Cable salad**          | Too many patch cords become unreadable | Auto-layout, bundling, hide/show by type        |
| **Modal fatigue**        | Too many dialogs for CRUD              | Inline editing, bulk operations, smart defaults |
| **Information overload** | Too many dimensions at once            | Progressive disclosure, drill-down, saved views |

### 6. Architecture Risks

| Risk               | Impact                            | Mitigation                                      |
| ------------------ | --------------------------------- | ----------------------------------------------- |
| **Tight coupling** | Changes break multiple components | Event-driven architecture, interface boundaries |
| **Feature creep**  | Complexity grows unbounded        | Feature flags, staged rollout, user feedback    |
| **Technical debt** | Code quality degrades             | Refactoring sprints, code review, documentation |

## Implementation Phases

### Phase 1: Core Entities (Week 1-2)

- Providers, Models, Pubkeys CRUD
- Basic patch cord visualization
- Known peers management

### Phase 2: Sessions & Auth (Week 3-4)

- SSH pubkey authentication
- Session management
- Quota tracking

### Phase 3: Advanced Graph (Week 5-6)

- Clustering/grouping
- Auto-layout
- Real-time updates

### Phase 4: Embodiment Tools (Week 7-8)

- MCP integration
- Tool access control
- Agent tool bindings

### Phase 5: Rankings & Optimization (Week 9-10)

- Provider ranking algorithms
- Failover configuration
- Performance tuning
