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
import { useTranslation } from "react-i18next";
import type { AppId } from "@/lib/api";

interface ProxyToggleProps {
  className?: string;
  activeApp: AppId;
}

export function ProxyToggle({ className, activeApp }: ProxyToggleProps) {
  const { t } = useTranslation();
  const { isRunning, takeoverStatus, setTakeoverForApp, isPending, status } =
    useProxyStatus();

  const handleToggle = async (checked: boolean) => {
    try {
      await setTakeoverForApp({ appType: activeApp, enabled: checked });
    } catch (error) {
      console.error("[ProxyToggle] Toggle takeover failed:", error);
    }
  };

  const takeoverEnabled = takeoverStatus?.[activeApp] || false;

  const appLabel =
    activeApp === "claude"
      ? "Claude"
      : activeApp === "codex"
        ? "Codex"
        : activeApp === "gemini"
          ? "Gemini"
          : "OpenCode";

  const tooltipText = takeoverEnabled
    ? isRunning
      ? t("proxy.takeover.tooltip.active", {
          defaultValue: `${appLabel} 已接管 - ${status?.address}:${status?.port}\n切换该应用供应商为热切换`,
        })
      : t("proxy.takeover.tooltip.broken", {
          defaultValue: `${appLabel} 已接管，但代理服务未运行`,
        })
    : t("proxy.takeover.tooltip.inactive", {
        defaultValue: `接管 ${appLabel} 的 Live 配置，让该应用请求走本地代理`,
      });

  return (
    <div
      className={cn("transition-all", className)}
      title={tooltipText}
    >
      <div className="flex items-center gap-1.5 px-2 h-7 rounded-full bg-gray-100/80 dark:bg-gray-800/50 cursor-default">
        {isPending ? (
          <Loader2 className="h-3.5 w-3.5 animate-spin text-gray-400" />
        ) : (
          <Radio
            className={cn(
              "h-3.5 w-3.5 transition-colors",
              takeoverEnabled
                ? "text-emerald-500 animate-pulse"
                : "text-gray-400",
            )}
          />
        )}
        <span
          className={cn(
            "text-xs font-medium transition-colors select-none",
            takeoverEnabled
              ? "text-emerald-600 dark:text-emerald-400"
              : "text-gray-500 dark:text-gray-400",
          )}
        >
          Proxy
        </span>
        <Switch
          checked={takeoverEnabled}
          onCheckedChange={handleToggle}
          disabled={isPending}
          className="ml-0.5 scale-90"
        />
      </div>
    </div>
  );
}
