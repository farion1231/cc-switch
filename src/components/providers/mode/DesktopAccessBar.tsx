import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "@/lib/toast";
import type { Provider } from "@/types";
import { providersApi } from "@/lib/api/providers";
import { proxyApi } from "@/lib/api/proxy";
import { proxyKeys, useProxyStatusQuery } from "@/lib/query/proxy";
import { providerNeedsRouting } from "@/utils/providerCapabilities";
import { extractErrorMessage } from "@/utils/errorUtils";
import { Button } from "@/components/ui/button";
import { HelpTip } from "@/components/ui/help-tip";
import { Notice } from "@/components/ui/notice";
import { cn } from "@/lib/utils";

interface DesktopAccessBarProps {
  current?: Provider;
  onOpenRoutingSettings: () => void;
}

/**
 * Claude Desktop 的接入条（v7 S5）：模式按供应商设置，没有 tab。路由服务由模型映射自动管理，
 * 这里只显示状态；起不来时给「重试」和「更改端口…」。下面是 Desktop 配置的检查结果。
 */
export function DesktopAccessBar({
  current,
  onOpenRoutingSettings,
}: DesktopAccessBarProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const { data: proxyStatus } = useProxyStatusQuery();
  const { data: status } = useQuery({
    queryKey: ["claudeDesktopStatus"],
    queryFn: () => providersApi.getClaudeDesktopStatus(),
    refetchInterval: 5000,
  });
  const mapping = current
    ? providerNeedsRouting("claude-desktop", current)
    : false;
  const running = proxyStatus?.running ?? false;

  const retry = async () => {
    try {
      await proxyApi.startProxyServer();
      await queryClient.invalidateQueries({ queryKey: proxyKeys.status });
    } catch (error) {
      toast.error(
        t("proxy.server.startFailed", {
          detail: extractErrorMessage(error) || t("common.unknown"),
        }),
      );
    }
  };

  const messages = useMemo(() => {
    if (!status) return [];
    if (!status.supported) {
      return [t("claudeDesktop.statusUnsupported")];
    }
    const list: string[] = [];
    if (status.staleRawModels)
      list.push(t("claudeDesktop.statusStaleRawModels"));
    if (status.missingRouteMappings) {
      list.push(t("claudeDesktop.statusMissingRouteMappings"));
    }
    if (status.mode === "proxy" && !status.gatewayTokenConfigured) {
      list.push(t("claudeDesktop.statusGatewayTokenMissing"));
    }
    const expected = status.expectedBaseUrl?.replace(/\/+$/, "");
    const actual = status.actualBaseUrl?.replace(/\/+$/, "");
    if (expected && actual && expected !== actual) {
      list.push(t("claudeDesktop.statusBaseUrlMismatch", { expected, actual }));
    }
    return list;
  }, [status, t]);

  return (
    <div className="space-y-2">
      <div className="flex min-h-12 flex-wrap items-center gap-x-4 gap-y-1 rounded-panel bg-subtle px-5 py-2.5 text-body">
        <div className="flex min-w-0 flex-1 items-center gap-1 text-fg-2">
          <span className="truncate">
            {current
              ? t("desktopAccess.current", {
                  name: current.name,
                  mode: mapping
                    ? t("nav.mode.mappingFull")
                    : t("mode.names.direct"),
                })
              : t("desktopAccess.noCurrent")}
          </span>
          <HelpTip title={t("desktopAccess.helpTitle")}>
            {t("desktopAccess.help")}
          </HelpTip>
        </div>
        {mapping ? (
          running ? (
            <span className="flex shrink-0 items-center gap-2 text-fg-2">
              {t("desktopAccess.service")}
              <span
                className="h-1.5 w-1.5 rounded-full bg-route"
                aria-hidden="true"
              />
              <span className="font-medium text-fg-1">
                {t("desktopAccess.running")}
              </span>
            </span>
          ) : (
            <span className="flex shrink-0 items-center gap-2">
              <span className="text-warning-text">
                {t("desktopAccess.notRunning")}
              </span>
              <Button
                variant="neutral"
                size="compact"
                onClick={() => void retry()}
              >
                {t("common.retry")}
              </Button>
              <Button
                variant="quiet"
                size="compact"
                onClick={onOpenRoutingSettings}
              >
                {t("desktopAccess.changePort")}
              </Button>
            </span>
          )
        ) : (
          <span className={cn("shrink-0 text-fg-3")}>
            {t("desktopAccess.notNeeded")}
          </span>
        )}
      </div>
      {messages.length > 0 && (
        <Notice tone="warning" title={t("claudeDesktop.statusTitle")}>
          {messages.map((message) => (
            <span key={message} className="block">
              {message}
            </span>
          ))}
        </Notice>
      )}
    </div>
  );
}
