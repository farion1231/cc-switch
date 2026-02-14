/**
 * 代理模式切换开关组件
 *
 * 放置在主界面头部，用于一键启用/关闭代理模式
 * 启用时自动接管 Live 配置，关闭时恢复原始配置
 */

import { Radio, Loader2 } from "lucide-react";
import { useState } from "react";
import { Switch } from "@/components/ui/switch";
import { useProxyStatus } from "@/hooks/useProxyStatus";
import { cn } from "@/lib/utils";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import type { AppId } from "@/lib/api";
import { proxyApi } from "@/lib/api/proxy";
import type { Provider } from "@/types";
import { extractProviderBaseUrl } from "@/utils/providerBaseUrl";

interface ProxyToggleProps {
  className?: string;
  activeApp: AppId;
  currentProvider?: Provider | null;
}

export function ProxyToggle({
  className,
  activeApp,
  currentProvider,
}: ProxyToggleProps) {
  const { t } = useTranslation();
  const [isCheckingRequirement, setIsCheckingRequirement] = useState(false);
  const { isRunning, takeoverStatus, setTakeoverForApp, isPending, status } =
    useProxyStatus();

  const handleToggle = async (checked: boolean) => {
    // 关闭代理时，检查当前供应商是否是全链接配置
    if (
      !checked &&
      currentProvider &&
      currentProvider.category !== "official"
    ) {
      const baseUrl = extractProviderBaseUrl(currentProvider, activeApp);
      const apiFormat = currentProvider.meta?.apiFormat;
      setIsCheckingRequirement(true);
      try {
        let proxyRequirement: string | null = null;

        // 先按 API 格式做硬性判断（baseUrl 缺失时仍需提示）
        if (activeApp === "claude" && apiFormat === "openai_chat") {
          proxyRequirement = "openai_chat_format";
        }

        if (!proxyRequirement && baseUrl) {
          proxyRequirement = await proxyApi.checkProxyRequirement(
            activeApp,
            baseUrl,
            apiFormat,
          );
        }

        if (proxyRequirement) {
          const warningKey =
            proxyRequirement === "openai_chat_format"
              ? "notifications.openAIChatFormatWarningOnDisable"
              : proxyRequirement === "url_mismatch"
                ? "notifications.urlMismatchWarningOnDisable"
                : "notifications.fullUrlWarningOnDisable";

          toast.warning(
            t(warningKey, {
              defaultValue:
                "当前供应商配置可能依赖代理模式，关闭代理后可能无法正常工作。建议更换为基础地址配置或保持代理开启。",
            }),
            { duration: 6000, closeButton: true },
          );
        }
      } catch (error) {
        console.error("Failed to check proxy requirement:", error);
      } finally {
        setIsCheckingRequirement(false);
      }
    }

    try {
      await setTakeoverForApp({ appType: activeApp, enabled: checked });
    } catch (error) {
      console.error("[ProxyToggle] Toggle takeover failed:", error);
      toast.error(
        t("proxy.takeover.toggleFailed", {
          defaultValue: "切换接管状态失败",
        }),
        {
          description: t("proxy.takeover.toggleFailedDesc", {
            defaultValue: "请检查代理服务状态与权限，然后重试。",
          }),
          closeButton: true,
        },
      );
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
      {isPending || isCheckingRequirement ? (
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
        disabled={isPending || isCheckingRequirement}
      />
    </div>
  );
}
