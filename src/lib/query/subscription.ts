import { useRef } from "react";
import {
  useQuery,
  useQueryClient,
  type QueryClient,
  type UseQueryResult,
} from "@tanstack/react-query";
import { subscriptionApi } from "@/lib/api/subscription";
import { kimiOAuthApi } from "@/lib/api/kimiOAuth";
import type { AppId } from "@/lib/api/types";
import type { ProviderMeta } from "@/types";
import type {
  ClaudeAccountPlan,
  SubscriptionQuota,
} from "@/types/subscription";
import type { ManagedAuthStatus } from "@/lib/api/auth";
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
    enabled:
      enabled &&
      ["claude", "codex", "gemini", "grokbuild", "agy"].includes(appId),
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
  /** 是否启用自动轮询与窗口 focus 重取 */
  autoQuery?: boolean;
  autoQueryIntervalMinutes?: number;
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
  const {
    enabled = true,
    autoQuery = false,
    autoQueryIntervalMinutes = 5,
  } = options;
  const refetchInterval =
    autoQuery && autoQueryIntervalMinutes > 0
      ? Math.max(autoQueryIntervalMinutes, 1) * 60 * 1000
      : false;
  const query = useQuery({
    queryKey: ["codex_oauth", "quota", accountId ?? "default"],
    queryFn: () => subscriptionApi.getCodexOauthQuota(accountId),
    enabled,
    refetchInterval,
    refetchIntervalInBackground: Boolean(refetchInterval),
    refetchOnWindowFocus: Boolean(refetchInterval),
    staleTime:
      autoQueryIntervalMinutes > 0
        ? Math.max(autoQueryIntervalMinutes, 1) * 60 * 1000
        : REFETCH_INTERVAL,
    retry: 1,
  });

  return useQuotaKeepLastGood(query, accountId ?? "default");
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

/**
 * xAI OAuth (SuperGrok 反代) 订阅额度查询 hook
 *
 * 与 `useCodexOauthQuota` 平行：数据走 cc-switch 自管的 xAI OAuth token，
 * 而不是 Grok CLI 的 ~/.grok/auth.json；后端复用同一个 grok.com 账单端点，
 * 因此与 Grok Build 分区的官方订阅显示同一份额度。
 */
export function useXaiOauthQuota(
  meta: ProviderMeta | undefined,
  options: UseCodexOauthQuotaOptions = {},
) {
  const accountId = resolveManagedAccountId(meta, PROVIDER_TYPES.XAI_OAUTH);
  return useXaiOauthQuotaByAccountId(accountId, options);
}

/**
 * xAI OAuth 订阅额度（按账号 ID）：授权中心逐个账号显示额度时用。
 * Query key 与 `useXaiOauthQuota` 一致，绑定同一账号的供应商卡片共用缓存。
 */
export function useXaiOauthQuotaByAccountId(
  accountId: string | null,
  options: UseCodexOauthQuotaOptions = {},
) {
  const { enabled = true, autoQuery = false } = options;
  const query = useQuery({
    queryKey: ["xai_oauth", "quota", accountId ?? "default"],
    queryFn: () => subscriptionApi.getXaiOauthQuota(accountId),
    enabled,
    refetchInterval: autoQuery ? REFETCH_INTERVAL : false,
    refetchIntervalInBackground: autoQuery,
    refetchOnWindowFocus: autoQuery,
    staleTime: REFETCH_INTERVAL,
    retry: 1,
  });

  return useQuotaKeepLastGood(query, accountId ?? "default");
}

/**
 * Antigravity 账号的剩余额度（按账号 ID，用该账号自己的令牌直接查，不用先切号）。
 * 授权中心逐个账号、以及绑了这个账号的官方卡共用同一份缓存。
 */
export function useAgyAccountQuota(
  accountId: string,
  options: UseCodexOauthQuotaOptions = {},
) {
  const {
    enabled = true,
    autoQuery = false,
    autoQueryIntervalMinutes = 5,
  } = options;
  const refetchInterval =
    autoQuery && autoQueryIntervalMinutes > 0
      ? Math.max(autoQueryIntervalMinutes, 1) * 60 * 1000
      : false;
  const query = useQuery({
    queryKey: ["agy_oauth", "quota", accountId],
    queryFn: () => subscriptionApi.getAgyAccountQuota(accountId),
    enabled,
    refetchInterval,
    refetchIntervalInBackground: Boolean(refetchInterval),
    refetchOnWindowFocus: Boolean(refetchInterval),
    staleTime: REFETCH_INTERVAL,
    retry: 1,
  });

  return useQuotaKeepLastGood(query, accountId);
}

