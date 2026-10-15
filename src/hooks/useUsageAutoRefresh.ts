import { useCallback, useEffect, useRef } from "react";
import { focusManager, useQueryClient } from "@tanstack/react-query";
import { usageApi } from "@/lib/api/usage";
import { usageKeys } from "@/lib/query/usage";
import type { SessionSyncResult } from "@/types/usage";

interface UsageAutoRefreshOptions {
  refreshIntervalMs: number;
  sessionAutoSyncEnabled: boolean;
  paused?: boolean;
  /** 时间范围随「现在」移动（今天、最近 24 小时等） */
  rollingRange?: boolean;
}

/** 随时间移动的范围多久重算一次：跨过午夜、滚动窗口移出旧记录都靠它 */
const ROLLING_RANGE_REFRESH_MS = 60_000;

/**
 * 用量页打开期间按所选间隔扫描会话日志：打开即扫一次，之后每隔一段扫一次，
 * 窗口隐藏时停、重新显示时立刻补扫。后台只有 10 分钟一次的兜底扫描。
 * 导入新用量后后端会发 `usage-log-recorded`，统计由 useUsageEventBridge 刷新；
 * 这里每轮只刷新「上次同步」时间，没有新数据时不重读整页。
 * 范围随时间移动时另有每分钟一次的重查，否则没有新用量时跨过午夜「今天」还停在昨天。
 */
export function useUsageAutoRefresh({
  refreshIntervalMs,
  sessionAutoSyncEnabled,
  paused = false,
  rollingRange = false,
}: UsageAutoRefreshOptions) {
  const queryClient = useQueryClient();
  const pendingSync = useRef<Promise<SessionSyncResult> | null>(null);

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
    if (!sessionAutoSyncEnabled || refreshIntervalMs <= 0 || paused) return;
    let disposed = false;

    const scan = async () => {
      if (!focusManager.isFocused() || pendingSync.current) return;
      try {
        const result = await syncSessionUsage();
        if (result.errors.length > 0) {
          console.warn(
            "[UsageDashboard] Session auto-sync errors",
            result.errors,
          );
        }
      } catch (error) {
        // 静默扫描失败不弹提示，下一轮继续重试
        console.warn("[UsageDashboard] Session auto-sync failed", error);
      }
      if (!disposed) {
        await queryClient.invalidateQueries({
          queryKey: [...usageKeys.all, "session-last-sync"],
        });
      }
    };

    void scan();
    const timer = window.setInterval(() => void scan(), refreshIntervalMs);
    const unsubscribe = focusManager.subscribe((focused) => {
      if (focused) void scan();
    });
    return () => {
      disposed = true;
      window.clearInterval(timer);
      unsubscribe();
    };
  }, [
    paused,
    queryClient,
    refreshIntervalMs,
    sessionAutoSyncEnabled,
    syncSessionUsage,
  ]);

  useEffect(() => {
    if (!rollingRange) return;
    const timer = window.setInterval(() => {
      if (focusManager.isFocused()) {
        void queryClient.invalidateQueries({ queryKey: usageKeys.all });
      }
    }, ROLLING_RANGE_REFRESH_MS);
    return () => window.clearInterval(timer);
  }, [queryClient, rollingRange]);

  return { syncSessionUsage };
}
