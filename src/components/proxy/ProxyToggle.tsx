/**
 * 代理模式切换开关组件
 *
 * 放置在主界面头部，用于一键启用/关闭代理模式
 * 启用时自动接管 Live 配置，关闭时恢复原始配置
 *
 * `pool` 为真时是附加模式开关（设置里和路由开关二选一，只用于 Claude Code、Codex）：
 * 打开进入附加模式，关掉回到直连。
 */

import { Layers, Radio, Loader2 } from "lucide-react";
import { Switch } from "@/components/ui/switch";
import { useProxyStatus } from "@/hooks/useProxyStatus";
import { useProxyPool } from "@/lib/query/proxy";
import { cn } from "@/lib/utils";
import { useTranslation } from "react-i18next";
import { getAppLabel, type ProxyAppId } from "@/config/appConfig";

interface ProxyToggleProps {
  className?: string;
  activeApp: ProxyAppId;
  pool?: boolean;
}

export function ProxyToggle({
  className,
  activeApp,
  pool = false,
}: ProxyToggleProps) {
  const { t } = useTranslation();
  const {
    isRunning,
    takeoverStatus,
    setTakeoverForApp,
    isPending,
    isInitialStatusPending,
    status,
  } = useProxyStatus();
  const { data: poolView } = useProxyPool(activeApp, pool);

  const handleToggle = async (checked: boolean) => {
    try {
      await setTakeoverForApp({ appType: activeApp, enabled: checked, pool });
    } catch (error) {
      console.error("[ProxyToggle] Toggle takeover failed:", error);
    }
  };

  const takeoverEnabled = takeoverStatus?.[activeApp] || false;
  // 附加模式开关只在附加模式下亮：路由模式（比如刚在设置里换过来）算关着，打开就换成附加模式。
  const checked = takeoverEnabled && (!pool || poolView?.active === true);

  const appLabel = getAppLabel(activeApp);

  const tooltipText = pool
    ? checked
      ? t("proxy.poolMode.tooltip.active", { appLabel })
      : t("proxy.poolMode.tooltip.inactive", { appLabel })
    : takeoverEnabled
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

  const Icon = pool ? Layers : Radio;

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
        <Icon
          className={cn(
            "h-4 w-4 transition-colors",
            checked
              ? pool
                ? "text-violet-500"
                : "text-emerald-500 status-heartbeat"
              : "text-muted-foreground",
          )}
        />
      )}
      <Switch
        checked={checked}
        onCheckedChange={handleToggle}
        disabled={isPending || isInitialStatusPending}
        aria-label={
          pool
            ? t("proxy.poolMode.ariaLabel", { appLabel })
            : t("proxy.takeover.ariaLabel", { appLabel })
        }
      />
    </div>
  );
}
