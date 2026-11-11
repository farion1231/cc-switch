import { useMemo } from "react";
import { useQueries } from "@tanstack/react-query";
import type { AppId } from "@/lib/api";
import * as authApi from "@/lib/api/auth";
import type { ManagedAuthProvider } from "@/lib/api/auth";
import { useProxyStatusQuery } from "@/lib/query/proxy";
import { proxyApi } from "@/lib/api/proxy";
import { useUsageSummary } from "@/lib/query/usage";
import { MODE_APP_IDS, isModeAppId } from "@/config/appConfig";
import { parseFiniteNumber } from "@/components/usage/format";
import type { UsageRangeSelection } from "@/types/usage";
import type { AppMode } from "@/types/proxy";

export interface AppNavStatus {
  mode: AppMode;
  /** 需要处理：在路由 / 聚合，但路由服务没在运行 */
  alert: boolean;
}

const TODAY: UsageRangeSelection = { preset: "today" };
const MANAGED_AUTH_PROVIDERS: ManagedAuthProvider[] = [
  "github_copilot",
  "codex_oauth",
  "xai_oauth",
];

/**
 * 侧栏需要的状态：每个应用的模式标签和提醒、今日花费与 Token、授权中心是否有账号要重新登录。
 */
export function useSidebarStatus() {
  const { data: proxyStatus } = useProxyStatusQuery();
  // 和应用页的模式行同一份数据（["providers", app, "mode"]），进出模式时一起刷新
  const modeQueries = useQueries({
    queries: MODE_APP_IDS.map((app) => ({
      queryKey: ["providers", app, "mode"],
      queryFn: () => proxyApi.getAppMode(app),
    })),
  });
  const { data: todaySummary } = useUsageSummary(TODAY, undefined, {
    refetchInterval: 60_000,
  });

  const authStatuses = useQueries({
    queries: MANAGED_AUTH_PROVIDERS.map((provider) => ({
      queryKey: ["managed-auth-status", provider],
      queryFn: () => authApi.authGetStatus(provider),
      staleTime: 60_000,
    })),
  });

  const serviceRunning = proxyStatus?.running ?? false;
  const modes = MODE_APP_IDS.map((_, index) => modeQueries[index]?.data?.mode);
  const modeKey = modes.join(",");

  const appStatus = useMemo(() => {
    return (app: AppId): AppNavStatus => {
      if (!isModeAppId(app)) {
        return { mode: "direct", alert: false };
      }
      const mode = modes[MODE_APP_IDS.indexOf(app)] ?? "direct";
      return {
        mode,
        alert:
          mode !== "direct" && proxyStatus !== undefined && !serviceRunning,
      };
    };
    // modes 是每次渲染新建的数组，用 modeKey 判断变化
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [proxyStatus, serviceRunning, modeKey]);

  const todayCost = parseFiniteNumber(todaySummary?.totalCost) ?? 0;
  // 和用量页「真实消耗 Tokens」同口径
  const todayTokens = todaySummary?.realTotalTokens ?? 0;
  const todayRequests = todaySummary?.totalRequests ?? 0;
  const todayHitRate = todaySummary?.cacheHitRate ?? 0;

  const authNeedsAttention = authStatuses.some((query) =>
    query.data?.accounts.some(
      (account) => account.requires_reauth || account.reauth_required === true,
    ),
  );

  return {
    appStatus,
    todayCost,
    todayTokens,
    todayRequests,
    todayHitRate,
    authNeedsAttention,
  };
}
