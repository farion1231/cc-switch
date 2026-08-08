import type { AggregateRoute, AggregateRoutes, Provider } from "@/types";
import { providerPresets } from "@/config/claudeProviderPresets";
import { extractCodexBaseUrl } from "@/utils/providerConfigUtils";
import { supportsOfficialProxyTakeover } from "@/utils/providerCapabilities";
import type { FetchedModel } from "@/lib/api/model-fetch";

// 聚合供应商自身没有端点或凭据；接管时由后端注入本地路由地址和占位认证。
export const AGGREGATE_SETTINGS_CONFIG = {} as const;

export const AGGREGATE_ROUTE_TIERS = [
  "haiku",
  "sonnet",
  "opus",
  "fable",
] as const;

export type AggregateRouteTier = (typeof AGGREGATE_ROUTE_TIERS)[number];

/** 档位是否已有任何输入（providerId 或 model 非空即视为"动了这一档"） */
function tierHasAnyInput(route?: AggregateRoute): boolean {
  if (!route) return false;
  return Boolean(route.providerId?.trim() || route.model?.trim());
}

/** custom 条目是否已有任何输入（key/providerId/model 任一非空） */
function customEntryHasAnyInput(key: string, route?: AggregateRoute): boolean {
  return Boolean(
    key.trim() || route?.providerId?.trim() || route?.model?.trim(),
  );
}

/** 路由表是否至少配置（或填写）了一档 / 一条 custom 路由 */
export function hasAggregateRoutes(routes?: AggregateRoutes | null): boolean {
  if (!routes) return false;
  return (
    AGGREGATE_ROUTE_TIERS.some((tier) => tierHasAnyInput(routes[tier])) ||
    Object.entries(routes.custom ?? {}).some(([key, route]) =>
      customEntryHasAnyInput(key, route),
    )
  );
}

/** 是否为聚合供应商（meta.aggregateRoutes 非空） */
export function isAggregateProvider(provider: Pick<Provider, "meta">): boolean {
  return hasAggregateRoutes(provider.meta?.aggregateRoutes);
}

/** 收集路由表中所有目标 provider ID（四档 + custom，去重） */
export function getAggregateRouteTargetIds(
  routes?: AggregateRoutes | null,
): string[] {
  if (!routes) return [];
  const ids = new Set<string>();
  for (const tier of AGGREGATE_ROUTE_TIERS) {
    const providerId = routes[tier]?.providerId?.trim();
    if (providerId) ids.add(providerId);
  }
  for (const route of Object.values(routes.custom ?? {})) {
    const providerId = route?.providerId?.trim();
    if (providerId) ids.add(providerId);
  }
  return [...ids];
}

/**
 * 可作为聚合路由目标的供应商列表：
 * 排除聚合供应商自身（不允许嵌套）与当前正在编辑的供应商（不允许自指）。
 * 官方供应商默认排除，但保留后端允许接管的目标（Codex 内置官方供应商，
 * 与 Rust 端 official_provider_supports_proxy_takeover 保持一致）。
 */
export function getAggregateRouteTargets(
  providers: Provider[],
  appId: "claude" | "claude-desktop" | "codex",
  excludeProviderId?: string,
): Provider[] {
  return providers.filter(
    (provider) =>
      provider.id !== excludeProviderId &&
      (provider.category !== "official" ||
        supportsOfficialProxyTakeover(appId, provider)) &&
      !isAggregateProvider(provider),
  );
}

/** Codex 聚合路由的表单行（有序列表，key 即请求模型名） */
export interface AggregateCustomRouteRow {
  key: string;
  providerId: string;
  model: string;
}

/** custom Record -> 有序行列表（编辑时回填表单） */
export function customRoutesToRows(
  custom?: Record<string, AggregateRoute> | null,
): AggregateCustomRouteRow[] {
  return Object.entries(custom ?? {}).map(([key, route]) => ({
    key,
    providerId: route?.providerId ?? "",
    model: route?.model ?? "",
  }));
}

/** 有序行列表 -> custom Record（原始 key 直接作为键，重复/空 key 由提交校验处理） */
export function rowsToCustomRoutes(
  rows: AggregateCustomRouteRow[],
): Record<string, AggregateRoute> {
  const custom: Record<string, AggregateRoute> = {};
  for (const row of rows) {
    custom[row.key] = { providerId: row.providerId, model: row.model };
  }
  return custom;
}

