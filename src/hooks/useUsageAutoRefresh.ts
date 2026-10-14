import { useCallback, useEffect, useRef } from "react";
import { focusManager, useQueryClient } from "@tanstack/react-query";
import { usageApi } from "@/lib/api/usage";
import { usageKeys } from "@/lib/query/usage";
import type { SessionSyncResult } from "@/types/usage";

interface UsageAutoRefreshOptions {
  refreshIntervalMs: number;
  sessionAutoSyncEnabled: boolean;
  paused?: boolean;
}

/** 页面刷新统一走「扫描会话 → 重取统计」，避免短间隔只读到旧的数据库内容。 */
export function useUsageAutoRefresh({
  refreshIntervalMs,
  sessionAutoSyncEnabled,
  paused = false,
}: UsageAutoRefreshOptions) {
  const queryClient = useQueryClient();
  const pendingSync = useRef<Promise<SessionSyncResult> | null>(null);
  const refreshing = useRef(false);

  // 手动同步复用尚未完成的静默扫描；慢扫描不会在后端互斥锁前排队累积。
  const syncSessionUsage = useCallback(() => {
    if (!pendingSync.current) {
      pendingSync.current = usageApi.syncSessionUsage().finally(() => {
        pendingSync.current = null;
      });
    }
    return pendingSync.current;
  }, []);

  useEffect(() => {
    if (refreshIntervalMs <= 0 || paused) return;
    let disposed = false;

    const refresh = async () => {
      // 与原查询轮询保持一致：隐藏窗口不额外扫描，后台的 60 秒扫描仍独立运行。
      if (
        !focusManager.isFocused() ||
        refreshing.current ||
        pendingSync.current
      ) {
        return;
      }
      refreshing.current = true;
      try {
        if (sessionAutoSyncEnabled) {
          try {
            const result = await syncSessionUsage();
            if (result.errors.length > 0) {
              console.warn(
                "[UsageDashboard] Session auto-sync errors",
                result.errors,
              );
            }
          } catch (error) {
            // 扫描失败也要更新已经入库的路由用量；下一轮继续重试，不弹提示。
            console.warn("[UsageDashboard] Session auto-sync failed", error);
          }
        }
        if (!disposed && focusManager.isFocused()) {
          await queryClient.invalidateQueries({ queryKey: usageKeys.all });
        }
      } finally {
        refreshing.current = false;
      }
    };

    const timer = window.setInterval(() => void refresh(), refreshIntervalMs);
    return () => {
      disposed = true;
      window.clearInterval(timer);
    };
  }, [
    paused,
    queryClient,
    refreshIntervalMs,
    sessionAutoSyncEnabled,
    syncSessionUsage,
  ]);

  return { syncSessionUsage };
}
