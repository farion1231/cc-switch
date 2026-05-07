import { useCallback, useEffect, useState } from "react";
import {
  Download,
  Copy,
  ExternalLink,
  Info,
  Loader2,
  RefreshCw,
  Terminal,
  CheckCircle2,
  AlertCircle,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { getVersion } from "@tauri-apps/api/app";
import { settingsApi } from "@/lib/api";
import { useUpdate } from "@/contexts/UpdateContext";
import { relaunchApp } from "@/lib/updater";
import { Badge } from "@/components/ui/badge";
import { motion } from "framer-motion";
import appIcon from "@/assets/icons/app-icon.png";
import { EnvironmentDoctorPanel } from "./EnvironmentDoctorPanel";
import { doctorApi, type DiagnosisResult } from "@/lib/api/doctor";

interface AboutSectionProps {
  isPortable: boolean;
}

interface ToolVersion {
  name: string;
  version: string | null;
  latest_version: string | null;
  error: string | null;
  env_type: "windows" | "wsl" | "macos" | "linux" | "unknown";
  wsl_distro: string | null;
}

type InstallActionState = "install" | "upgrade" | "installed";

const TOOL_NAMES = ["claude"] as const;
type ToolName = (typeof TOOL_NAMES)[number];

type WslShellPreference = {
  wslShell?: string | null;
  wslShellFlag?: string | null;
};

const WSL_SHELL_OPTIONS = ["sh", "bash", "zsh", "fish", "dash"] as const;
// UI-friendly order: login shell first.
const WSL_SHELL_FLAG_OPTIONS = ["-lic", "-lc", "-c"] as const;

const ENV_BADGE_CONFIG: Record<
  string,
  { labelKey: string; className: string }
> = {
  wsl: {
    labelKey: "settings.envBadge.wsl",
    className:
      "bg-orange-500/10 text-orange-600 dark:text-orange-400 border-orange-500/20",
  },
  windows: {
    labelKey: "settings.envBadge.windows",
    className:
      "bg-blue-500/10 text-blue-600 dark:text-blue-400 border-blue-500/20",
  },
  macos: {
    labelKey: "settings.envBadge.macos",
    className:
      "bg-gray-500/10 text-gray-600 dark:text-gray-400 border-gray-500/20",
  },
  linux: {
    labelKey: "settings.envBadge.linux",
    className:
      "bg-green-500/10 text-green-600 dark:text-green-400 border-green-500/20",
  },
};

const ONE_CLICK_INSTALL_COMMANDS = `# Claude Code (Native install - recommended)
curl -fsSL https://claude.ai/install.sh | bash`;

export function AboutSection({ isPortable }: AboutSectionProps) {
  // ... (use hooks as before) ...
  const { t } = useTranslation();
  const [version, setVersion] = useState<string | null>(null);
  const [isLoadingVersion, setIsLoadingVersion] = useState(true);
  const [isDownloading, setIsDownloading] = useState(false);
  const [toolVersions, setToolVersions] = useState<ToolVersion[]>([]);
  const [isLoadingTools, setIsLoadingTools] = useState(true);

  // 环境诊断状态
  const [diagnosis, setDiagnosis] = useState<DiagnosisResult | null>(null);
  const [isInstalling, setIsInstalling] = useState(false);
  const [isVerifyingInstall, setIsVerifyingInstall] = useState(false);
  const [isFixing, setIsFixing] = useState(false);

  const {
    hasUpdate,
    updateInfo,
    updateHandle,
    checkUpdate,
    resetDismiss,
    isChecking,
  } = useUpdate();

  const [wslShellByTool, setWslShellByTool] = useState<
    Record<string, WslShellPreference>
  >({});
  const [loadingTools, setLoadingTools] = useState<Record<string, boolean>>({});

  const refreshToolVersions = useCallback(
    async (
      toolNames: ToolName[],
      wslOverrides?: Record<string, WslShellPreference>,
    ) => {
      if (toolNames.length === 0) return;

      // 单工具刷新使用统一后端入口（get_tool_versions）并带工具过滤。
      setLoadingTools((prev) => {
        const next = { ...prev };
        for (const name of toolNames) next[name] = true;
        return next;
      });

      try {
        const updated = await settingsApi.getToolVersions(
          toolNames,
          wslOverrides,
        );

        setToolVersions((prev) => {
          if (prev.length === 0) return updated;
          const byName = new Map(updated.map((t) => [t.name, t]));
          const merged = prev.map((t) => byName.get(t.name) ?? t);
          const existing = new Set(prev.map((t) => t.name));
          for (const u of updated) {
            if (!existing.has(u.name)) merged.push(u);
          }
          return merged;
        });
      } catch (error) {
        console.error("[AboutSection] Failed to refresh tools", error);
      } finally {
        setLoadingTools((prev) => {
          const next = { ...prev };
          for (const name of toolNames) next[name] = false;
          return next;
        });
      }
    },
    [],
  );

  const loadAllToolVersions = useCallback(async () => {
    setIsLoadingTools(true);
    try {
      // Respect current UI overrides (shell / flag) when doing a full refresh.
      const versions = await settingsApi.getToolVersions(
        [...TOOL_NAMES],
        wslShellByTool,
      );
      setToolVersions(versions);
    } catch (error) {
      console.error("[AboutSection] Failed to load tool versions", error);
    } finally {
      setIsLoadingTools(false);
    }
  }, [wslShellByTool]);

  const handleToolShellChange = async (toolName: ToolName, value: string) => {
    const wslShell = value === "auto" ? null : value;
    const nextPref: WslShellPreference = {
      ...(wslShellByTool[toolName] ?? {}),
      wslShell,
    };
    setWslShellByTool((prev) => ({ ...prev, [toolName]: nextPref }));
    await refreshToolVersions([toolName], { [toolName]: nextPref });
  };

  const handleToolShellFlagChange = async (
    toolName: ToolName,
    value: string,
  ) => {
    const wslShellFlag = value === "auto" ? null : value;
    const nextPref: WslShellPreference = {
      ...(wslShellByTool[toolName] ?? {}),
      wslShellFlag,
    };
    setWslShellByTool((prev) => ({ ...prev, [toolName]: nextPref }));
    await refreshToolVersions([toolName], { [toolName]: nextPref });
  };

  useEffect(() => {
    let active = true;
    const load = async () => {
      try {
        const appVersion = await getVersion();

        if (active) {
          setVersion(appVersion);
        }

        await Promise.all([
          loadAllToolVersions(),
          runDiagnosis(),
        ]);
      } catch (error) {
        console.error("[AboutSection] Failed to load info", error);
        if (active) {
          setVersion(null);
        }
      } finally {
        if (active) {
          setIsLoadingVersion(false);
        }
      }
    };

    void load();
    return () => {
      active = false;
    };
    // Mount-only: loadAllToolVersions is intentionally excluded to avoid
    // re-fetching all tools whenever wslShellByTool changes. Single-tool
    // refreshes are handled by refreshToolVersions in the shell/flag handlers.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // ... (handlers like handleOpenReleaseNotes, handleCheckUpdate) ...

  const handleOpenReleaseNotes = useCallback(async () => {
    try {
      const targetVersion = updateInfo?.availableVersion ?? version ?? "";
      const displayVersion = targetVersion.startsWith("v")
        ? targetVersion
        : targetVersion
          ? `v${targetVersion}`
          : "";

      if (!displayVersion) {
        await settingsApi.openExternal(
          "https://github.com/farion1231/cc-switch/releases",
        );
        return;
      }

      await settingsApi.openExternal(
        `https://github.com/farion1231/cc-switch/releases/tag/${displayVersion}`,
      );
    } catch (error) {
      console.error("[AboutSection] Failed to open release notes", error);
      toast.error(t("settings.openReleaseNotesFailed"));
    }
  }, [t, updateInfo?.availableVersion, version]);

  const handleCheckUpdate = useCallback(async () => {
    if (hasUpdate && updateHandle) {
      if (isPortable) {
        try {
          await settingsApi.checkUpdates();
        } catch (error) {
          console.error("[AboutSection] Portable update failed", error);
        }
        return;
      }

      setIsDownloading(true);
      try {
        resetDismiss();
        await updateHandle.downloadAndInstall();
        await relaunchApp();
      } catch (error) {
        console.error("[AboutSection] Update failed", error);
        toast.error(t("settings.updateFailed"));
        try {
          await settingsApi.checkUpdates();
        } catch (fallbackError) {
          console.error(
            "[AboutSection] Failed to open fallback updater",
            fallbackError,
          );
        }
      } finally {
        setIsDownloading(false);
      }
      return;
    }

    try {
      const available = await checkUpdate();
      if (!available) {
        toast.success(t("settings.upToDate"), { closeButton: true });
      }
    } catch (error) {
      console.error("[AboutSection] Check update failed", error);
      toast.error(t("settings.checkUpdateFailed"));
    }
  }, [checkUpdate, hasUpdate, isPortable, resetDismiss, t, updateHandle]);

  const handleCopyInstallCommands = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(ONE_CLICK_INSTALL_COMMANDS);
      toast.success(t("settings.installCommandsCopied"), { closeButton: true });
    } catch (error) {
      console.error("[AboutSection] Failed to copy install commands", error);
      toast.error(t("settings.installCommandsCopyFailed"));
    }
  }, [t]);

  // 环境诊断相关函数
  const runDiagnosis = useCallback(async () => {
    try {
      const result = await doctorApi.diagnoseEnvironment();
      setDiagnosis(result);
    } catch (error) {
      console.error("[AboutSection] Failed to diagnose environment", error);
      // 静默失败，不影响其他功能
    }
  }, []);

  const handleInstall = useCallback(async (tool: string) => {
    const toolLabel = tool === "claude" ? "Claude Code" : tool;
    setIsInstalling(true);
    try {
      const result = await doctorApi.installTool(tool);

      if (result.already_installed || result.action === "none") {
        toast.success(t("doctor.alreadyInstalled", { tool: toolLabel }));
        await Promise.all([runDiagnosis(), loadAllToolVersions()]);
        return;
      }

      if (!result.success) {
        toast.error(
          result.message || t("doctor.installFailedGeneric"),
          { closeButton: true },
        );
        await Promise.all([runDiagnosis(), loadAllToolVersions()]);
        return;
      }

      setIsVerifyingInstall(true);
      await Promise.all([runDiagnosis(), loadAllToolVersions()]);

      if (result.verified === false) {
        toast.error(t("doctor.installVerificationFailed"), {
          closeButton: true,
        });
        return;
      }

      if (result.action === "upgrade" && result.installed_version) {
        toast.success(
          t("doctor.upgradeSuccess", {
            tool: toolLabel,
            version: result.installed_version,
          }),
          { closeButton: true },
        );
        return;
      }

      toast.success(t("doctor.installSuccess", { tool: toolLabel }), {
        closeButton: true,
      });
    } catch (error) {
      console.error("[AboutSection] Failed to install tool", error);
      toast.error(t("doctor.installFailedGeneric"), { closeButton: true });
    } finally {
      setIsInstalling(false);
      setIsVerifyingInstall(false);
    }
  }, [loadAllToolVersions, runDiagnosis, t]);

  const handleFix = useCallback(async () => {
    if (!diagnosis) return;

    setIsFixing(true);
    try {
      const fixableIssues = diagnosis.issues.filter((i) => i.auto_fixable);
      const result = await doctorApi.fixEnvironment(fixableIssues);

      if (result.fixed.length > 0) {
        toast.success(t("doctor.fixSuccess", { count: result.fixed.length }));
      }

      if (result.failed.length > 0) {
        const failedMessages = result.failed.map(([id, err]) => `${id}: ${err}`).join("\n");
        toast.error(t("doctor.fixFailed", { error: failedMessages }));
      }

      await runDiagnosis(); // 重新诊断
    } catch (error) {
      console.error("[AboutSection] Failed to fix environment", error);
      toast.error(t("doctor.fixFailed", { error: String(error) }));
    } finally {
      setIsFixing(false);
    }
  }, [t, diagnosis, runDiagnosis]);

  const displayVersion = version ?? t("common.unknown");
  const claudeTool = toolVersions.find((item) => item.name === "claude");
  const claudeInstalled = Boolean(claudeTool?.version);
  const claudeUpgradable = Boolean(
    claudeTool?.version &&
      claudeTool.latest_version &&
      claudeTool.version !== claudeTool.latest_version,
  );
  const installActionState: InstallActionState = claudeInstalled
    ? claudeUpgradable
      ? "upgrade"
      : "installed"
    : "install";
  const installButtonLabel = isInstalling
    ? t("doctor.installing")
    : isVerifyingInstall
      ? t("doctor.verifying")
      : installActionState === "upgrade"
        ? t("settings.upgradeNow")
        : installActionState === "installed"
          ? t("settings.installed")
          : t("settings.installNow");
  const installHint = isInstalling
    ? t("doctor.installing")
    : isVerifyingInstall
      ? t("settings.verifyingInstall")
      : installActionState === "upgrade"
        ? t("settings.upgradeReady")
        : installActionState === "installed"
          ? t("settings.installedStatusHint")
          : t("settings.installReady");

  return (
    <motion.section
      initial={{ opacity: 0, y: 10 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.3 }}
      className="space-y-6"
    >
      <header className="space-y-1">
        <h3 className="text-sm font-medium">{t("common.about")}</h3>
        <p className="text-xs text-muted-foreground">
          {t("settings.aboutHint")}
        </p>
      </header>

      <motion.div
        initial={{ opacity: 0, scale: 0.98 }}
        animate={{ opacity: 1, scale: 1 }}
        transition={{ duration: 0.3, delay: 0.1 }}
        className="rounded-xl border border-border bg-gradient-to-br from-card/80 to-card/40 p-6 space-y-5 shadow-sm"
      >
        <div className="flex flex-col gap-4 sm:flex-row sm:items-center sm:justify-between">
          <div className="space-y-2">
            <div className="flex items-center gap-2">
              <img src={appIcon} alt="CC Doctor" className="h-5 w-5" />
              <h4 className="text-lg font-semibold text-foreground">
                {t("app.title")}
              </h4>
            </div>
            <div className="flex items-center gap-2">
              <Badge variant="outline" className="gap-1.5 bg-background/80">
                <span className="text-muted-foreground">
                  {t("common.version")}
                </span>
                {isLoadingVersion ? (
                  <Loader2 className="h-3 w-3 animate-spin" />
                ) : (
                  <span className="font-medium">{`v${displayVersion}`}</span>
                )}
              </Badge>
              {isPortable && (
                <Badge variant="secondary" className="gap-1.5">
                  <Info className="h-3 w-3" />
                  {t("settings.portableMode")}
                </Badge>
              )}
            </div>
          </div>

          <div className="flex items-center gap-2">
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={handleOpenReleaseNotes}
              className="h-8 gap-1.5 text-xs"
            >
              <ExternalLink className="h-3.5 w-3.5" />
              {t("settings.releaseNotes")}
            </Button>
            <Button
              type="button"
              size="sm"
              onClick={handleCheckUpdate}
              disabled={isChecking || isDownloading}
              className="h-8 gap-1.5 text-xs"
            >
              {isDownloading ? (
                <>
                  <Loader2 className="h-3.5 w-3.5 animate-spin" />
                  {t("settings.updating")}
                </>
              ) : hasUpdate ? (
                <>
                  <Download className="h-3.5 w-3.5" />
                  {t("settings.updateTo", {
                    version: updateInfo?.availableVersion ?? "",
                  })}
                </>
              ) : isChecking ? (
                <>
                  <RefreshCw className="h-3.5 w-3.5 animate-spin" />
                  {t("settings.checking")}
                </>
              ) : (
                <>
                  <RefreshCw className="h-3.5 w-3.5" />
                  {t("settings.checkForUpdates")}
                </>
              )}
            </Button>
          </div>
        </div>

        {hasUpdate && updateInfo && (
          <motion.div
            initial={{ opacity: 0, height: 0 }}
            animate={{ opacity: 1, height: "auto" }}
            className="rounded-lg bg-primary/10 border border-primary/20 px-4 py-3 text-sm"
          >
            <p className="font-medium text-primary mb-1">
              {t("settings.updateAvailable", {
                version: updateInfo.availableVersion,
              })}
            </p>
            {updateInfo.notes && (
              <p className="text-muted-foreground line-clamp-3 leading-relaxed">
                {updateInfo.notes}
              </p>
            )}
          </motion.div>
        )}
      </motion.div>

      {/* 环境诊断面板 */}
      {diagnosis && (
        <EnvironmentDoctorPanel
          diagnosis={diagnosis}
          onInstall={handleInstall}
          onFix={handleFix}
          isInstalling={isInstalling}
          isFixing={isFixing}
        />
      )}

      <div className="space-y-3">
          <div className="flex items-center justify-between px-1">
            <h3 className="text-sm font-medium">
              {t("settings.localEnvCheck")}
            </h3>
            <Button
              size="sm"
              variant="outline"
              className="h-7 gap-1.5 text-xs"
              onClick={() => loadAllToolVersions()}
              disabled={isLoadingTools}
            >
              <RefreshCw
                className={
                  isLoadingTools ? "h-3.5 w-3.5 animate-spin" : "h-3.5 w-3.5"
                }
              />
              {isLoadingTools ? t("common.refreshing") : t("common.refresh")}
            </Button>
          </div>

      <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4 px-1">
            {(() => {
              const cards = claudeTool ? [claudeTool] : [];

              return cards.map((tool, index) => {
                const toolName = tool.name as ToolName;
                const displayName = "Claude Code";
                const title = tool.version || tool.error || t("common.unknown");
                const versionText = tool.version ? tool.version : tool.error || t("common.notInstalled");

                return (
                  <motion.div
                    key={tool.name}
                    initial={{ opacity: 0, y: 10 }}
                    animate={{ opacity: 1, y: 0 }}
                    transition={{ duration: 0.3, delay: 0.15 + index * 0.05 }}
                    whileHover={{ scale: 1.02 }}
                    className="flex flex-col gap-2 rounded-xl border border-border bg-gradient-to-br from-card/80 to-card/40 p-4 shadow-sm transition-colors hover:border-primary/30"
                  >
                    <div className="flex items-center justify-between">
                      <div className="flex items-center gap-2">
                        <Terminal className="h-4 w-4 text-muted-foreground" />
                        <span className="text-sm font-medium">{displayName}</span>
                        {tool?.env_type && ENV_BADGE_CONFIG[tool.env_type] && (
                          <span
                            className={`text-[9px] px-1.5 py-0.5 rounded-full border ${ENV_BADGE_CONFIG[tool.env_type].className}`}
                          >
                            {t(ENV_BADGE_CONFIG[tool.env_type].labelKey)}
                          </span>
                        )}
                        {tool.name === "claude" && tool?.env_type === "wsl" && (
                          <>
                            <Select
                              value={wslShellByTool[toolName]?.wslShell || "auto"}
                              onValueChange={(v) => handleToolShellChange(toolName, v)}
                              disabled={isLoadingTools || loadingTools[toolName]}
                            >
                              <SelectTrigger className="h-6 w-[70px] text-xs">
                                <SelectValue />
                              </SelectTrigger>
                              <SelectContent>
                                <SelectItem value="auto">{t("common.auto")}</SelectItem>
                                {WSL_SHELL_OPTIONS.map((shell) => (
                                  <SelectItem key={shell} value={shell}>
                                    {shell}
                                  </SelectItem>
                                ))}
                              </SelectContent>
                            </Select>
                            <Select
                              value={wslShellByTool[toolName]?.wslShellFlag || "auto"}
                              onValueChange={(v) => handleToolShellFlagChange(toolName, v)}
                              disabled={isLoadingTools || loadingTools[toolName]}
                            >
                              <SelectTrigger className="h-6 w-[70px] text-xs">
                                <SelectValue />
                              </SelectTrigger>
                              <SelectContent>
                                <SelectItem value="auto">{t("common.auto")}</SelectItem>
                                {WSL_SHELL_FLAG_OPTIONS.map((flag) => (
                                  <SelectItem key={flag} value={flag}>
                                    {flag}
                                  </SelectItem>
                                ))}
                              </SelectContent>
                            </Select>
                          </>
                        )}
                      </div>
                      {isLoadingTools || (tool.name === "claude" && loadingTools[toolName]) ? (
                        <Loader2 className="h-4 w-4 animate-spin text-muted-foreground" />
                      ) : tool.version ? (
                        tool.latest_version && tool.version !== tool.latest_version ? (
                          <span className="text-[10px] px-1.5 py-0.5 rounded-full bg-yellow-500/10 text-yellow-600 dark:text-yellow-400 border border-yellow-500/20">
                            {tool.latest_version}
                          </span>
                        ) : (
                          <CheckCircle2 className="h-4 w-4 text-green-500" />
                        )
                      ) : (
                        <AlertCircle className="h-4 w-4 text-yellow-500" />
                      )}
                    </div>
                    <div
                      className="text-xs font-mono text-muted-foreground truncate"
                      title={title}
                    >
                      {isLoadingTools ? t("common.loading") : versionText}
                    </div>
                  </motion.div>
                );
              });
            })()}
          </div>
        </div>

        <motion.div
          initial={{ opacity: 0, y: 10 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.3, delay: 0.3 }}
          className="space-y-3"
        >
          <h3 className="text-sm font-medium px-1">
            {t("settings.oneClickInstall")}
          </h3>
          <div className="rounded-xl border border-border bg-gradient-to-br from-card/80 to-card/40 p-4 space-y-3 shadow-sm">
            <div className="flex items-center justify-between gap-2">
              <p className="text-xs text-muted-foreground">
                {installHint}
              </p>
              <div className="flex items-center gap-2">
                <Button
                  size="sm"
                  onClick={() => handleInstall("claude")}
                  disabled={
                    isInstalling ||
                    isVerifyingInstall ||
                    installActionState === "installed"
                  }
                  className="h-7 gap-1.5 text-xs"
                >
                  {isInstalling || isVerifyingInstall ? (
                    <>
                      <Loader2 className="h-3.5 w-3.5 animate-spin" />
                      {installButtonLabel}
                    </>
                  ) : installActionState === "upgrade" ? (
                    <>
                      <Download className="h-3.5 w-3.5" />
                      {installButtonLabel}
                    </>
                  ) : installActionState === "installed" ? (
                    <>
                      <CheckCircle2 className="h-3.5 w-3.5" />
                      {installButtonLabel}
                    </>
                  ) : (
                    <>
                      <Download className="h-3.5 w-3.5" />
                      {installButtonLabel}
                    </>
                  )}
                </Button>
                <Button
                  size="sm"
                  variant="outline"
                  onClick={handleCopyInstallCommands}
                  className="h-7 gap-1.5 text-xs"
                >
                  <Copy className="h-3.5 w-3.5" />
                  {t("common.copy")}
                </Button>
              </div>
            </div>
            <pre className="text-xs font-mono bg-background/80 px-3 py-2.5 rounded-lg border border-border/60 overflow-x-auto">
              {ONE_CLICK_INSTALL_COMMANDS}
            </pre>
          </div>
        </motion.div>
    </motion.section>
  );
}