/**
 * 归一化路由表（按 app 剔除另一侧的配置）：
 * - claude：trim，仅保留 providerId 与 model 均非空的档位，丢弃 custom；
 * - codex：仅保留 key/providerId/model trim 后均非空的 custom 条目（key 也 trim），丢弃四档。
 */
export function normalizeAggregateRoutes(
  routes: AggregateRoutes,
  appId: "claude" | "codex",
): AggregateRoutes {
  if (appId === "codex") {
    const custom: Record<string, AggregateRoute> = {};
    for (const [rawKey, route] of Object.entries(routes.custom ?? {})) {
      const key = rawKey.trim();
      const providerId = route?.providerId?.trim() ?? "";
      const model = route?.model?.trim() ?? "";
      if (key && providerId && model) {
        custom[key] = { providerId, model };
      }
    }
    return Object.keys(custom).length > 0 ? { custom } : {};
  }

  const normalized: AggregateRoutes = {};
  for (const tier of AGGREGATE_ROUTE_TIERS) {
    const route = routes[tier];
    if (!route) continue;
    const providerId = route.providerId?.trim() ?? "";
    const model = route.model?.trim() ?? "";
    if (providerId && model) {
      normalized[tier] = { providerId, model };
    }
  }
  return normalized;
}

export type AggregateRoutesValidation =
  | { ok: true; routes: AggregateRoutes }
  | { ok: false; reason: "empty" }
  | { ok: false; reason: "incomplete"; tier: string }
  | { ok: false; reason: "duplicate"; key: string };

/**
 * 提交前校验：
 * - claude：至少配置一档；动了某档（provider/model 填了任意一个）就要求两者都非空。
 * - codex：归一化后至少一条 custom；任一条目只填了部分报 incomplete
 *   （tier 字段复用携带 key）；key（trim 后）重复报 duplicate。
 *   customRows 为表单的有序行态（Record 无法表达重复 key，故由表单传入）。
 */
export function validateAggregateRoutes(
  routes: AggregateRoutes,
  appId: "claude" | "claude-desktop" | "codex",
  customRows?: AggregateCustomRouteRow[],
): AggregateRoutesValidation {
  if (appId === "codex") {
    const rows =
      customRows ??
      customRoutesToRows(routes.custom).filter(
        (row) => row.key.trim() || row.providerId.trim() || row.model.trim(),
      );

    const seenKeys = new Set<string>();
    for (const row of rows) {
      const key = row.key.trim();
      if (!key) continue;
      if (seenKeys.has(key)) {
        return { ok: false, reason: "duplicate", key };
      }
      seenKeys.add(key);
    }

    for (const row of rows) {
      const filled = [
        row.key.trim(),
        row.providerId.trim(),
        row.model.trim(),
      ].filter(Boolean).length;
      if (filled > 0 && filled < 3) {
        return { ok: false, reason: "incomplete", tier: row.key.trim() };
      }
    }

    const normalized = normalizeAggregateRoutes(routes, "codex");
    if (!hasAggregateRoutes(normalized)) {
      return { ok: false, reason: "empty" };
    }
    return { ok: true, routes: normalized };
  }

  for (const tier of AGGREGATE_ROUTE_TIERS) {
    const route = routes[tier];
    if (!route) continue;
    const hasProvider = Boolean(route.providerId?.trim());
    const hasModel = Boolean(route.model?.trim());
    if (hasProvider !== hasModel) {
      return { ok: false, reason: "incomplete", tier };
    }
  }

  const normalized = normalizeAggregateRoutes(routes, "claude");
  if (!hasAggregateRoutes(normalized)) {
    return { ok: false, reason: "empty" };
  }
  return { ok: true, routes: normalized };
}

export interface AggregateRouteConnection {
  baseUrl: string;
  apiKey: string;
  isFullUrl?: boolean;
  modelsUrl?: string;
  customUserAgent?: string;
}

/**
 * 从目标 provider 的 settings_config 提取「获取模型列表」所需的连接信息。
 * modelsUrl 的取法与 ClaudeFormFields 一致：baseURL 命中某预设的默认值时，
 * 优先使用该预设上的 modelsUrl 覆写（如 DeepSeek 把 /models 挂在根路径）。
 */
