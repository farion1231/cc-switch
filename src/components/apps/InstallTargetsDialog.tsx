import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQueryClient } from "@tanstack/react-query";
import { Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { toast } from "@/lib/toast";
import {
  appTargetsApi,
  installOf,
  runningWslDistros,
  type AppTargetAppId,
} from "@/lib/api/appTargets";
import {
  APP_TARGETS_QUERY_KEY,
  useAppTargetsOverview,
} from "@/hooks/useAppTargets";

const WINDOWS = "windows";

interface InstallTargetsDialogProps {
  open: boolean;
  tool: AppTargetAppId;
  appName: string;
  /** Windows 上已经装了（这一处不能再勾） */
  windowsInstalled: boolean;
  windowsVersion?: string | null;
  onClose: () => void;
  /** 安装到 Windows：沿用应用页原来的安装流程 */
  onInstallWindows: () => Promise<void> | void;
}

/**
 * 安装前确认目标环境：先重新检测运行中的 WSL 发行版装没装，再让用户勾选装到 Windows 还是
 * 哪些 WSL。正常安装的不能勾；损坏的可重新安装；停止的发行版需先由用户启动。
 */
export function InstallTargetsDialog({
  open,
  tool,
  appName,
  windowsInstalled,
  windowsVersion,
  onClose,
  onInstallWindows,
}: InstallTargetsDialogProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const liveOverview = useAppTargetsOverview(open);
  const overview = liveOverview.data;
  const [detecting, setDetecting] = useState(false);
  const [selected, setSelected] = useState<string[]>([]);
  const [running, setRunning] = useState(false);

  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    setSelected(windowsInstalled ? [] : [WINDOWS]);
    setDetecting(true);
    appTargetsApi
      .overview(false, true)
      .then((data) => {
        if (cancelled) return;
        queryClient.setQueryData(APP_TARGETS_QUERY_KEY, data);
      })
      .catch((error) => {
        if (!cancelled) toast.error(String(error));
      })
      .finally(() => {
        if (!cancelled) setDetecting(false);
      });
    return () => {
      cancelled = true;
    };
  }, [open, windowsInstalled, queryClient]);

  const distros = runningWslDistros(overview?.inventory);
  const availableSelected = selected.filter(
    (key) => key === WINDOWS || distros.some((env) => env.distro === key),
  );

  const toggle = (key: string, checked: boolean) =>
    setSelected((current) =>
      checked ? [...current, key] : current.filter((k) => k !== key),
    );

  const confirm = async () => {
    setRunning(true);
    const wsl = availableSelected.filter((key) => key !== WINDOWS);
    try {
      if (availableSelected.includes(WINDOWS)) {
        await onInstallWindows();
      }
      for (const distro of wsl) {
        try {
          await appTargetsApi.wslToolAction(tool, distro, "install");
          const refreshed = await appTargetsApi.overview(false, true);
          queryClient.setQueryData(APP_TARGETS_QUERY_KEY, refreshed);
          const result = installOf(refreshed.installs, distro, tool);
          if (result?.status !== "installed" || result.cached) {
            toast.warning(
              t("appTargets.installs.notReady", { app: appName, distro }),
              {
                description: result?.probeError || result?.error,
                duration: 8000,
              },
            );
            continue;
          }
          toast.success(
            t(
              installOf(overview?.installs, distro, tool)?.status === "broken"
                ? "appTargets.installs.reinstalled"
                : "appTargets.installs.installed",
              { app: appName, distro },
            ),
          );
        } catch (error) {
          toast.error(
            t("appTargets.installs.failed", { app: appName, distro }),
            { description: String(error), duration: 8000 },
          );
        }
      }
    } finally {
      setRunning(false);
      await queryClient.invalidateQueries({ queryKey: APP_TARGETS_QUERY_KEY });
      onClose();
    }
  };

  const statusText = (distro: string, isRunning: boolean) => {
    const install = installOf(overview?.installs, distro, tool);
    const stale = install?.cached
      ? ` · ${
          isRunning
            ? t("appTargets.installs.cached")
            : t("appTargets.installs.stoppedCached")
        }`
      : "";
    if (install?.status === "installed") {
      return `${t("appTargets.installTargets.installed", {
        version: install.version ?? "",
      })}${stale}`;
    }
    if (install?.status === "broken")
      return `${t("appTargets.installTargets.broken")}${stale}`;
    if (!isRunning) return t("appTargets.installTargets.stopped");
    if (install?.status === "notInstalled") {
      return `${t("common.notInstalled")}${stale}`;
    }
    return `${t("appTargets.installs.notChecked")}${stale}`;
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next && !running) onClose();
      }}
    >
      <DialogContent className="max-w-md" zIndex="nested">
        <DialogHeader>
          <DialogTitle>
            {t("appTargets.installTargets.title", { app: appName })}
          </DialogTitle>
        </DialogHeader>
        <div className="space-y-2 px-6 py-4">
          <DialogDescription>
            {t("appTargets.installTargets.description")}
          </DialogDescription>
          <label className="flex items-center gap-3 rounded-control border border-border px-3 py-2">
            <Checkbox
              checked={windowsInstalled || selected.includes(WINDOWS)}
              disabled={windowsInstalled || running}
              aria-label="Windows"
              onCheckedChange={(checked) => toggle(WINDOWS, checked)}
            />
            <span className="min-w-0 flex-1 text-body text-fg-1">Windows</span>
            <span className="text-caption text-fg-2">
              {windowsInstalled
                ? t("appTargets.installTargets.installed", {
                    version: windowsVersion ?? "",
                  })
                : t("common.notInstalled")}
            </span>
          </label>
          {detecting ? (
            <div className="flex items-center gap-2 px-1 py-2 text-caption text-fg-2">
              <Loader2 className="h-3.5 w-3.5 animate-spin" />
              {t("appTargets.installTargets.detecting")}
            </div>
          ) : (
            distros.map((env) => {
              const install = installOf(overview?.installs, env.distro, tool);
              const present = install?.status === "installed";
              // 没在运行的发行版里安装会卡在启动上：不让勾，提示先启动它。
              const blocked = !present && !env.running;
              const label = `WSL · ${env.distro}`;
              return (
                <label
                  key={env.distro}
                  className="flex items-center gap-3 rounded-control border border-border px-3 py-2"
                >
                  <Checkbox
                    checked={present || selected.includes(env.distro)}
                    disabled={present || blocked || running}
                    aria-label={label}
                    onCheckedChange={(checked) => toggle(env.distro, checked)}
                  />
                  <div className="min-w-0 flex-1">
                    <div className="truncate text-body text-fg-1">{label}</div>
                    {(install?.probeError || install?.error) && (
                      <div className="mt-1 break-words text-caption text-warning-text">
                        {install?.probeError && (
                          <div>
                            {t(
                              install.cached
                                ? "appTargets.installs.probeFailedCached"
                                : "appTargets.installs.probeFailed",
                            )}
                          </div>
                        )}
                        {install?.probeError || install?.error}
                      </div>
                    )}
                    {install?.status === "broken" && (
                      <div className="mt-1 text-caption text-fg-2">
                        {t("appTargets.installs.repairHint")}
                      </div>
                    )}
                  </div>
                  <span className="shrink-0 text-caption text-fg-2">
                    {statusText(env.distro, env.running)}
                  </span>
                </label>
              );
            })
          )}
        </div>
        <DialogFooter>
          <Button variant="neutral" disabled={running} onClick={onClose}>
            {t("common.cancel")}
          </Button>
          <Button
            variant="solid"
            disabled={running || detecting || availableSelected.length === 0}
            onClick={() => void confirm()}
          >
            {running && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
            {t("appTargets.installTargets.confirm", {
              count: availableSelected.length,
            })}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
