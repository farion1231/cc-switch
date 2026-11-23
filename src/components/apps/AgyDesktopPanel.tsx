import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import {
  BarChart3,
  ChevronRight,
  FileText,
  FolderOpen,
  KeyRound,
  MessageSquare,
  RefreshCw,
  Server,
  Wrench,
} from "lucide-react";
import { settingsApi } from "@/lib/api";
import type { GlobalPage } from "@/lib/navigation";
import { toast } from "@/lib/toast";
import { extractErrorMessage } from "@/utils/errorUtils";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

interface AgyDesktopPanelProps {
  onOpenPage: (page: GlobalPage) => void;
  onOpenUsage: () => void;
}

/**
 * Antigravity Desktop 独立侧栏页：桌面应用自身管理登录与模型切换，这里展示本机安装
 * 状态、共享配置目录，并提供 MCP / Skills / 提示词 / 会话 / 用量 / 授权中心快捷入口。
 */
export function AgyDesktopPanel({
  onOpenPage,
  onOpenUsage,
}: AgyDesktopPanelProps) {
  const { t } = useTranslation();
  const {
    data: desktopStatus,
    isFetching: isRefreshingStatus,
    refetch: refetchDesktopStatus,
  } = useQuery({
    queryKey: ["agy-desktop-status"],
    queryFn: () => settingsApi.getAgyDesktopStatus(),
    staleTime: 30_000,
  });
  const { data: configStatus, refetch: refetchConfigStatus } = useQuery({
    queryKey: ["config-status", "agy-desktop"],
    queryFn: () => settingsApi.getConfigStatus("agy-desktop"),
    staleTime: 30_000,
  });

  const handleRefresh = () => {
    void refetchDesktopStatus();
    void refetchConfigStatus();
  };

  const handleOpenConfigFolder = async () => {
    try {
      await settingsApi.openConfigFolder("agy-desktop");
    } catch (error) {
      toast.error(extractErrorMessage(error) || t("common.error"));
    }
  };

  const statusText =
    desktopStatus?.supported === false
      ? t("appsPage.desktopUnsupported")
      : desktopStatus?.installed
        ? desktopStatus.version
          ? `v${desktopStatus.version}`
          : t("appsPage.desktopConnected")
        : t("common.notInstalled");

  const shortcuts: Array<{
    key: string;
    icon: typeof Server;
    label: string;
    hint: string;
    onClick: () => void;
  }> = [
    {
      key: "mcp",
      icon: Server,
      label: "MCP",
      hint: "~/.gemini/config/mcp_config.json",
      onClick: () => onOpenPage("mcp"),
    },
    {
      key: "skills",
      icon: Wrench,
      label: t("nav.skills"),
      hint: "~/.gemini/config/skills",
      onClick: () => onOpenPage("skills"),
    },
    {
      key: "prompts",
      icon: FileText,
      label: t("nav.prompts"),
      hint: "~/.gemini/config/AGENTS.md",
      onClick: () => onOpenPage("prompts"),
    },
    {
      key: "sessions",
      icon: MessageSquare,
      label: t("nav.sessions"),
      hint: "~/.gemini/antigravity/brain",
      onClick: () => onOpenPage("sessions"),
    },
    {
      key: "usage",
      icon: BarChart3,
      label: t("nav.usage"),
      hint: t("agyDesktop.usageHint"),
      onClick: onOpenUsage,
    },
    {
      key: "auth",
      icon: KeyRound,
      label: t("nav.auth"),
      hint: t("agyDesktop.authHint"),
      onClick: () => onOpenPage("auth"),
    },
  ];

  return (
    <div
      id="main-content"
      className="min-h-0 flex-1 overflow-y-auto scroll-stable px-6 pb-12 pt-4"
    >
      <div className="max-w-3xl space-y-6">
        <section className="rounded-panel border border-border bg-surface p-5">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div className="flex items-center gap-2.5">
              <h2 className="m-0 text-strong text-fg-1">
                {t("agyDesktop.statusTitle")}
              </h2>
              <span
                className={cn(
                  "inline-flex items-center rounded-full px-2 py-0.5 text-caption font-medium",
                  desktopStatus?.installed
                    ? "bg-emerald-500/10 font-mono tabular-nums text-emerald-700 dark:text-emerald-300"
                    : "bg-subtle text-fg-2",
                )}
              >
                {statusText}
              </span>
            </div>
            <div className="flex items-center gap-2">
              <Button
                variant="neutral"
                size="compact"
                disabled={isRefreshingStatus}
                onClick={handleRefresh}
              >
                <RefreshCw
                  className={cn(
                    "h-3.5 w-3.5",
                    isRefreshingStatus && "animate-spin",
                  )}
                />
                {t("agyDesktop.refreshStatus")}
              </Button>
              <Button
                variant="neutral"
                size="compact"
                onClick={() => void handleOpenConfigFolder()}
              >
                <FolderOpen className="h-3.5 w-3.5" />
                {t("agyDesktop.openConfigFolder")}
              </Button>
            </div>
          </div>

          <p className="mt-3 text-body text-fg-2">
            {t("agyDesktop.overviewDescription")}
          </p>

          <dl className="mt-4 space-y-2 border-t border-border pt-4 text-caption">
            {desktopStatus?.installPath && (
              <div className="flex items-baseline gap-3">
                <dt className="w-24 shrink-0 text-fg-3">
                  {t("agyDesktop.installPath")}
                </dt>
                <dd
                  className="min-w-0 flex-1 truncate font-mono text-fg-1"
                  title={desktopStatus.installPath}
                >
                  {desktopStatus.installPath}
                </dd>
              </div>
            )}
            {configStatus?.path && (
              <div className="flex items-baseline gap-3">
                <dt className="w-24 shrink-0 text-fg-3">
                  {t("agyDesktop.configDir")}
                </dt>
                <dd
                  className="min-w-0 flex-1 truncate font-mono text-fg-1"
                  title={configStatus.path}
                >
                  {configStatus.path}
                </dd>
              </div>
            )}
          </dl>
        </section>

        <section className="space-y-2">
          <h2 className="m-0 px-1 text-caption font-medium text-fg-2">
            {t("agyDesktop.sharedFeaturesTitle")}
          </h2>
          <div className="divide-y divide-border overflow-hidden rounded-panel border border-border bg-surface">
            {shortcuts.map(({ key, icon: Icon, label, hint, onClick }) => (
              <button
                key={key}
                type="button"
                onClick={onClick}
                className="flex w-full items-center gap-3 px-4 py-3 text-start transition-colors hover:bg-subtle focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring"
              >
                <Icon
                  className="h-4 w-4 shrink-0 text-fg-2"
                  strokeWidth={1.5}
                />
                <div className="min-w-0 flex-1">
                  <div className="text-body font-medium text-fg-1">{label}</div>
                  <div className="truncate font-mono text-caption text-fg-3">
                    {hint}
                  </div>
                </div>
                <ChevronRight
                  className="h-4 w-4 shrink-0 text-fg-3"
                  strokeWidth={1.5}
                />
              </button>
            ))}
          </div>
        </section>
      </div>
    </div>
  );
}
