import { useRef } from "react";
import { useQuery, type UseQueryResult } from "@tanstack/react-query";
import { subscriptionApi } from "@/lib/api/subscription";
import type { AppId } from "@/lib/api/types";
import type { ProviderMeta } from "@/types";
import type { SubscriptionQuota } from "@/types/subscription";
import { resolveManagedAccountId } from "@/lib/authBinding";
import { PROVIDER_TYPES } from "@/config/constants";
import { resolveDisplayUsage, type LastGoodSnapshot } from "./queries";
import { extractErrorMessage } from "@/utils/errorUtils";

const REFETCH_INTERVAL = 5 * 60 * 1000; // 5 minutes

export const subscriptionKeys = {
  all: ["subscription"] as const,
  quota: (appId: AppId) => [...subscriptionKeys.all, "quota", appId] as const,
};

/**
 * reject 且无可展示值时的失败占位：首次查询就失败（data 为 undefined），或
 * react-query 保留的旧成功已超出 keep-last-good 窗口——合成一个失败结果，让
 * 订阅视图仍渲染「查询失败」+ 刷新按钮，而不是 footer 整体消失、无从手动重查。
 */
const QUERY_REJECTED_PLACEHOLDER: SubscriptionQuota = {
  tool: "",
  credentialStatus: "valid",
  credentialMessage: null,
  success: false,
  tiers: [],
  extraUsage: null,
  error: null,
  queriedAt: null,
};

/**
 * Keep-last-good：与 useUsageQuery 同一策略（resolveDisplayUsage）。
 *
 * 后端对纯传输失败（网络/超时/读体中断）已 reject——react-query 保留上次 data
 * 并触发 retry，但那份 data 是陈旧的：以 `rejected` 标志交给 resolveDisplayUsage
 * 按同一窗口处理（窗口内继续展示，超窗透出失败），与仍以 `Ok(success:false)`
 * 返回的瞬时失败（HTTP 5xx/429）行为一致。确定性失败（过期/鉴权/解析）不掩盖，
 * 立即透出。
 *
 * `scopeKey` 标识查询身份（appId / 绑定的账号 id）：身份变化时丢弃旧快照，
 * 避免用上一个账号的额度掩盖新账号的瞬时失败。
 */
function useQuotaKeepLastGood(
  query: UseQueryResult<SubscriptionQuota>,
  scopeKey: string,
) {
  const lastGoodRef = useRef<{
    key: string;
    snap: LastGoodSnapshot<SubscriptionQuota> | null;
  }>({ key: scopeKey, snap: null });
  if (lastGoodRef.current.key !== scopeKey) {
    lastGoodRef.current = { key: scopeKey, snap: null };
  }
  const { data, lastGood } = resolveDisplayUsage(
    query.data,
    query.dataUpdatedAt,
    lastGoodRef.current.snap,
    Date.now(),
    { rejected: query.isError },
  );
  lastGoodRef.current.snap = lastGood;
  return {
    ...query,
    data:
      data ??
      (query.isError
        ? {
            ...QUERY_REJECTED_PLACEHOLDER,
            error: extractErrorMessage(query.error) || null,
          }
        : undefined),
  };
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

  const query = useQuery({
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

  return useQuotaKeepLastGood(query, appId);
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
  const query = useQuery({
    queryKey: ["codex_oauth", "quota", accountId ?? "default"],
    queryFn: () => subscriptionApi.getCodexOauthQuota(accountId),
    enabled,
    refetchInterval: autoQuery ? REFETCH_INTERVAL : false,
    refetchIntervalInBackground: autoQuery,
    refetchOnWindowFocus: autoQuery,
    staleTime: REFETCH_INTERVAL,
    retry: 1,
  });

  return useQuotaKeepLastGood(query, accountId ?? "default");
}
