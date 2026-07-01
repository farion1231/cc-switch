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
};

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

export interface UseCodexOauthQuotaOptions {
  enabled?: boolean;
  /** 是否启用自动轮询（5 分钟）与窗口 focus 重取 */
  autoQuery?: boolean;
}

/**
 * Codex OAuth 订阅额度查询 hook（按账号 ID）
 *
 * 直接以 cc-switch 自管的 ChatGPT 账号 ID 查询额度，供认证中心里逐个账号
 * 展示用量时复用。Query key 与 `useCodexOauthQuota` 一致，绑定到同一账号的
 * 供应商卡片与账号列表会自动去重共享同一份请求缓存。
 * accountId 为 null 时使用 "default" 占位，让后端 fallback 到默认账号。
 */
export function useCodexOauthQuotaByAccountId(
  accountId: string | null,
  options: UseCodexOauthQuotaOptions = {},
) {
  const { enabled = true, autoQuery = false } = options;
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

/**
 * Codex OAuth (ChatGPT Plus/Pro 反代) 订阅额度查询 hook
 *
 * 与 `useSubscriptionQuota` 平行：数据走 cc-switch 自管的 OAuth token，
 * 而不是 Codex CLI 的 ~/.codex/auth.json。账号 ID 从供应商 meta 的
 * authBinding 中解析，再委托给 `useCodexOauthQuotaByAccountId`。
 */
export function useCodexOauthQuota(
  meta: ProviderMeta | undefined,
  options: UseCodexOauthQuotaOptions = {},
) {
  const accountId = resolveManagedAccountId(meta, PROVIDER_TYPES.CODEX_OAUTH);
  return useCodexOauthQuotaByAccountId(accountId, options);
}
