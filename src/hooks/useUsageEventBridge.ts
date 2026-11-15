import { focusManager, useQueryClient } from "@tanstack/react-query";
import { usageKeys } from "@/lib/query/usage";
import { useTauriEvent } from "./useTauriEvent";

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

  useTauriEvent("usage-log-recorded", () => {
    // Mark inactive-window data stale without repeatedly querying SQLite. The
    // normal focus refetch catches up when the user returns to the window.
    void queryClient.invalidateQueries({
      queryKey: usageKeys.all,
      refetchType: focusManager.isFocused() ? "active" : "none",
    });
  });
}
