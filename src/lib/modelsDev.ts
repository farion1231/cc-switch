// models.dev 公共数据（https://models.dev/api.json）的拉取与原始结构。
// 用量统计的价格导入和供应商表单的模型参数补全共用这一份数据和查询缓存。

import { invoke } from "@tauri-apps/api/core";
import { queryOptions } from "@tanstack/react-query";

const MODELS_DEV_STALE_TIME_MS = 60 * 60 * 1000;

export interface ModelsDevCost {
  input?: number;
  output?: number;
  cache_read?: number;
  cache_write?: number;
}

export interface ModelsDevModalities {
  input?: string[];
  output?: string[];
}

export interface ModelsDevLimit {
  context?: number;
  input?: number;
  output?: number;
}

export interface ModelsDevReasoningOption {
  type?: string;
  values?: string[];
}

export interface ModelsDevModel {
  id?: string;
  name?: string;
  release_date?: string;
  cost?: ModelsDevCost;
  modalities?: ModelsDevModalities;
  limit?: ModelsDevLimit;
  reasoning?: boolean;
  reasoning_options?: ModelsDevReasoningOption[];
  // 转售 / 聚合商的条目指回原厂条目，形如 "zhipuai/glm-5"。
  canonical_model_id?: string;
  status?: string;
}

export interface ModelsDevProvider {
  id?: string;
  name?: string;
  // 供应商的 API 基址，按它把用户填的地址认成某一家。
  api?: string;
  models?: Record<string, ModelsDevModel>;
}

export type ModelsDevResponse = Record<string, ModelsDevProvider>;

export async function fetchModelsDev(): Promise<ModelsDevResponse> {
  // 走后端的全局代理客户端拉取：WebView 的 fetch 只跟随系统代理，
  // 代理环境下直连 models.dev 会超时。超时与响应大小上限由后端控制。
  const raw = await invoke<string>("fetch_models_dev");
  return JSON.parse(raw) as ModelsDevResponse;
}

/**
 * models.dev 的查询参数，所有用到它的地方共用一份缓存，1 小时内不重新请求。
 * 表单只预取不订阅，没有订阅者时缓存按 gcTime 回收（默认 5 分钟），这里与
 * staleTime 对齐。
 */
export const modelsDevQueryOptions = queryOptions({
  queryKey: ["models-dev"],
  queryFn: fetchModelsDev,
  staleTime: MODELS_DEV_STALE_TIME_MS,
  gcTime: MODELS_DEV_STALE_TIME_MS,
});

/**
 * 去掉命名空间前缀（`vendor/`）、`:变体` 后缀和 `[1m]` 标记后小写，
 * 让同一个模型在不同供应商那里的写法落到同一个 ID 上。
 */
export function normalizeModelsDevModelId(modelId: string): string {
  const afterSlash = modelId.slice(modelId.lastIndexOf("/") + 1);
  const beforeColon = afterSlash.split(":")[0] ?? "";
  let normalized = beforeColon.trim().replace(/@/g, "-").toLowerCase();
  if (normalized.endsWith("[1m]")) {
    normalized = normalized.slice(0, -"[1m]".length).trim();
  }
  return normalized;
}
