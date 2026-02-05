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
import { toast } from "sonner";
import type { AppId } from "@/lib/api";
import { proxyApi } from "@/lib/api/proxy";
import type { Provider } from "@/types";

interface ProxyToggleProps {
  className?: string;
  activeApp: AppId;
  currentProvider?: Provider | null;
}

/**
 * 从 provider 配置中提取 base URL
 */
function extractBaseUrl(provider: Provider, appId: AppId): string | null {
  try {
    const config = provider.settingsConfig;
    if (!config) return null;

    if (appId === "claude") {
      const envUrl = config?.env?.ANTHROPIC_BASE_URL;
      return typeof envUrl === "string" ? envUrl.trim() : null;
    }

    if (appId === "codex") {
      const tomlConfig = config?.config;
      if (typeof tomlConfig === "string") {
        const match = tomlConfig.match(/base_url\s*=\s*(['"])([^'"]+)\1/);
        return match?.[2]?.trim() || null;
      }
      const baseUrl = config?.base_url;
      return typeof baseUrl === "string" ? baseUrl.trim() : null;
    }

    if (appId === "gemini") {
      const envUrl = config?.env?.GOOGLE_GEMINI_BASE_URL;
      if (typeof envUrl === "string") return envUrl.trim();
      const baseUrl = config?.GEMINI_API_BASE || config?.base_url;
      return typeof baseUrl === "string" ? baseUrl.trim() : null;
    }

    return null;
  } catch {
    return null;
  }
}

export function ProxyToggle({
  className,
  activeApp,
  currentProvider,
}: ProxyToggleProps) {
  const { t } = useTranslation();
  const { isRunning, takeoverStatus, setTakeoverForApp, isPending, status } =
    useProxyStatus();

  const handleToggle = async (checked: boolean) => {
    // 关闭代理时，检查当前供应商是否是全链接配置
    if (
      !checked &&
      currentProvider &&
      currentProvider.category !== "official"
    ) {
      const baseUrl = extractBaseUrl(currentProvider, activeApp);
      const apiFormat = currentProvider.meta?.apiFormat;

      if (baseUrl) {
        try {
          const proxyRequirement = await proxyApi.checkProxyRequirement(
            activeApp,
            baseUrl,
            apiFormat,
          );

          if (proxyRequirement) {
            // 显示警告但仍允许关闭
            toast.warning(
              t("notifications.fullUrlWarningOnDisable", {
                defaultValue:
                  "当前供应商配置了完整 API 路径或特殊格式，关闭代理后可能无法正常工作。建议更换为基础地址配置或保持代理开启。",
              }),
              {
                duration: 6000,
                closeButton: true,
              },
            );
          }
        } catch (error) {
          console.error("Failed to check proxy requirement:", error);
        }
      }
    }

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
              ? "text-emerald-500 animate-pulse"
              : "text-muted-foreground",
          )}
        />
      )}
      <Switch
        checked={takeoverEnabled}
        onCheckedChange={handleToggle}
        disabled={isPending}
      />
    </div>
  );
}
