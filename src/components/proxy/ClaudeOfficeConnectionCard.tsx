import { useQuery } from "@tanstack/react-query";
import { Copy, ExternalLink, FileSpreadsheet } from "lucide-react";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { providersApi } from "@/lib/api/providers";
import { useProxyStatus } from "@/hooks/useProxyStatus";
import { cn } from "@/lib/utils";

interface ClaudeOfficeConnectionCardProps {
  className?: string;
}

/**
 * Claude for Office 连接信息卡片
 *
 * 展示 /claude-office gateway 的 URL 和 token，供用户粘贴到
 * Office 加载项的"Connect another way → Gateway"连接界面。
 * 该前缀的代理路由带 CORS + Private Network Access 支持，且接受 x-api-key 鉴权。
 */
export function ClaudeOfficeConnectionCard({
  className,
}: ClaudeOfficeConnectionCardProps) {
  const { t } = useTranslation();
  const { isRunning } = useProxyStatus();

  const { data: info, error } = useQuery({
    queryKey: ["claudeOfficeGatewayInfo"],
    queryFn: () => providersApi.getClaudeOfficeGatewayInfo(),
    refetchInterval: 5000,
    retry: false,
  });

  const copy = async (value: string, labelKey: string) => {
    try {
      await navigator.clipboard.writeText(value);
      toast.success(
        t("claudeOffice.copied", {
          label: labelKey,
          defaultValue: `${labelKey} 已复制`,
        }),
        { closeButton: true },
      );
    } catch {
      toast.error(t("claudeOffice.copyFailed", { defaultValue: "复制失败" }));
    }
  };

  return (
    <div
      className={cn(
        "rounded-xl border border-border bg-card/50 p-4 space-y-3",
        className,
      )}
    >
      <div className="flex items-center gap-3">
        <div className="flex h-8 w-8 items-center justify-center rounded-lg bg-background ring-1 ring-border">
          <FileSpreadsheet className="h-4 w-4 text-emerald-500" />
        </div>
        <div className="space-y-1">
          <p className="text-sm font-medium leading-none">
            {t("claudeOffice.title", { defaultValue: "Claude for Office" })}
          </p>
          <p className="text-xs text-muted-foreground">
            {t("claudeOffice.description", {
              defaultValue:
                "在 Office 加载项的「Connect another way → Gateway」中填入以下信息",
            })}
          </p>
        </div>
      </div>

      {!isRunning && (
        <p className="text-xs text-yellow-600 dark:text-yellow-400">
          {t("claudeOffice.proxyNotRunning", {
            defaultValue: "本地代理服务未运行，请先启动代理服务",
          })}
        </p>
      )}

      {error ? (
        <p className="text-xs text-muted-foreground">
          {t("claudeOffice.loadFailed", {
            defaultValue: "连接信息暂不可用，请先启动代理服务",
          })}
        </p>
      ) : info ? (
        <div className="space-y-2">
          <div>
            <p className="text-xs text-muted-foreground mb-1">
              {t("claudeOffice.gatewayUrl", { defaultValue: "Gateway URL" })}
            </p>
            <div className="flex items-center gap-2">
              <code className="flex-1 text-xs bg-background px-3 py-2 rounded border border-border/60 truncate">
                {info.gatewayUrl}
              </code>
              <Button
                size="sm"
                variant="outline"
                onClick={() => copy(info.gatewayUrl, "Gateway URL")}
              >
                <Copy className="h-3.5 w-3.5" />
              </Button>
            </div>
          </div>
          <div>
            <p className="text-xs text-muted-foreground mb-1">
              {t("claudeOffice.token", { defaultValue: "Token" })}
            </p>
            <div className="flex items-center gap-2">
              <code className="flex-1 text-xs bg-background px-3 py-2 rounded border border-border/60 truncate">
                {info.token}
              </code>
              <Button
                size="sm"
                variant="outline"
                onClick={() => copy(info.token, "Token")}
              >
                <Copy className="h-3.5 w-3.5" />
              </Button>
            </div>
          </div>
          <p className="text-xs text-muted-foreground flex items-center gap-1">
            <ExternalLink className="h-3 w-3" />
            {t("claudeOffice.hint", {
              defaultValue:
                "与 Claude Desktop 共用供应商配置与故障转移队列，切换供应商对两者同时生效",
            })}
          </p>
        </div>
      ) : null}
    </div>
  );
}
