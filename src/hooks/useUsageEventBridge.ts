import { useEffect } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useQueryClient } from "@tanstack/react-query";
import { usageKeys } from "@/lib/query/usage";

/**
 * 监听后端 `usage-log-recorded` 事件，收到后立刻 invalidate 所有
 * UsageDashboard 相关查询。用量页不再定时轮询，统计全靠它刷新。
 *
 * 后端在 `proxy_request_logs` 写入新行时会 emit 该事件（200ms 防抖合并），
 * 来源覆盖代理日志、Claude/Codex/Gemini 会话同步、启动归档。
 *
 * 该 hook 只挂在 UsageDashboard 上，避免在主界面其他位置无意义触发。
 */
export function useUsageEventBridge() {
  const queryClient = useQueryClient();

  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    let disposed = false;
    let refreshing = false;
    let dirty = false;

    const refresh = async () => {
      refreshing = true;
      try {
        while (dirty && !disposed) {
          dirty = false;
          // An initial fetch or another refresh may have captured its snapshot
          // before this event. Joining that work saves a backend call, but still
          // needs one trailing refresh to include the newly written logs.
          if (
            queryClient.isFetching({
              queryKey: usageKeys.all,
              type: "active",
            }) > 0
          ) {
            dirty = true;
          }
          // Cancelling React Query cannot cancel an already-running Tauri DB
          // query. Keep one refresh running and coalesce events into a dirty bit.
          await queryClient.invalidateQueries(
            { queryKey: usageKeys.all },
            { cancelRefetch: false },
          );
        }
      } finally {
        refreshing = false;
      }
    };

    (async () => {
      const off = await listen("usage-log-recorded", () => {
        if (disposed) return;
        // invalidate 整个 usage 命名空间：summary / trends / providerStats /
        // modelStats / logs 全部跟着重拉
        dirty = true;
        if (!refreshing) void refresh();
      });

      if (disposed) {
        off();
      } else {
        unlisten = off;
      }
    })();

    return () => {
      disposed = true;
      dirty = false;
      unlisten?.();
    };
  }, [queryClient]);
}
