import { useEffect } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { usageSyncApi } from "@/lib/api/usageSync";
import { usageKeys } from "@/lib/query/usage";
import type {
  RemoteDeviceInfo,
  UsageSyncConflictResolution,
  UsageSyncPreferences,
} from "@/types/usageSync";

export const usageSyncKeys = {
  all: ["usage-sync"] as const,
  overview: () => [...usageSyncKeys.all, "overview"] as const,
};

/** 后台同步完成或状态变化时后端发出的事件 */
export const USAGE_SYNC_STATUS_EVENT = "usage-sync-status-updated";

export function useUsageSyncOverview(options?: { enabled?: boolean }) {
  return useQuery({
    queryKey: usageSyncKeys.overview(),
    queryFn: () => usageSyncApi.getOverview(),
    enabled: options?.enabled ?? true,
    // 设备的「N 分钟前同步」靠这里刷新；真正的变化走事件
    refetchInterval: 60_000,
    refetchIntervalInBackground: false,
  });
}

/** 收到后端同步状态事件后刷新概览与全部用量统计 */
export function useUsageSyncEventBridge() {
  const queryClient = useQueryClient();
  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    let disposed = false;
    (async () => {
      try {
        const off = await listen(USAGE_SYNC_STATUS_EVENT, () => {
          queryClient.invalidateQueries({ queryKey: usageSyncKeys.all });
          queryClient.invalidateQueries({ queryKey: usageKeys.all });
        });
        if (disposed) off();
        else unlisten = off;
      } catch {
        // 非 Tauri 环境（测试）没有事件通道，靠轮询兜底
      }
    })();
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [queryClient]);
}

function useInvalidateAll() {
  const queryClient = useQueryClient();
  return () => {
    queryClient.invalidateQueries({ queryKey: usageSyncKeys.all });
    queryClient.invalidateQueries({ queryKey: usageKeys.all });
  };
}

export function useRunUsageSync() {
  const invalidate = useInvalidateAll();
  return useMutation({
    mutationFn: () => usageSyncApi.runNow(),
    onSettled: invalidate,
  });
}

export function useSaveUsageSyncSettings() {
  const invalidate = useInvalidateAll();
  return useMutation({
    mutationFn: (preferences: UsageSyncPreferences) =>
      usageSyncApi.saveSettings(preferences),
    onSettled: invalidate,
  });
}

export function useSetUsageSyncDeviceName() {
  const invalidate = useInvalidateAll();
  return useMutation({
    mutationFn: (name: string | null) => usageSyncApi.setDeviceName(name),
    onSettled: invalidate,
  });
}

export function useSetUsageSyncDeviceHidden() {
  const invalidate = useInvalidateAll();
  return useMutation({
    mutationFn: ({ deviceId, hidden }: { deviceId: string; hidden: boolean }) =>
      usageSyncApi.setDeviceHidden(deviceId, hidden),
    onSettled: invalidate,
  });
}

export function useResolveUsageSyncConflict() {
  const invalidate = useInvalidateAll();
  return useMutation({
    mutationFn: (resolution: UsageSyncConflictResolution) =>
      usageSyncApi.resolveConflict(resolution),
    onSettled: invalidate,
  });
}

/** 该设备的数据是否过期：最近一次上传早于阈值，或从未上传过 */
export function isDeviceStale(
  device: Pick<RemoteDeviceInfo, "lastUploadAt">,
  staleAfterSecs: number,
  nowMs: number = Date.now(),
): boolean {
  if (device.lastUploadAt == null) return true;
  return nowMs / 1000 - device.lastUploadAt > staleAfterSecs;
}
