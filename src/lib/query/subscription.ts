import { useQuery } from "@tanstack/react-query";
import { subscriptionApi } from "@/lib/api/subscription";
import type { AppId } from "@/lib/api/types";
import type { ProviderMeta } from "@/types";
import { resolveManagedAccountId } from "@/lib/authBinding";
import { PROVIDER_TYPES } from "@/config/constants";

const REFETCH_INTERVAL = 5 * 60 * 1000; // 5 minutes

export const subscriptionKeys = {
  all: ["subscription"] as const,
  quota: (appId: AppId) => [...subscriptionKeys.all, "quota", appId] as const,
  allCodexQuotas: () =>
    [...subscriptionKeys.all, "codex", "all-quotas"] as const,
  codexAll: () => subscriptionKeys.allCodexQuotas(),
};

interface CodexAllQuotasQueryOptions {
  enabled: boolean;
  refetchInterval: number | false;
  refetchIntervalInBackground: boolean;
  refetchOnWindowFocus: boolean;
  staleTime: number;
}

function useCodexAllQuotasQuery(options: CodexAllQuotasQueryOptions) {
  return useQuery({
    queryKey: subscriptionKeys.allCodexQuotas(),
    queryFn: () => subscriptionApi.getAllCodexQuotas(),
    enabled: options.enabled,
    refetchInterval: options.refetchInterval,
    refetchIntervalInBackground: options.refetchIntervalInBackground,
    refetchOnWindowFocus: options.refetchOnWindowFocus,
    staleTime: options.staleTime,
    retry: 1,
  });
}

export function useSubscriptionQuota(
  appId: AppId,
  enabled: boolean,
  autoQuery = false,
  autoQueryIntervalMinutes = 5,
) {
  const refetchInterval =
    autoQuery && autoQueryIntervalMinutes > 0
      ? Math.max(autoQueryIntervalMinutes, 1) * 60 * 1000
      : false;

  return useQuery({
    queryKey: subscriptionKeys.quota(appId),
    queryFn: () => subscriptionApi.getQuota(appId),
    enabled: enabled && ["claude", "codex", "gemini"].includes(appId),
    refetchInterval,
    refetchIntervalInBackground: Boolean(refetchInterval),
    refetchOnWindowFocus: Boolean(refetchInterval),
    staleTime:
      autoQueryIntervalMinutes > 0
        ? Math.max(autoQueryIntervalMinutes, 1) * 60 * 1000
        : REFETCH_INTERVAL,
    retry: 1,
  });
}

/**
 * 查询所有 Codex 账号的官方用量（5h / 7d 窗口）
 *
 * 返回 accountKey -> SubscriptionQuota 的映射。
 * 默认每 5 分钟自动刷新一次。
 */
export function useAllCodexQuotas(
  enabled = true,
  intervalMs: number = REFETCH_INTERVAL,
) {
  const autoRefresh = intervalMs > 0;

  return useCodexAllQuotasQuery({
    enabled,
    refetchInterval: intervalMs > 0 ? intervalMs : false,
    refetchIntervalInBackground: autoRefresh,
    refetchOnWindowFocus: autoRefresh,
    staleTime: intervalMs,
  });
}

export interface UseCodexAllQuotasOptions {
  enabled?: boolean;
  /** 是否启用自动轮询（与 settings 中的 usage_refresh_interval_secs 同步） */
  autoQuery?: boolean;
  /** 刷新间隔（毫秒，默认 60 秒） */
  intervalMs?: number;
}

/**
 * 查询所有 Codex 账号的用量
 *
 * 与 `useUsageCacheBridge` 共享同一个 query key，所以托盘触发的后端刷新
 * 可以通过 React Query 缓存立刻反映到主界面。
 */
export function useCodexAllQuotas(options: UseCodexAllQuotasOptions = {}) {
  const { enabled = true, autoQuery = false, intervalMs = 60_000 } = options;

  return useCodexAllQuotasQuery({
    enabled,
    refetchInterval: autoQuery ? intervalMs : false,
    refetchIntervalInBackground: autoQuery,
    refetchOnWindowFocus: autoQuery,
    staleTime: intervalMs,
  });
}

export interface UseCodexOauthQuotaOptions {
  enabled?: boolean;
  /** 是否启用自动轮询（5 分钟）与窗口 focus 重取 */
  autoQuery?: boolean;
}

/**
 * Codex OAuth (ChatGPT Plus/Pro 反代) 订阅额度查询 hook
 *
 * 与 `useSubscriptionQuota` 平行：数据走 cc-switch 自管的 OAuth token，
 * 而不是 Codex CLI 的 ~/.codex/auth.json。
 *
 * Query key 包含 accountId，多张卡片绑定到同一账号时会自动去重共享请求。
 * accountId 为 null 时使用 "default" 占位，让后端 fallback 到默认账号。
 */
export function useCodexOauthQuota(
  meta: ProviderMeta | undefined,
  options: UseCodexOauthQuotaOptions = {},
) {
  const { enabled = true, autoQuery = false } = options;
  const accountId = resolveManagedAccountId(meta, PROVIDER_TYPES.CODEX_OAUTH);
  return useQuery({
    queryKey: ["codex_oauth", "quota", accountId ?? "default"],
    queryFn: () => subscriptionApi.getCodexOauthQuota(accountId),
    enabled,
    refetchInterval: autoQuery ? REFETCH_INTERVAL : false,
    refetchIntervalInBackground: autoQuery,
    refetchOnWindowFocus: autoQuery,
    staleTime: REFETCH_INTERVAL,
    retry: 1,
  });
}
