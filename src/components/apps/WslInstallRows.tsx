import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useQueryClient } from "@tanstack/react-query";
import { AlertTriangle, ArrowUpCircle, Download, Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { HoverTip } from "@/components/ui/hover-tip";
import { toast } from "@/lib/toast";
import { isUpdateAvailable } from "@/lib/version";
import { cn } from "@/lib/utils";
import { appTargetsApi, installOf, isAppTargetApp } from "@/lib/api/appTargets";
import {
  APP_TARGETS_QUERY_KEY,
  useAppTargetsOverview,
} from "@/hooks/useAppTargets";

/**
 * 应用页：Claude Code、Codex、Gemini CLI、OpenCode 在各个 WSL 发行版里的安装，作为应用行
 * 下面的子行，和 Windows 那一处同一套列：名称列是环境标签 + 可执行文件路径，版本列是版本，
 * 操作列是安装 / 更新。停止的发行版不为此启动，用上次结果。
 */
export function WslInstallRows({
  tool,
  appName,
  latestVersion,
  gridClassName,
}: {
  tool: string;
  appName: string;
  latestVersion?: string | null;
  /** 与应用行相同的列网格（列宽和表头对齐） */
  gridClassName: string;
}) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const supported = isAppTargetApp(tool);
  const overview = useAppTargetsOverview(supported);
  const [busy, setBusy] = useState<string | null>(null);
  const distros = overview.data?.inventory.hostWindows
    ? overview.data.inventory.distros
    : [];
  if (!supported || distros.length === 0) return null;

  const run = async (distro: string, action: "install" | "update") => {
    if (!isAppTargetApp(tool)) return;
    setBusy(distro);
    try {
      await appTargetsApi.wslToolAction(tool, distro, action);
      toast.success(
        t(
          action === "install"
            ? "appTargets.installs.installed"
            : "appTargets.installs.updated",
          { app: appName, distro },
        ),
      );
    } catch (error) {
      toast.error(t("appTargets.installs.failed", { app: appName, distro }), {
        description: String(error),
        duration: 8000,
      });
    } finally {
      setBusy(null);
      await queryClient.invalidateQueries({ queryKey: APP_TARGETS_QUERY_KEY });
    }
  };

  return (
    <>
      {distros.map((env) => {
        const install = installOf(overview.data?.installs, env.distro, tool);
        const installed = install?.status === "installed";
        const outdated =
          installed &&
          isUpdateAvailable(install.version, latestVersion ?? undefined);
        const action =
          install?.status === "notInstalled"
            ? "install"
            : outdated
              ? "update"
              : null;
        const loading = busy === env.distro;
        const label = `WSL · ${env.distro}`;
        const staleNote = install?.cached
          ? env.running
            ? t("appTargets.installs.cached")
            : t("appTargets.installs.stoppedCached")
          : null;
        const version = installed ? (
          <div>
            <div className="font-mono text-body font-medium tabular-nums text-fg-1">
              {install.version ?? t("appTargets.installs.versionUnknown")}
            </div>
            {outdated && latestVersion && (
              <div className="text-caption text-fg-2">
                {t("appsPage.newVersion", { version: latestVersion })}
              </div>
            )}
          </div>
        ) : install?.status === "broken" ? (
          <span className="inline-flex items-center gap-1 text-caption font-medium text-warning-text">
            <AlertTriangle className="h-3.5 w-3.5" />
            {t("appsPage.notRunnable")}
          </span>
        ) : (
          <span className="text-caption text-fg-2">
            {install?.status === "notInstalled"
              ? t("common.notInstalled")
              : t("appTargets.installs.notChecked")}
          </span>
        );
        return (
          <div
            key={env.distro}
            data-wsl-install={env.distro}
            className={cn(gridClassName, "mt-2 min-h-10")}
          >
            <div className="flex min-w-0 items-center gap-4">
              {/* 占住应用图标的位置，环境标签和路径与上面的名称、路径对齐 */}
              <span aria-hidden className="w-5 shrink-0" />
              <div className="min-w-0 flex-1">
                <div className="flex min-w-0 items-center gap-1.5">
                  <HoverTip content={label}>
                    <span className="inline-flex h-[18px] max-w-[180px] shrink-0 items-center rounded-full border border-border-strong px-1.5 text-badge text-fg-2">
                      <span className="min-w-0 truncate">{label}</span>
                    </span>
                  </HoverTip>
                </div>
                {(install?.path || staleNote) && (
                  <div className="mt-0.5 flex min-w-0 items-center gap-1.5 text-caption text-fg-2">
                    {install?.path && (
                      <span
                        className="min-w-0 truncate font-mono"
                        title={install.path}
                      >
                        {install.path}
                      </span>
                    )}
                    {staleNote && <span className="shrink-0">{staleNote}</span>}
                  </div>
                )}
              </div>
            </div>
            <div className="text-end">{version}</div>
            <div
              data-tool-actions
              className="flex w-max items-center justify-self-end gap-2 [&>button]:shrink-0"
            >
              {action && (
                <Button
                  variant="neutral"
                  size="compact"
                  disabled={busy !== null}
                  aria-busy={loading}
                  aria-label={`${
                    action === "install"
                      ? t("settings.toolInstall")
                      : t("settings.toolUpdate")
                  } · ${label}`}
                  onClick={() => void run(env.distro, action)}
                >
                  {loading ? (
                    <Loader2 className="h-3.5 w-3.5 animate-spin" />
                  ) : action === "install" ? (
                    <Download className="h-3.5 w-3.5" />
                  ) : (
                    <ArrowUpCircle className="h-3.5 w-3.5" />
                  )}
                  {action === "install"
                    ? t("settings.toolInstall")
                    : t("settings.toolUpdate")}
                </Button>
              )}
            </div>
            <div />
            <div />
          </div>
        );
      })}
    </>
  );
}
