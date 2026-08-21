import { useEffect } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useQueryClient } from "@tanstack/react-query";
import { usageKeys } from "@/lib/query/usage";

const USAGE_EVENT_INVALIDATION_THROTTLE_MS = 5000;

/**
 * 监听后端 `usage-log-recorded` 事件，合并短时间内的连续写入后 invalidate
 * UsageDashboard 相关查询，让用户无需等待完整的轮询周期。
 *
 * 后端在 `proxy_request_logs` 写入新行时会 emit 该事件（200ms 防抖合并），
 * 来源覆盖代理日志、Claude/Codex/Gemini 会话同步、启动归档。
 *
 * 该 hook 只挂在 UsageDashboard 上，避免在主界面其他位置无意义触发。
 */
export function useUsageEventBridge(enabled = true) {
  const queryClient = useQueryClient();

  useEffect(() => {
    if (!enabled) return;

    let unlisten: UnlistenFn | undefined;
    let disposed = false;
    let trailingTimer: ReturnType<typeof setTimeout> | undefined;
    let lastInvalidatedAt: number | undefined;

    const invalidateUsageQueries = () => {
      lastInvalidatedAt = Date.now();
      void queryClient.invalidateQueries({
        queryKey: usageKeys.all,
        refetchType: "active",
      });
    };

    const scheduleInvalidation = () => {
      if (lastInvalidatedAt === undefined) {
        invalidateUsageQueries();
        return;
      }

      const elapsed = Date.now() - lastInvalidatedAt;
      if (elapsed >= USAGE_EVENT_INVALIDATION_THROTTLE_MS) {
        if (trailingTimer !== undefined) {
          clearTimeout(trailingTimer);
          trailingTimer = undefined;
        }
        invalidateUsageQueries();
        return;
      }

      if (trailingTimer !== undefined) return;

      trailingTimer = setTimeout(() => {
        trailingTimer = undefined;
        if (!disposed) invalidateUsageQueries();
      }, USAGE_EVENT_INVALIDATION_THROTTLE_MS - elapsed);
    };

    (async () => {
      const off = await listen("usage-log-recorded", scheduleInvalidation);

      if (disposed) {
        off();
      } else {
        unlisten = off;
      }
    })();

    return () => {
      disposed = true;
      if (trailingTimer !== undefined) clearTimeout(trailingTimer);
      unlisten?.();
    };
  }, [enabled, queryClient]);
}
