import { useState } from "react";
import type { ClaudeApiFormat } from "@/types";

/** 聚合上游草稿（表单内部状态） */
export interface AggUpstreamDraft {
  id: string;
  name: string;
  baseUrl: string;
  apiKey: string;
  apiFormat: ClaudeApiFormat;
  isFullUrl: boolean;
}

/** 聚合模型路由草稿 */
export interface AggRouteDraft {
  model: string;
  upstreamId: string;
  upstreamModel: string;
}

/** 序列化后的聚合配置（写入 settings_config.aggregation） */
export interface AggregationConfigPayload {
  upstreams: Array<{
    id: string;
    name?: string;
    baseUrl: string;
    apiKey: string;
    apiFormat: ClaudeApiFormat;
    isFullUrl?: boolean;
  }>;
  routes: Array<{
    model: string;
    upstreamId: string;
    upstreamModel?: string;
  }>;
}

function genId(): string {
  if (typeof crypto !== "undefined" && crypto.randomUUID) {
    return crypto.randomUUID().slice(0, 8);
  }
  return Math.random().toString(36).slice(2, 10);
}

function newUpstream(): AggUpstreamDraft {
  return {
    id: genId(),
    name: "",
    baseUrl: "",
    apiKey: "",
    apiFormat: "anthropic",
    isFullUrl: false,
  };
}

/** 从已保存的 settings_config.aggregation 还原草稿（编辑态） */
function parseInitial(initial: unknown): {
  upstreams: AggUpstreamDraft[];
  routes: AggRouteDraft[];
} {
  const obj = (initial ?? {}) as Record<string, unknown>;
  const rawUpstreams = Array.isArray(obj.upstreams) ? obj.upstreams : [];
  const rawRoutes = Array.isArray(obj.routes) ? obj.routes : [];

  const upstreams: AggUpstreamDraft[] = rawUpstreams.map((u) => {
    const up = (u ?? {}) as Record<string, unknown>;
    return {
      id: typeof up.id === "string" && up.id ? up.id : genId(),
      name: typeof up.name === "string" ? up.name : "",
      baseUrl: typeof up.baseUrl === "string" ? up.baseUrl : "",
      apiKey: typeof up.apiKey === "string" ? up.apiKey : "",
      apiFormat: (typeof up.apiFormat === "string"
        ? up.apiFormat
        : "anthropic") as ClaudeApiFormat,
      isFullUrl: up.isFullUrl === true,
    };
  });

  const routes: AggRouteDraft[] = rawRoutes.map((r) => {
    const rt = (r ?? {}) as Record<string, unknown>;
    return {
      model: typeof rt.model === "string" ? rt.model : "",
      upstreamId: typeof rt.upstreamId === "string" ? rt.upstreamId : "",
      upstreamModel:
        typeof rt.upstreamModel === "string" ? rt.upstreamModel : "",
    };
  });

  return {
    upstreams: upstreams.length > 0 ? upstreams : [newUpstream()],
    routes,
  };
}

/**
 * 「供应商聚合」表单草稿状态。
 *
 * 维护多条上游 + 模型路由；`toConfig()` 序列化为写入
 * `settings_config.aggregation` 的对象。
 */
export function useAggregationDraftState(initial?: unknown) {
  const [initialState] = useState(() => parseInitial(initial));
  const [upstreams, setUpstreams] = useState<AggUpstreamDraft[]>(
    initialState.upstreams,
  );
  const [routes, setRoutes] = useState<AggRouteDraft[]>(initialState.routes);

  const addUpstream = () => setUpstreams((prev) => [...prev, newUpstream()]);
  const removeUpstream = (id: string) =>
    setUpstreams((prev) => prev.filter((u) => u.id !== id));
  const updateUpstream = (id: string, patch: Partial<AggUpstreamDraft>) =>
    setUpstreams((prev) =>
      prev.map((u) => (u.id === id ? { ...u, ...patch } : u)),
    );

  const addRoute = (route?: Partial<AggRouteDraft>) =>
    setRoutes((prev) => [
      ...prev,
      { model: "", upstreamId: "", upstreamModel: "", ...route },
    ]);
  const removeRoute = (index: number) =>
    setRoutes((prev) => prev.filter((_, i) => i !== index));
  const updateRoute = (index: number, patch: Partial<AggRouteDraft>) =>
    setRoutes((prev) =>
      prev.map((r, i) => (i === index ? { ...r, ...patch } : r)),
    );

  const toConfig = (): AggregationConfigPayload => ({
    upstreams: upstreams
      .filter((u) => u.baseUrl.trim())
      .map((u) => ({
        id: u.id,
        name: u.name.trim() || undefined,
        baseUrl: u.baseUrl.trim(),
        apiKey: u.apiKey.trim(),
        apiFormat: u.apiFormat,
        isFullUrl: u.isFullUrl || undefined,
      })),
    routes: routes
      .filter((r) => r.model.trim() && r.upstreamId)
      .map((r) => ({
        model: r.model.trim(),
        upstreamId: r.upstreamId,
        upstreamModel: r.upstreamModel.trim() || undefined,
      })),
  });

  return {
    upstreams,
    routes,
    addUpstream,
    removeUpstream,
    updateUpstream,
    addRoute,
    removeRoute,
    updateRoute,
    toConfig,
  };
}

export type AggregationDraft = ReturnType<typeof useAggregationDraftState>;
