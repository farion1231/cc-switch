import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useCallback } from "react";
import type { TFunction } from "i18next";
import {
  appTargetsApi,
  distroOf,
  isAppTargetApp,
  type AppTargetAppId,
  type AppTargetsOverview,
  type TargetKey,
  type TargetReport,
} from "@/lib/api/appTargets";

export const APP_TARGETS_QUERY_KEY = ["appTargets", "overview"] as const;

/** 设置页 / 主页的多环境概览。不会为它启动停止的发行版。 */
export function useAppTargetsOverview(enabled = true) {
  return useQuery<AppTargetsOverview>({
    queryKey: APP_TARGETS_QUERY_KEY,
    queryFn: () => appTargetsApi.overview(false),
    enabled,
    // 发行版空闲一会儿会自动停下：运行状态要跟着刷新（枚举本身不启动发行版）
    staleTime: 15_000,
    refetchInterval: enabled ? 30_000 : false,
    refetchOnWindowFocus: true,
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

export function useAppTargetView(app: string) {
  // 只有四个支持多环境的应用才查（Windows 上会调用 wsl.exe 枚举发行版）。
  const query = useAppTargetsOverview(isAppTargetApp(app));
  return query.data?.apps.find((view) => view.app === app);
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

export const failedTargets = (reports: TargetReport[]) =>
  reports.filter((report) => report.status === "error");

export type { AppTargetAppId };
