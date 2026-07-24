import type { AggregateRoute, AggregateRoutes, Provider } from "@/types";
import { providerPresets } from "@/config/claudeProviderPresets";

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

/** 路由表是否至少配置（或填写）了一档 */
export function hasAggregateRoutes(routes?: AggregateRoutes | null): boolean {
  if (!routes) return false;
  return AGGREGATE_ROUTE_TIERS.some((tier) => tierHasAnyInput(routes[tier]));
}

/** 是否为聚合供应商（meta.aggregateRoutes 非空） */
export function isAggregateProvider(provider: Pick<Provider, "meta">): boolean {
  return hasAggregateRoutes(provider.meta?.aggregateRoutes);
}

/**
 * 可作为聚合路由目标的供应商列表：
 * 排除聚合供应商自身（不允许嵌套）与当前正在编辑的供应商（不允许自指）。
 */
export function getAggregateRouteTargets(
  providers: Provider[],
  excludeProviderId?: string,
): Provider[] {
  return providers.filter(
    (provider) =>
      provider.id !== excludeProviderId &&
      provider.category !== "official" &&
      !isAggregateProvider(provider),
  );
}

/** 归一化路由表：trim，仅保留 providerId 与 model 均非空的档位 */
export function normalizeAggregateRoutes(
  routes: AggregateRoutes,
): AggregateRoutes {
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
  | { ok: false; reason: "incomplete"; tier: AggregateRouteTier };

/**
 * 提交前校验：
 * - 至少配置一档；
 * - 动了某档（provider/model 填了任意一个）就要求两者都非空。
 */
export function validateAggregateRoutes(
  routes: AggregateRoutes,
): AggregateRoutesValidation {
  for (const tier of AGGREGATE_ROUTE_TIERS) {
    const route = routes[tier];
    if (!route) continue;
    const hasProvider = Boolean(route.providerId?.trim());
    const hasModel = Boolean(route.model?.trim());
    if (hasProvider !== hasModel) {
      return { ok: false, reason: "incomplete", tier };
    }
  }

  const normalized = normalizeAggregateRoutes(routes);
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