/**
 * Kimi 官方账号的剩余额度（按授权中心账号 ID，也就是它的供应商 ID）。
 * 令牌快过期时后端会拿 Kimi 同款文件锁续期；授权中心那一行和官方卡共用同一份缓存。
 */
export function useKimiAccountQuota(
  accountId: string,
  options: UseCodexOauthQuotaOptions = {},
) {
  const {
    enabled = true,
    autoQuery = false,
    autoQueryIntervalMinutes = 5,
  } = options;
  const refetchInterval =
    autoQuery && autoQueryIntervalMinutes > 0
      ? Math.max(autoQueryIntervalMinutes, 1) * 60 * 1000
      : false;
  const query = useQuery({
    queryKey: ["kimi_oauth", "quota", accountId],
    queryFn: () => kimiOAuthApi.quota(accountId),
    enabled,
    refetchInterval,
    refetchIntervalInBackground: Boolean(refetchInterval),
    refetchOnWindowFocus: Boolean(refetchInterval),
    staleTime: REFETCH_INTERVAL,
    retry: 1,
  });

  return useQuotaKeepLastGood(query, accountId);
}

/**
 * 绑了账号的 Claude 官方卡额度（按账号查）：在用的号读默认登录，没在用的读它自己的账号目录。
 * 令牌过期时后端会在那个目录里跑一次 `claude -p /usage`（Claude Code 自己刷新），可能要几秒。
 */
export function useClaudeAccountQuota(
  accountId: string,
  options: UseCodexOauthQuotaOptions = {},
) {
  const {
    enabled = true,
    autoQuery = false,
    autoQueryIntervalMinutes = 5,
  } = options;
  const refetchInterval =
    autoQuery && autoQueryIntervalMinutes > 0
      ? Math.max(autoQueryIntervalMinutes, 1) * 60 * 1000
      : false;
  const queryClient = useQueryClient();
  const query = useQuery({
    queryKey: ["claude_official", "quota", accountId],
    queryFn: async () => {
      const quota = await subscriptionApi.getClaudeAccountQuota(accountId);
      refreshAccountsOnPlanChange(queryClient, accountId, quota.plan);
      return quota;
    },
    enabled,
    refetchInterval,
    refetchIntervalInBackground: Boolean(refetchInterval),
    refetchOnWindowFocus: Boolean(refetchInterval),
    staleTime: REFETCH_INTERVAL,
    retry: 1,
  });

  return useQuotaKeepLastGood(query, accountId);
}

/**
 * 查额度时后端顺手把凭据里的套餐记进了账号；和账号列表里的不一样（升级 / 降级过）时重新拉
 * 列表，卡片副标题和授权中心显示的套餐就跟着变。
 */
function refreshAccountsOnPlanChange(
  queryClient: QueryClient,
  accountId: string,
  plan: ClaudeAccountPlan | null | undefined,
) {
  if (!plan?.subscriptionType) return;
  const statusKey = ["managed-auth-status", "claude_official"];
  const account = queryClient
    .getQueryData<ManagedAuthStatus>(statusKey)
    ?.accounts.find((item) => item.id === accountId);
  if (!account) return;
  if (
    (account.plan ?? null) !== plan.subscriptionType ||
    (account.rate_limit_tier ?? null) !== (plan.rateLimitTier ?? null)
  ) {
    void queryClient.invalidateQueries({ queryKey: statusKey });
  }
}

/**
 * Claude Code 默认登录的套餐（没绑账号的官方卡显示）。默认登录会随切卡、在 Claude Code 里
 * 重新登录而变，所以不久存，回到窗口时重读。
 */
export function useClaudeDefaultPlan(enabled: boolean) {
  return useQuery({
    queryKey: ["claude_official", "default_plan"],
    queryFn: () => subscriptionApi.getClaudeDefaultPlan(),
    enabled,
    staleTime: 60 * 1000,
    refetchOnWindowFocus: true,
    retry: false,
  });
}
