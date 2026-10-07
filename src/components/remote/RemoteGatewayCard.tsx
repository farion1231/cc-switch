import { useEffect, useState } from "react";
import { AlertTriangle, Loader2, Network, RefreshCw } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { RemoteGatewayState } from "@/lib/api/providers";
import { cn } from "@/lib/utils";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";

interface RemoteGatewayCardProps {
  appName: string;
  hostLabel: string;
  state?: RemoteGatewayState;
  /** Password logins can't keep a background tunnel open. */
  passwordTarget: boolean;
  enabling: boolean;
  disabling: boolean;
  reconnecting: boolean;
  onEnable: (remotePort?: number) => void;
  onDisable: () => void;
  onReconnect: () => void;
}

export function RemoteGatewayCard({
  appName,
  hostLabel,
  state,
  passwordTarget,
  enabling,
  disabling,
  reconnecting,
  onEnable,
  onDisable,
  onReconnect,
}: RemoteGatewayCardProps) {
  const { t } = useTranslation();
  const enabled = state?.enabled ?? false;
  const [portInput, setPortInput] = useState("");

  useEffect(() => {
    setPortInput(state?.remotePort ? String(state.remotePort) : "");
  }, [state?.remotePort]);

  const portNumber = Number(portInput.trim());
  const portValid =
    Number.isInteger(portNumber) && portNumber >= 1024 && portNumber <= 65535;
  const portChanged = portValid && portNumber !== state?.remotePort;
  const busy = enabling || disabling || reconnecting;

  const tunnel = state?.tunnel;
  const tunnelBadge = (() => {
    switch (tunnel?.state) {
      case "connected":
        return {
          label: t("remote.gateway.tunnelConnected", {
            defaultValue: "隧道已连接",
          }),
          className:
            "bg-emerald-100 text-emerald-700 dark:bg-emerald-900/40 dark:text-emerald-300",
        };
      case "connecting":
        return {
          label: t("remote.gateway.tunnelConnecting", {
            defaultValue: "隧道连接中",
          }),
          className:
            "bg-sky-100 text-sky-700 dark:bg-sky-900/40 dark:text-sky-300",
        };
      case "error":
        return {
          label: t("remote.gateway.tunnelError", {
            defaultValue: "隧道断开，自动重连中",
          }),
          className:
            "bg-amber-100 text-amber-800 dark:bg-amber-900/40 dark:text-amber-200",
        };
      default:
        return {
          label: t("remote.gateway.tunnelIdle", {
            defaultValue: "隧道未连接",
          }),
          className: "bg-muted text-muted-foreground",
        };
    }
  })();

  return (
    <section className="rounded-lg border border-border bg-card p-4">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <Network className="h-4 w-4 text-muted-foreground" />
            <h2 className="text-base font-semibold">
              {t("remote.gateway.title", { defaultValue: "使用本机网关" })}
            </h2>
            {enabled && (
              <Badge
                variant="secondary"
                className={cn("rounded-md", tunnelBadge.className)}
              >
                {tunnelBadge.label}
              </Badge>
            )}
          </div>
          <p className="mt-1 text-xs text-muted-foreground">
            {t("remote.gateway.description", {
              defaultValue:
                "{{host}} 上的 {{app}} 通过 SSH 反向隧道把请求转回本机 CC Switch 网关。切换供应商即时生效，不用改远端文件、不用重启；需要本机保持在线并运行 CC Switch。",
              host: hostLabel,
              app: appName,
            })}
          </p>
        </div>
        <div className="flex items-center gap-2">
          {(enabling || disabling) && (
            <Loader2 className="h-4 w-4 animate-spin text-muted-foreground" />
          )}
          <Switch
            checked={enabled}
            disabled={busy || (!enabled && passwordTarget)}
            onCheckedChange={(checked) => {
              if (checked) onEnable(portValid ? portNumber : undefined);
              else onDisable();
            }}
            aria-label={t("remote.gateway.title", {
              defaultValue: "使用本机网关",
            })}
          />
        </div>
      </div>

      {passwordTarget && !enabled && (
        <p className="mt-3 text-xs text-amber-700 dark:text-amber-300">
          {t("remote.gateway.passwordUnsupported", {
            defaultValue:
              "网关模式需要免密 SSH（~/.ssh/config 中的 Host 或密钥登录），密码登录无法在后台保持隧道。",
          })}
        </p>
      )}

      {enabled && (
        <div className="mt-4 space-y-3">
          {tunnel?.state === "error" && tunnel.message && (
            <div className="flex items-start gap-2 rounded-md border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-xs text-amber-900 dark:text-amber-200">
              <AlertTriangle className="mt-0.5 h-3.5 w-3.5 shrink-0" />
              <span className="break-all">{tunnel.message}</span>
            </div>
          )}
          {state && !state.proxyRunning && (
            <div className="flex flex-wrap items-center justify-between gap-2 rounded-md border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-xs text-amber-900 dark:text-amber-200">
              <span>
                {t("remote.gateway.proxyStopped", {
                  defaultValue:
                    "本机网关未运行，远端请求会失败。重连会自动启动网关。",
                })}
              </span>
            </div>
          )}

          <div className="flex flex-wrap items-end gap-3">
            <div className="space-y-1">
              <label
                htmlFor="remote-gateway-port"
                className="text-xs text-muted-foreground"
              >
                {t("remote.gateway.remotePort", {
                  defaultValue: "远端端口（服务器上的 127.0.0.1）",
                })}
              </label>
              <div className="flex items-center gap-2">
                <Input
                  id="remote-gateway-port"
                  className="h-8 w-28"
                  inputMode="numeric"
                  value={portInput}
                  onChange={(event) => setPortInput(event.target.value)}
                  disabled={busy}
                />
                <Button
                  size="sm"
                  variant="outline"
                  disabled={busy || !portChanged}
                  onClick={() => onEnable(portNumber)}
                >
                  {t("remote.gateway.applyPort", { defaultValue: "应用" })}
                </Button>
              </div>
            </div>
            <Button
              size="sm"
              variant="outline"
              disabled={busy}
              onClick={onReconnect}
            >
              {reconnecting ? (
                <Loader2 className="h-4 w-4 animate-spin" />
              ) : (
                <RefreshCw className="h-4 w-4" />
              )}
              {t("remote.gateway.reconnect", { defaultValue: "重连" })}
            </Button>
          </div>
          {portInput.trim() && !portValid && (
            <p className="text-xs text-destructive">
              {t("remote.gateway.portInvalid", {
                defaultValue: "端口需在 1024-65535 之间。",
              })}
            </p>
          )}
        </div>
      )}
    </section>
  );
}
