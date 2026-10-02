import { useMemo } from "react";
import { useQueries } from "@tanstack/react-query";
import type { AppId } from "@/lib/api";
import * as authApi from "@/lib/api/auth";
import type { ManagedAuthProvider } from "@/lib/api/auth";
import { useProvidersQuery, useSettingsQuery } from "@/lib/query";
import { useProxyStatusQuery, useProxyTakeoverStatus } from "@/lib/query/proxy";
import { useUsageSummary } from "@/lib/query/usage";
import { isProxyAppId, isStackAppId } from "@/config/appConfig";
import { providerNeedsRouting } from "@/utils/providerCapabilities";
import { parseFiniteNumber } from "@/components/usage/format";
import type { UsageRangeSelection } from "@/types/usage";

/** 应用当前生效的连接方式。直连不在侧栏显示标签。 */
export type AppMode = "direct" | "route" | "stack";

export interface AppNavStatus {
  mode: AppMode;
  /** Claude Desktop 的当前供应商走模型映射（需要路由服务） */
  mapping: boolean;
  /** 需要处理：在路由 / 叠加 / 映射，但路由服务没在运行 */
  alert: boolean;
}

const TODAY: UsageRangeSelection = { preset: "today" };
const MANAGED_AUTH_PROVIDERS: ManagedAuthProvider[] = [
  "github_copilot",
  "codex_oauth",
  "xai_oauth",
];

/**
 * 侧栏需要的状态：每个应用的模式标签和提醒、今日花费、授权中心是否有账号要重新登录。
 */
export function useSidebarStatus() {
  const { data: settings } = useSettingsQuery();
  const { data: proxyStatus } = useProxyStatusQuery();
  const { data: takeover } = useProxyTakeoverStatus(false);
  const { data: desktopProviders } = useProvidersQuery("claude-desktop");
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
  const stackModeEnabled = settings?.enableStackMode ?? false;

  const desktopMapping = useMemo(() => {
    const current =
      desktopProviders?.providers[desktopProviders.currentProviderId];
    return current ? providerNeedsRouting("claude-desktop", current) : false;
  }, [desktopProviders]);

  const appStatus = useMemo(() => {
    return (app: AppId): AppNavStatus => {
      if (app === "claude-desktop") {
        return {
          mode: "direct",
          mapping: desktopMapping,
          alert: desktopMapping && proxyStatus !== undefined && !serviceRunning,
        };
      }
      if (!isProxyAppId(app) || !takeover?.[app]) {
        return { mode: "direct", mapping: false, alert: false };
      }
      const mode: AppMode =
        stackModeEnabled && isStackAppId(app) ? "stack" : "route";
      return {
        mode,
        mapping: false,
        alert: proxyStatus !== undefined && !serviceRunning,
      };
    };
  }, [desktopMapping, proxyStatus, serviceRunning, stackModeEnabled, takeover]);

  const todayCost = parseFiniteNumber(todaySummary?.totalCost) ?? 0;

  const authNeedsAttention = authStatuses.some((query) =>
    query.data?.accounts.some(
      (account) => account.requires_reauth || account.reauth_required === true,
    ),
  );

  return { appStatus, todayCost, authNeedsAttention };
}
