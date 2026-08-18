import { Loader2, Radio } from "lucide-react";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import { Switch } from "@/components/ui/switch";
import { useProxyStatus } from "@/hooks/useProxyStatus";
import { cn } from "@/lib/utils";

interface ClaudeDesktopRouteToggleProps {
  className?: string;
  target?: "claude" | "codex";
}

export function ClaudeDesktopRouteToggle({
  className,
  target = "claude",
}: ClaudeDesktopRouteToggleProps) {
  const { t } = useTranslation();
  const {
    isRunning,
    status,
    takeoverStatus,
    startProxyServer,
    stopProxyServer,
    isStarting,
    isStoppingServer,
  } = useProxyStatus();

  const isBusy = isStarting || isStoppingServer;
  const otherTakeoverActive = Boolean(
    takeoverStatus?.claude ||
      takeoverStatus?.codex ||
      takeoverStatus?.gemini ||
      takeoverStatus?.grokbuild,
  );
  const routeAddress = status?.address ?? "127.0.0.1";
  const routePort = status?.port ?? 15721;

  const handleToggle = async (checked: boolean) => {
    try {
      if (checked) {
        await startProxyServer();
        return;
      }

      if (otherTakeoverActive) {
        toast.warning(
          t("claudeDesktop.route.stopBlockedByTakeover", {
            defaultValue:
              "其它应用正在使用代理接管。请先在设置中关闭对应应用接管，再停止本地路由。",
          }),
          { duration: 5000 },
        );
        return;
      }

      await stopProxyServer();
    } catch (error) {
      console.error("[DesktopRouteToggle] Toggle route failed:", error);
    }
  };

  const routeKey =
    target === "codex" ? "codexDesktop.route" : "claudeDesktop.route";
  const desktopName = target === "codex" ? "Codex Desktop" : "Claude Desktop";
  const tooltipText = isRunning
    ? t(`${routeKey}.tooltip.active`, {
        address: routeAddress,
        port: routePort,
        defaultValue: `${desktopName} 本地路由已开启 - ${routeAddress}:${routePort}`,
      })
    : t(`${routeKey}.tooltip.inactive`, {
        address: routeAddress,
        port: routePort,
        defaultValue: `开启 ${desktopName} 本地路由，用于需要格式转换或动态认证的供应商。当前配置地址：${routeAddress}:${routePort}`,
      });

  return (
    <div
      className={cn(
        "flex items-center gap-1 px-1.5 h-8 rounded-lg bg-muted/50 transition-all",
        className,
      )}
      title={tooltipText}
    >
      {isBusy ? (
        <Loader2 className="h-4 w-4 animate-spin text-muted-foreground" />
      ) : (
        <Radio
          className={cn(
            "h-4 w-4 transition-colors",
            isRunning
              ? "text-emerald-500 status-heartbeat"
              : "text-muted-foreground",
          )}
        />
      )}
      <Switch
        checked={isRunning}
        onCheckedChange={handleToggle}
        disabled={isBusy}
      />
    </div>
  );
}
