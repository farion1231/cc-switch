/**
 * 代理模式切换开关组件
 *
 * 放置在主界面头部，用于一键启用/关闭代理模式
 * 启用时自动接管 Live 配置，关闭时恢复原始配置
 */

import { Radio, Loader2 } from "lucide-react";
import { Switch } from "@/components/ui/switch";
import { useProxyStatus } from "@/hooks/useProxyStatus";
import { cn } from "@/lib/utils";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import { getAppLabel, type ProxyAppId } from "@/config/appConfig";

interface ProxyToggleProps {
  className?: string;
  activeApp: ProxyAppId;
}

export function ProxyToggle({ className, activeApp }: ProxyToggleProps) {
  const { t } = useTranslation();
  const {
    isRunning,
    takeoverStatus,
    setTakeoverForApp,
    isPending,
    isInitialStatusPending,
    status,
  } = useProxyStatus();

  const handleToggle = async (checked: boolean) => {
    try {
      await setTakeoverForApp({ appType: activeApp, enabled: checked });
      // Cursor 走本地路由时必须配合公网路由，提示设置入口。
      if (activeApp === "cursor" && checked) {
        toast.info(
          t("proxy.publicRoute.takeoverHint", {
            defaultValue:
              "Cursor 经本地路由接管时需要配置公网路由（隧道），请到 设置 → 路由 → 公网路由 查看设置方式",
          }),
          { closeButton: true },
        );
      }
    } catch (error) {
      console.error("[ProxyToggle] Toggle takeover failed:", error);
    }
  };

  const takeoverEnabled = takeoverStatus?.[activeApp] || false;

  const appLabel = getAppLabel(activeApp);

  const tooltipText = takeoverEnabled
    ? isRunning
      ? t("proxy.takeover.tooltip.active", {
          appLabel,
          address: status?.address,
          port: status?.port,
          defaultValue: `${appLabel} 已接管 - ${status?.address}:${status?.port}\n切换该应用供应商为热切换`,
        })
      : t("proxy.takeover.tooltip.broken", {
          appLabel,
          defaultValue: `${appLabel} 已接管，但代理服务未运行`,
        })
    : t("proxy.takeover.tooltip.inactive", {
        appLabel,
        defaultValue: `接管 ${appLabel} 的 Live 配置，让该应用请求走本地代理`,
      });

  return (
    <div
      className={cn(
        "flex items-center gap-1 px-1.5 h-8 rounded-lg bg-muted/50 transition-all",
        className,
      )}
      title={tooltipText}
    >
      {isPending ? (
        <Loader2 className="h-4 w-4 animate-spin text-muted-foreground" />
      ) : (
        <Radio
          className={cn(
            "h-4 w-4 transition-colors",
            takeoverEnabled
              ? "text-emerald-500 status-heartbeat"
              : "text-muted-foreground",
          )}
        />
      )}
      <Switch
        checked={takeoverEnabled}
        onCheckedChange={handleToggle}
        disabled={isPending || isInitialStatusPending}
      />
    </div>
  );
}
