import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useCallback } from "react";
import type { TFunction } from "i18next";
import {
  appTargetsApi,
  distroOf,
  type AppTargetsOverview,
  type TargetKey,
} from "@/lib/api/appTargets";

export const APP_TARGETS_QUERY_KEY = ["appTargets", "overview"] as const;
export const WSL_ROUTE_INFO_QUERY_KEY = ["appTargets", "wslRouteInfo"] as const;

/** 设置页 / 主页的多环境概览。不会为它启动停止的发行版。 */
export function useAppTargetsOverview(enabled = true) {
  return useQuery<AppTargetsOverview>({
    queryKey: APP_TARGETS_QUERY_KEY,
    queryFn: () => appTargetsApi.overview(false),
    enabled,
    // 只有 Windows 跟踪 WSL 启停；macOS / Linux 保留原有刷新策略。
    staleTime: (query) =>
      query.state.data?.inventory.hostWindows ? 5_000 : 15_000,
    refetchInterval: (query) =>
      enabled
        ? query.state.data?.inventory.hostWindows
          ? 5_000
          : 30_000
        : false,
    refetchOnWindowFocus: (query) =>
      query.state.data?.inventory.hostWindows ? "always" : true,
  });
}

export function useInvalidateAppTargets() {
  const queryClient = useQueryClient();
  return useCallback(
    () =>
      Promise.all([
        queryClient.invalidateQueries({ queryKey: APP_TARGETS_QUERY_KEY }),
        queryClient.invalidateQueries({ queryKey: ["settings"] }),
      ]),
    [queryClient],
  );
}

/** 环境名称：`Windows` / `WSL · Ubuntu` */
export const targetLabel = (key: TargetKey) => {
  const distro = distroOf(key);
  return distro ? `WSL · ${distro}` : "Windows";
};

/** 状态文字（与原型一致）。 */
export const statusLabel = (t: TFunction, status: string) =>
  t(`appTargets.status.${status}`, { defaultValue: status });

/** 原因代码的说明；未知代码原样显示。 */
export const reasonLabel = (t: TFunction, reason?: string) =>
  reason ? t(`appTargets.reason.${reason}`, { defaultValue: reason }) : "";

export const syncFailureWarnings = (warnings: string[] = []) =>
  warnings.filter((warning) =>
    ["mcp_failed:", "prompt_failed:", "skills_failed:"].some((prefix) =>
      warning.startsWith(prefix),
    ),
  );

export function syncWarningLabel(t: TFunction, warning: string): string {
  const separator = warning.indexOf(":");
  return t(`appTargets.syncFailure.${warning.slice(0, separator)}`, {
    detail: warning.slice(separator + 1),
  });
}
