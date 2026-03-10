import {
  Server,
  Cpu,
  Users,
  Wifi,
  Key,
  Gauge,
  TrendingUp,
  Wrench,
  Trash2,
  Edit,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
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

interface EntityPanelProps {
  node: GraphNode;
  onDelete: () => void;
}

export function EntityPanel({ node, onDelete }: EntityPanelProps) {
  const Icon = ENTITY_ICONS[node.type];
  const color = ENTITY_COLORS[node.type];
  const name = "name" in node.data ? node.data.name : node.id;

  return (
    <div className="space-y-4">
      <Card>
        <CardHeader className="pb-2">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-2">
              <div
                className="w-8 h-8 rounded-lg flex items-center justify-center"
                style={{ backgroundColor: color + "20" }}
              >
                <Icon className="w-5 h-5" style={{ color }} />
              </div>
              <CardTitle className="text-lg">{name}</CardTitle>
            </div>
            <Badge variant="outline" style={{ borderColor: color, color }}>
              {node.type}
            </Badge>
          </div>
        </CardHeader>
        <CardContent className="space-y-3 text-sm">
          <div className="flex justify-between">
            <span className="text-muted-foreground">ID</span>
            <code className="text-xs bg-muted px-1 rounded">{node.id}</code>
          </div>

          {"type" in node.data && node.data.type && (
            <div className="flex justify-between">
              <span className="text-muted-foreground">Type</span>
              <span>{node.data.type}</span>
            </div>
          )}

          {"health" in node.data && node.data.health && (
            <>
              <div className="flex justify-between">
                <span className="text-muted-foreground">Status</span>
                <Badge
                  variant={
                    node.data.health.status === "healthy"
                      ? "default"
                      : node.data.health.status === "degraded"
                        ? "secondary"
                        : "destructive"
                  }
                >
                  {node.data.health.status}
                </Badge>
              </div>
              <div className="flex justify-between">
                <span className="text-muted-foreground">Latency</span>
                <span>{node.data.health.latency}ms</span>
              </div>
              <div className="flex justify-between">
                <span className="text-muted-foreground">Error Rate</span>
                <span>{(node.data.health.errorRate * 100).toFixed(1)}%</span>
              </div>
            </>
          )}

          {"cluster" in node.data && node.data.cluster && (
            <div className="border-t pt-3 mt-3">
              <div className="text-xs font-medium text-muted-foreground mb-2">
                Cluster
              </div>
              {Object.entries(node.data.cluster).map(
                ([key, value]) =>
                  value !== undefined && (
                    <div key={key} className="flex justify-between">
                      <span className="text-muted-foreground capitalize">
                        {key}
                      </span>
                      <span>{String(value)}</span>
                    </div>
                  ),
              )}
            </div>
          )}

          {"pricing" in node.data && node.data.pricing && (
            <div className="border-t pt-3 mt-3">
              <div className="text-xs font-medium text-muted-foreground mb-2">
                Pricing
              </div>
              <div className="flex justify-between">
                <span className="text-muted-foreground">Input</span>
                <span>${node.data.pricing.inputPer1M}/1M</span>
              </div>
              <div className="flex justify-between">
                <span className="text-muted-foreground">Output</span>
                <span>${node.data.pricing.outputPer1M}/1M</span>
              </div>
            </div>
          )}

          {"performance" in node.data && node.data.performance && (
            <div className="border-t pt-3 mt-3">
              <div className="text-xs font-medium text-muted-foreground mb-2">
                Performance
              </div>
              <div className="flex justify-between">
                <span className="text-muted-foreground">Context</span>
                <span>
                  {node.data.performance.contextWindow?.toLocaleString()}
                </span>
              </div>
              <div className="flex justify-between">
                <span className="text-muted-foreground">Max Output</span>
                <span>{node.data.performance.maxOutput?.toLocaleString()}</span>
              </div>
            </div>
          )}

          {"permissions" in node.data && node.data.permissions && (
            <div className="border-t pt-3 mt-3">
              <div className="text-xs font-medium text-muted-foreground mb-2">
                Permissions
              </div>
              <div className="flex flex-wrap gap-1">
                {node.data.permissions.providers?.map((p) => (
                  <Badge key={p} variant="secondary" className="text-xs">
                    {p}
                  </Badge>
                ))}
              </div>
            </div>
          )}

          {"auth" in node.data && node.data.auth && (
            <div className="border-t pt-3 mt-3">
              <div className="text-xs font-medium text-muted-foreground mb-2">
                Authentication
              </div>
              <div className="flex justify-between">
                <span className="text-muted-foreground">Fingerprint</span>
                <code className="text-xs">
                  {node.data.auth.fingerprint?.slice(0, 16)}...
                </code>
              </div>
              <div className="flex justify-between">
                <span className="text-muted-foreground">Quota</span>
                <span>
                  {node.data.auth.quotaUsed}/{node.data.auth.quotaAllocated}
                </span>
              </div>
            </div>
          )}

          {"limits" in node.data && node.data.limits && (
            <div className="border-t pt-3 mt-3">
              <div className="text-xs font-medium text-muted-foreground mb-2">
                Quota
              </div>
              <div className="flex justify-between">
                <span className="text-muted-foreground">Used</span>
                <span>{node.data.limits.used?.toLocaleString()}</span>
              </div>
              <div className="flex justify-between">
                <span className="text-muted-foreground">Total</span>
                <span>{node.data.limits.total?.toLocaleString()}</span>
              </div>
              <div className="w-full bg-muted rounded-full h-2 mt-2">
                <div
                  className="h-full rounded-full transition-all"
                  style={{
                    width: `${(node.data.limits.used / node.data.limits.total) * 100}%`,
                    backgroundColor: color,
                  }}
                />
              </div>
            </div>
          )}

          {"sessions" in node.data && node.data.sessions && (
            <div className="border-t pt-3 mt-3">
              <div className="text-xs font-medium text-muted-foreground mb-2">
                Sessions
              </div>
              <div className="flex justify-between">
                <span className="text-muted-foreground">Active</span>
                <Badge>{node.data.sessions.active?.length || 0}</Badge>
              </div>
              <div className="flex justify-between">
                <span className="text-muted-foreground">Total</span>
                <span>{node.data.sessions.total}</span>
              </div>
            </div>
          )}

          <div className="flex gap-2 pt-3 border-t">
            <Button variant="outline" size="sm" className="flex-1">
              <Edit className="w-4 h-4 mr-1" />
              Edit
            </Button>
            <Button variant="destructive" size="sm" onClick={onDelete}>
              <Trash2 className="w-4 h-4 mr-1" />
              Delete
            </Button>
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
