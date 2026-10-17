/**
 * 订阅 cc-switch remote server 的 SSE 事件流，让前端 React Query 在控制面
 * （远程 REST API / 脚本）改动代理状态时自动 invalidate，UI 立即同步。
 *
 * 设计动机：
 * - cc-switch v3.20.1 (#5928) 重构后去掉了 useProxyTakeoverStatus 的轮询以节省 idle GPU
 * - 但远程 API（/api/proxy/control）改状态后，桌面 app 没有通知链路
 * - 后端 sse_tx 在接管状态变更时 broadcast EventSource，前端订阅后 invalidate React Query
 *
 * 用法：在 App.tsx 顶层挂一次即可
 */

import { useEffect } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { proxyKeys } from "@/lib/query/proxy";

const REMOTE_PORT = 4000;

export function useProxyEventStream() {
  const queryClient = useQueryClient();

  useEffect(() => {
    const es = new EventSource(`http://127.0.0.1:${REMOTE_PORT}/api/events`);

    es.onmessage = (event) => {
      try {
        const data = JSON.parse(event.data);
        switch (data.type) {
          case "proxy_started":
          case "proxy_stopped":
            queryClient.invalidateQueries({ queryKey: proxyKeys.status });
            queryClient.invalidateQueries({
              queryKey: proxyKeys.takeoverStatus,
            });
            break;
          case "takeover_changed":
          case "live_configs_changed":
          case "switch":
          case "shutdown":
            queryClient.invalidateQueries({
              queryKey: proxyKeys.takeoverStatus,
            });
            break;
        }
      } catch (_) {
        // ignore malformed events
      }
    };

    return () => es.close();
  }, [queryClient]);
}
