import { useEffect } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useQueryClient } from "@tanstack/react-query";
import { budgetKeys } from "@/lib/query/budget";

/**
 * 监听后端 `usage-log-recorded` 事件，invalidate 所有 budget 查询。
 * 使 BudgetDashboard 进度条在产生新请求时实时刷新。
 * 仅在 BudgetDashboard 挂载时生效，离开页面自动取消监听。
 */
export function useBudgetEventBridge() {
  const queryClient = useQueryClient();

  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    let disposed = false;

    (async () => {
      const off = await listen("usage-log-recorded", () => {
        queryClient.invalidateQueries({ queryKey: budgetKeys.all });
      });
      if (disposed) {
        off();
      } else {
        unlisten = off;
      }
    })();

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [queryClient]);
}