export function getAggregateRouteConnection(
  provider: Provider,
): AggregateRouteConnection {
  const env =
    ((provider.settingsConfig as Record<string, unknown>)?.env as
      | Record<string, unknown>
      | undefined) ?? {};
  const baseUrl =
    typeof env.ANTHROPIC_BASE_URL === "string" ? env.ANTHROPIC_BASE_URL : "";
  const token = env.ANTHROPIC_AUTH_TOKEN;
  const key = env.ANTHROPIC_API_KEY;
  const apiKey =
    typeof token === "string" && token
      ? token
      : typeof key === "string"
        ? key
        : "";

  const matchedPreset = providerPresets.find((preset) => {
    const presetEnv = (
      preset.settingsConfig as { env?: Record<string, string> }
    )?.env;
    return baseUrl !== "" && presetEnv?.ANTHROPIC_BASE_URL === baseUrl;
  });

  return {
    baseUrl,
    apiKey,
    isFullUrl: provider.meta?.isFullUrl,
    modelsUrl: matchedPreset?.modelsUrl,
    customUserAgent: provider.meta?.customUserAgent,
  };
}

// Codex 聚合路由「请求模型名」的官方候选（下拉建议，仍允许自由输入）
export const CODEX_OFFICIAL_MODEL_SUGGESTIONS: FetchedModel[] = [
  "gpt-5.6",
  "gpt-5.5",
  "gpt-5.4",
  "gpt-5.4-mini",
  "gpt-5.4-mini-fast",
  "codex-mini-latest",
].map((id) => ({ id, ownedBy: "OpenAI" }));

/**
 * Codex 目标 provider 的「获取模型列表」连接信息：
 * baseUrl 从 settings_config.config（TOML）提取，apiKey 取 auth.OPENAI_API_KEY；
 * codex 预设没有 modelsUrl 覆写，固定为 undefined。
 */
export function getCodexAggregateRouteConnection(
  provider: Provider,
): AggregateRouteConnection {
  const settings = provider.settingsConfig as
    | Record<string, unknown>
    | undefined;
  const config = typeof settings?.config === "string" ? settings.config : "";
  const auth = settings?.auth as Record<string, unknown> | undefined;
  const apiKey =
    typeof auth?.OPENAI_API_KEY === "string" ? auth.OPENAI_API_KEY : "";

  return {
    baseUrl: extractCodexBaseUrl(config) ?? "",
    apiKey,
    isFullUrl: provider.meta?.isFullUrl,
    modelsUrl: undefined,
    customUserAgent: provider.meta?.customUserAgent,
  };
}

/** 提取 Codex provider modelCatalog 里已配置的模型名（去重并保持顺序）。 */
export function codexConfiguredModelsOf(provider: Provider): string[] {
  const models: string[] = [];
  const seen = new Set<string>();
  const catalog = (provider.settingsConfig as Record<string, unknown>)
    ?.modelCatalog as { models?: Array<{ model?: unknown }> } | undefined;

  for (const entry of catalog?.models ?? []) {
    const value = entry?.model;
    if (typeof value !== "string" || !value.trim()) continue;
    const model = value.trim();
    if (seen.has(model)) continue;
    seen.add(model);
    models.push(model);
  }
  return models;
}

// 从各 provider env 里提取模型候选的环境变量名
const MODEL_ENV_KEYS = [
  "ANTHROPIC_MODEL",
  "ANTHROPIC_DEFAULT_HAIKU_MODEL",
  "ANTHROPIC_DEFAULT_SONNET_MODEL",
  "ANTHROPIC_DEFAULT_OPUS_MODEL",
  "ANTHROPIC_DEFAULT_FABLE_MODEL",
] as const;

/** 提取单个 provider 已配置的模型名（去重并保持 env 中的顺序）。 */
export function configuredModelsOf(provider: Provider): string[] {
  const models: string[] = [];
  const seen = new Set<string>();
  const env = (provider.settingsConfig as Record<string, unknown>)?.env as
    | Record<string, unknown>
    | undefined;
  if (!env) return models;

  for (const envKey of MODEL_ENV_KEYS) {
    const value = env[envKey];
    if (typeof value !== "string" || !value.trim()) continue;
    const model = value.trim();
    if (seen.has(model)) continue;
    seen.add(model);
    models.push(model);
  }
  return models;
}
