import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ChevronRight, Loader2, Network, RefreshCw } from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "@/lib/toast";
import type { AppId } from "@/lib/api";
import {
  providersApi,
  type RemoteGatewayHost,
  type RemoteTunnelState,
} from "@/lib/api/providers";
import { extractErrorMessage } from "@/utils/errorUtils";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";

export const REMOTE_GATEWAY_OVERVIEW_KEY = ["remoteGatewayOverview"] as const;

const TUNNEL_DOT: Record<RemoteTunnelState, string> = {
  connected: "bg-emerald-500",
  connecting: "bg-sky-500 animate-pulse",
  error: "bg-amber-500",
  idle: "bg-muted-foreground/50",
};

interface RemoteGatewayIndicatorProps {
  onManage: (host: RemoteGatewayHost, app: AppId) => void;
}

/** Header pill listing SSH hosts that use the local gateway; hidden when none. */
export function RemoteGatewayIndicator({
  onManage,
}: RemoteGatewayIndicatorProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [open, setOpen] = useState(false);

  const overviewQuery = useQuery({
    queryKey: REMOTE_GATEWAY_OVERVIEW_KEY,
    queryFn: () => providersApi.getRemoteGatewayOverview(),
    refetchInterval: open ? 2000 : 5000,
  });

  const reconnectMutation = useMutation({
    mutationFn: (host: RemoteGatewayHost) =>
      providersApi.reconnectRemoteGateway(host.routes[0].app, host.target),
    onSettled: () =>
      queryClient.invalidateQueries({ queryKey: REMOTE_GATEWAY_OVERVIEW_KEY }),
    onError: (error: unknown) =>
      toast.error(
        t("remote.gateway.failed", {
          defaultValue: "本机网关操作失败: {{error}}",
          error: extractErrorMessage(error),
        }),
      ),
  });

  const overview = overviewQuery.data;
  const hosts = overview?.hosts ?? [];
  if (hosts.length === 0) return null;

  const allConnected = hosts.every((host) => host.tunnel.state === "connected");
  const summaryDot = !overview?.proxyRunning
    ? "bg-red-500"
    : allConnected
      ? "bg-emerald-500"
      : "bg-amber-500";

  const tunnelLabel = (state: RemoteTunnelState) =>
    ({
      connected: t("remote.gateway.tunnelConnected", {
        defaultValue: "隧道已连接",
      }),
      connecting: t("remote.gateway.tunnelConnecting", {
        defaultValue: "隧道连接中",
      }),
      error: t("remote.gateway.tunnelError", {
        defaultValue: "隧道断开，自动重连中",
      }),
      idle: t("remote.gateway.tunnelIdle", { defaultValue: "隧道未连接" }),
    })[state];

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button
          variant="ghost"
          size="sm"
          className="relative h-8 gap-1.5 px-2 hover:bg-black/5 dark:hover:bg-white/5"
          title={t("remote.gateway.indicatorTitle", {
            defaultValue: "{{count}} 台服务器正在使用本机网关",
            count: hosts.length,
          })}
        >
          <Network className="h-4 w-4" />
          <span className="text-xs tabular-nums">{hosts.length}</span>
          <span
            className={cn(
              "absolute right-1 top-1 h-1.5 w-1.5 rounded-full",
              summaryDot,
            )}
            aria-hidden="true"
          />
        </Button>
      </PopoverTrigger>
      <PopoverContent className="w-80 p-0" align="start">
        <div className="flex items-center justify-between border-b border-border px-3 py-2">
          <span className="text-sm font-semibold">
            {t("remote.gateway.indicatorHeading", {
              defaultValue: "远端网关",
            })}
          </span>
          <span
            className={cn(
              "text-xs",
              overview?.proxyRunning
                ? "text-emerald-600 dark:text-emerald-400"
                : "text-red-600 dark:text-red-400",
            )}
          >
            {overview?.proxyRunning
              ? t("remote.gateway.proxyRunning", {
                  defaultValue: "本机网关运行中",
                })
              : t("remote.gateway.proxyNotRunning", {
                  defaultValue: "本机网关未运行",
                })}
          </span>
        </div>

        <div className="max-h-[22rem] divide-y divide-border overflow-y-auto">
          {hosts.map((host) => {
            const reconnecting =
              reconnectMutation.isPending &&
              reconnectMutation.variables?.hostKey === host.hostKey;
            return (
              <div key={host.hostKey} className="space-y-1.5 px-3 py-2.5">
                <div className="flex items-center gap-2">
                  <span
                    className={cn(
                      "h-2 w-2 shrink-0 rounded-full",
                      TUNNEL_DOT[host.tunnel.state],
                    )}
                    title={tunnelLabel(host.tunnel.state)}
                  />
                  <span className="min-w-0 flex-1 truncate text-sm font-medium">
                    {host.hostKey}
                  </span>
                  <Button
                    variant="ghost"
                    size="icon"
                    className="h-6 w-6"
                    disabled={reconnectMutation.isPending}
                    onClick={() => reconnectMutation.mutate(host)}
                    title={t("remote.gateway.reconnect", {
                      defaultValue: "重连",
                    })}
                  >
                    {reconnecting ? (
                      <Loader2 className="h-3.5 w-3.5 animate-spin" />
                    ) : (
                      <RefreshCw className="h-3.5 w-3.5" />
                    )}
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon"
                    className="h-6 w-6"
                    onClick={() => {
                      setOpen(false);
                      onManage(host, host.routes[0].app);
                    }}
                    title={t("remote.gateway.manage", {
                      defaultValue: "打开远端管理",
                    })}
                  >
                    <ChevronRight className="h-3.5 w-3.5" />
                  </Button>
                </div>
                <div className="pl-4 text-xs text-muted-foreground">
                  {tunnelLabel(host.tunnel.state)} · 127.0.0.1:
                  {host.remotePort}
                </div>
                {host.tunnel.state === "error" && host.tunnel.message && (
                  <p className="line-clamp-2 break-all pl-4 text-xs text-amber-700 dark:text-amber-300">
                    {host.tunnel.message}
                  </p>
                )}
                <div className="space-y-0.5 pl-4">
                  {host.routes.map((route) => (
                    <button
                      key={route.app}
                      type="button"
                      className="flex w-full items-center gap-2 rounded px-1 py-0.5 text-left text-xs hover:bg-muted"
                      onClick={() => {
                        setOpen(false);
                        onManage(host, route.app);
                      }}
                    >
                      <span className="w-14 shrink-0 text-muted-foreground">
                        {t(`apps.${route.app}`)}
                      </span>
                      <span className="min-w-0 flex-1 truncate">
                        {route.providerId
                          ? (route.providerName ?? route.providerId)
                          : t("remote.gateway.followLocalShort", {
                              defaultValue: "跟随本地 · {{name}}",
                              name: route.providerName ?? "-",
                            })}
                      </span>
                    </button>
                  ))}
                </div>
              </div>
            );
          })}
        </div>

        <p className="border-t border-border px-3 py-2 text-[11px] leading-relaxed text-muted-foreground">
          {t("remote.gateway.indicatorHint", {
            defaultValue:
              "这些服务器通过 SSH 隧道使用本机网关，需要本机保持在线。",
          })}
        </p>
      </PopoverContent>
    </Popover>
  );
}
