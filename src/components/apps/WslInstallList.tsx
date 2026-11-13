import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useQueryClient } from "@tanstack/react-query";
import { ArrowUpCircle, Download, Loader2, RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { HoverTip } from "@/components/ui/hover-tip";
import { toast } from "@/lib/toast";
import { isUpdateAvailable } from "@/lib/version";
import { appTargetsApi, installOf, isAppTargetApp } from "@/lib/api/appTargets";
import {
  APP_TARGETS_QUERY_KEY,
  useAppTargetsOverview,
} from "@/hooks/useAppTargets";

/**
 * 应用页：Claude Code、Codex、Gemini CLI、OpenCode 在各个 WSL 发行版里的安装情况。
 * 每条绑定一个发行版：版本、上次检测、安装 / 更新。停止的发行版不为此启动，用上次结果。
 */
export function WslInstallList({
  tool,
  appName,
  latestVersion,
}: {
  tool: string;
  appName: string;
  latestVersion?: string | null;
}) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const supported = isAppTargetApp(tool);
  const overview = useAppTargetsOverview(supported);
  const [busy, setBusy] = useState<string | null>(null);
  const [detecting, setDetecting] = useState(false);
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

  const detect = async () => {
    setDetecting(true);
    try {
      queryClient.setQueryData(
        APP_TARGETS_QUERY_KEY,
        await appTargetsApi.overview(true),
      );
    } finally {
      setDetecting(false);
    }
  };

  return (
    <div className="space-y-1">
      {distros.map((env) => {
        const install = installOf(overview.data?.installs, env.distro, tool);
        const outdated =
          install?.status === "installed" &&
          isUpdateAvailable(install.version, latestVersion ?? undefined);
        const loading = busy === env.distro;
        return (
          <div
            key={env.distro}
            className="flex min-h-7 items-center gap-2 text-caption text-fg-2"
          >
            <HoverTip content={`WSL · ${env.distro}`}>
              <span className="inline-flex h-[18px] max-w-[160px] shrink-0 items-center rounded-full border border-border-strong px-1.5 text-badge text-fg-2">
                <span className="min-w-0 truncate">WSL · {env.distro}</span>
              </span>
            </HoverTip>
            <span className="min-w-0 flex-1 truncate">
              {install?.status === "installed" ? (
                <>
                  <span className="font-mono text-fg-1">
                    {install.version ?? t("appTargets.installs.versionUnknown")}
                  </span>
                  {outdated && latestVersion && (
                    <>
                      {" · "}
                      {t("appsPage.newVersion", { version: latestVersion })}
                    </>
                  )}
                </>
              ) : install?.status === "broken" ? (
                <span className="text-warning-text">
                  {t("appsPage.notRunnable")}
                </span>
              ) : install?.status === "notInstalled" ? (
                t("common.notInstalled")
              ) : (
                t("appTargets.installs.notChecked")
              )}
              {install?.cached && (
                <>
                  {" · "}
                  {env.running
                    ? t("appTargets.installs.cached")
                    : t("appTargets.installs.stoppedCached")}
                </>
              )}
            </span>
            {(install?.status === "notInstalled" || outdated) && (
              <Button
                variant="neutral"
                size="compact"
                disabled={busy !== null}
                aria-busy={loading}
                onClick={() =>
                  void run(
                    env.distro,
                    install?.status === "notInstalled" ? "install" : "update",
                  )
                }
              >
                {loading ? (
                  <Loader2 className="h-3.5 w-3.5 animate-spin" />
                ) : install?.status === "notInstalled" ? (
                  <Download className="h-3.5 w-3.5" />
                ) : (
                  <ArrowUpCircle className="h-3.5 w-3.5" />
                )}
                {install?.status === "notInstalled"
                  ? t("settings.toolInstall")
                  : t("settings.toolUpdate")}
              </Button>
            )}
          </div>
        );
      })}
      <Button
        variant="quiet"
        size="compact"
        disabled={detecting}
        onClick={() => void detect()}
      >
        <RefreshCw
          className={detecting ? "h-3.5 w-3.5 animate-spin" : "h-3.5 w-3.5"}
        />
        {t("appTargets.installs.detect")}
      </Button>
    </div>
  );
}
