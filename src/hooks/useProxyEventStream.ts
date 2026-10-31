/**
 * 订阅 cc-switch remote server 的 SSE 事件流，让前端 React Query 在控制面
 * （远程 REST API / 脚本）改动代理状态时自动 invalidate，UI 立即同步。
 *
 * 设计动机：
 * - cc-switch v3.20.1 (#5928) 重构后去掉了 useProxyTakeoverStatus 的轮询以节省 idle GPU
 * - 但远程 API（/api/proxy/control）改状态后，桌面 app 没有通知链路
 * - 后端 sse_tx 在接管状态变更时 broadcast EventSource，前端订阅后 invalidate React Query
 * - 远程 REST `POST /api/reorder` 改 sort_index 后同样没有任何前端通知，
 *   `providers`（卡片顺序）与 `failoverQueue`（P1/P2 徽章）两份缓存会一直停在旧顺序
 *
 * 用法：在 App.tsx 顶层挂一次即可
 */

import { useEffect } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useSettingsQuery } from "@/lib/query";
import { proxyKeys } from "@/lib/query/proxy";

const DEFAULT_REMOTE_PORT = 4000;

export function useProxyEventStream() {
  const queryClient = useQueryClient();
  // 端口必须跟用户配置走：写死 4000 时，把远程端口改成别的值的实例永远连不上
  // 自己的 SSE，表现为「REST 改了但 UI 不动」，和这条广播链本身坏掉长得一样。
  const { data: settings } = useSettingsQuery();
  const remotePort = settings?.remotePort ?? DEFAULT_REMOTE_PORT;
  const remoteEnabled = settings?.remoteEnabled ?? false;

  useEffect(() => {
    // 远程管理关着的时候**不建**这条连接。EventSource 断线后会自己重连，
    // 哪怕端口上根本没有服务在听，它也会按浏览器默认的退避节奏一直重试 ——
    // 于是一个从不用远程管理的用户，桌面端常年挂着一条注定失败的重连循环。
    if (!remoteEnabled) return;

    const es = new EventSource(`http://127.0.0.1:${remotePort}/api/events`);

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
            // 远程切接管模式会改「当前供应商」：只失效 takeoverStatus 时，
            // 桌面 UI 的供应商列表停在旧值，远程明明已经切了模式、本地却显示没切。
            queryClient.invalidateQueries({
              queryKey: proxyKeys.takeoverStatus,
            });
            queryClient.invalidateQueries({ queryKey: ["providers"] });
            break;
          case "live_configs_changed":
          case "shutdown":
            queryClient.invalidateQueries({
              queryKey: proxyKeys.takeoverStatus,
            });
            break;
          case "switch":
            // 切换同时改「当前供应商」和队列里的相对位置（P1/P2 徽章）：
            // 只失效 takeoverStatus 时列表与队列会一直停在旧值。
            queryClient.invalidateQueries({ queryKey: ["providers"] });
            queryClient.invalidateQueries({ queryKey: ["failoverQueue"] });
            queryClient.invalidateQueries({
              queryKey: ["autoFailoverEnabled"],
            });
            queryClient.invalidateQueries({
              queryKey: proxyKeys.takeoverStatus,
            });
            break;
          case "failover_changed":
            // 这个开关决定 UI 上「路由目标按队列还是按手动」。不失效的话，
            // 开关状态与目标列表会一直显示旧值（切走再回来才刷新）。
            queryClient.invalidateQueries({
              queryKey: ["autoFailoverEnabled"],
            });
            queryClient.invalidateQueries({ queryKey: ["failoverQueue"] });
            queryClient.invalidateQueries({ queryKey: ["providers"] });
            queryClient.invalidateQueries({ queryKey: proxyKeys.status });
            queryClient.invalidateQueries({
              queryKey: proxyKeys.takeoverStatus,
            });
            break;
          case "reorder":
            // 远程 REST /api/reorder 改了 sort_index；providers 决定卡片顺序，
            // failoverQueue 决定 P1/P2 徽章，两个都必须失效。
            queryClient.invalidateQueries({ queryKey: ["providers"] });
            queryClient.invalidateQueries({ queryKey: ["failoverQueue"] });
            break;
        }
      } catch (_) {
        // ignore malformed events
      }
    };

    return () => es.close();
  }, [queryClient, remotePort, remoteEnabled]);
}
