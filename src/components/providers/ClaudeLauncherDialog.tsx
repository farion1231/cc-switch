import { useEffect, useMemo, useState } from "react";
import {
  AlertCircle,
  Check,
  ChevronDown,
  ChevronUp,
  Copy,
  Download,
  FolderOpen,
  Loader2,
  RefreshCw,
  Rocket,
  Trash2,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import type { ClaudeLauncherPermissionMode, Provider } from "@/types";
import type {
  ClaudeLauncherSettingsUpdate,
  ClaudeShortcutCommandResult,
} from "@/lib/api/providers";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";

type PermissionModeInput = ClaudeLauncherPermissionMode | "inherit";

type PrimaryActionKind =
  | "close"
  | "disable"
  | "enable-install"
  | "install"
  | "save"
  | "update"
  | "conflict";

interface ClaudeLauncherDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  provider: Provider | null;
  onSaveLauncherSettings: (
    providerId: string,
    settings: ClaudeLauncherSettingsUpdate,
  ) => Promise<Provider>;
  onSyncProfile: (providerId: string) => Promise<string>;
  onOpenProfileDir: (path: string) => void;
  onGetLauncherStatus: (
    providerId: string,
  ) => Promise<ClaudeShortcutCommandResult>;
  onInstallLauncher: (
    providerId: string,
    alias?: string,
    launcherPermissionMode?: ClaudeLauncherPermissionMode | null,
    removePreviousShortcut?: boolean,
  ) => Promise<ClaudeShortcutCommandResult>;
  onRemoveLauncher: (
    providerId: string,
  ) => Promise<ClaudeShortcutCommandResult>;
}

const launcherBadgeClass: Record<string, string> = {
  installed:
    "border-emerald-200 bg-emerald-50 text-emerald-700 dark:border-emerald-900 dark:bg-emerald-950 dark:text-emerald-300",
  stale:
    "border-amber-200 bg-amber-50 text-amber-700 dark:border-amber-900 dark:bg-amber-950 dark:text-amber-300",
  missing:
    "border-slate-200 bg-slate-50 text-slate-600 dark:border-slate-800 dark:bg-slate-950 dark:text-slate-300",
  conflict:
    "border-red-200 bg-red-50 text-red-700 dark:border-red-900 dark:bg-red-950 dark:text-red-300",
};

const permissionModeOptions: PermissionModeInput[] = [
  "inherit",
  "default",
  "acceptEdits",
  "plan",
  "auto",
  "dontAsk",
  "bypassPermissions",
];

function permissionModeToSave(
  mode: PermissionModeInput,
): ClaudeLauncherPermissionMode | null {
  return mode === "inherit" ? null : mode;
}

function validateAlias(alias: string): "empty" | "invalid" | null {
  if (!alias) {
    return "empty";
  }
  if (
    alias === "." ||
    alias === ".." ||
    alias.startsWith("-") ||
    !/^[A-Za-z0-9._-]+$/.test(alias)
  ) {
    return "invalid";
  }
  return null;
}

export function ClaudeLauncherDialog({
  open,
  onOpenChange,
  provider,
  onSaveLauncherSettings,
  onSyncProfile,
  onOpenProfileDir,
  onGetLauncherStatus,
  onInstallLauncher,
  onRemoveLauncher,
}: ClaudeLauncherDialogProps) {
  const { t } = useTranslation();
  const [copied, setCopied] = useState<string | null>(null);
  const [draftEnabled, setDraftEnabled] = useState(false);
  const [draftAlias, setDraftAlias] = useState("");
  const [draftPermissionMode, setDraftPermissionMode] =
    useState<PermissionModeInput>("inherit");
  const [advancedOpen, setAdvancedOpen] = useState(false);
  const [launcherStatus, setLauncherStatus] =
    useState<ClaudeShortcutCommandResult | null>(null);
  const [loading, setLoading] = useState(false);
  const [syncing, setSyncing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [pendingBypassAction, setPendingBypassAction] = useState<
    (() => Promise<void>) | null
  >(null);
  const [pendingAliasCleanupAction, setPendingAliasCleanupAction] = useState<
    (() => Promise<void>) | null
  >(null);

  const enabled = provider?.meta?.parallelConfigEnabled ?? false;
  const profilePath = provider?.meta?.managedProfilePath;
  const savedAlias = provider?.meta?.shortcutName || "";
  const savedPermissionMode: PermissionModeInput =
    provider?.meta?.launcherPermissionMode ?? "inherit";
  const launcherInfo = launcherStatus?.info;
  const currentAlias = launcherInfo?.name || savedAlias;
  const normalizedAlias = draftAlias.trim();
  const selectedPermissionMode = permissionModeToSave(draftPermissionMode);
  const launcherStatusLabel = launcherInfo
    ? t(`provider.launcherStatus.${launcherInfo.status}`, {
        defaultValue: launcherInfo.status,
      })
    : t("provider.launcherStatus.missing", { defaultValue: "Not installed" });
  const selectedModeDescription = t(
    `provider.launcherPermissionModeDescriptions.${draftPermissionMode}`,
    { defaultValue: "" },
  );

  const aliasValidation = draftEnabled ? validateAlias(normalizedAlias) : null;
  const aliasValidationMessage =
    aliasValidation === "empty"
      ? t("provider.launcherAliasRequired", {
          defaultValue: "Command alias is required.",
        })
      : aliasValidation === "invalid"
        ? t("provider.launcherAliasInvalid", {
            defaultValue:
              "Use ASCII letters, numbers, dots, underscores, or hyphens. Do not start with a hyphen.",
          })
        : null;
  const aliasDirty = normalizedAlias !== (currentAlias || "");
  const permissionDirty = draftPermissionMode !== savedPermissionMode;
  const enabledDirty = draftEnabled !== enabled;
  const dirty = aliasDirty || permissionDirty || enabledDirty;
  const unresolvedConflict =
    draftEnabled && launcherInfo?.status === "conflict" && !aliasDirty;
  const canRemoveLauncher =
    launcherInfo?.status === "installed" || launcherInfo?.status === "stale";
  const shouldConfirmAliasCleanup =
    draftEnabled &&
    aliasDirty &&
    canRemoveLauncher &&
    Boolean(currentAlias) &&
    Boolean(normalizedAlias);

  const primaryActionKind: PrimaryActionKind = useMemo(() => {
    if (!draftEnabled) {
      return enabled ? "disable" : "close";
    }
    if (!enabled) {
      return "enable-install";
    }
    if (unresolvedConflict) {
      return "conflict";
    }
    if (!launcherInfo || launcherInfo.status === "missing") {
      return "install";
    }
    if (launcherInfo.status === "stale" || aliasDirty || permissionDirty) {
      return "update";
    }
    if (dirty) {
      return "save";
    }
    return "close";
  }, [
    aliasDirty,
    dirty,
    draftEnabled,
    enabled,
    launcherInfo,
    permissionDirty,
    unresolvedConflict,
  ]);

  const primaryLabel = useMemo(() => {
    switch (primaryActionKind) {
      case "disable":
        return t("provider.launcherPrimarySaveDisable", {
          defaultValue: "Save and Disable",
        });
      case "enable-install":
        return t("provider.launcherPrimaryEnableInstall", {
          defaultValue: "Enable and Install",
        });
      case "install":
        return t("provider.launcherPrimaryInstall", {
          defaultValue: "Install Command",
        });
      case "update":
        return t("provider.launcherPrimarySaveUpdate", {
          defaultValue: "Save and Update Command",
        });
      case "save":
        return t("provider.launcherPrimarySave", {
          defaultValue: "Save Changes",
        });
      case "conflict":
        return t("provider.launcherPrimaryResolveConflict", {
          defaultValue: "Choose Another Alias",
        });
      case "close":
      default:
        return t("provider.launcherPrimaryDone", {
          defaultValue: "Done",
        });
    }
  }, [primaryActionKind, t]);

  const primaryDisabled =
    loading ||
    syncing ||
    Boolean(aliasValidationMessage) ||
    primaryActionKind === "conflict";

  const maybeConfirmBypass = (action: () => Promise<void>) => {
    if (
      draftPermissionMode === "bypassPermissions" &&
      savedPermissionMode !== "bypassPermissions"
    ) {
      setPendingBypassAction(() => action);
      return;
    }
    void action();
  };

  const maybeConfirmAliasCleanup = (
    action: () => Promise<void>,
    shouldConfirm: boolean,
  ) => {
    if (shouldConfirm) {
      setPendingAliasCleanupAction(() => action);
      return;
    }
    maybeConfirmBypass(action);
  };

  const refreshLauncherStatus = async (options?: { syncAlias?: boolean }) => {
    if (!provider) return null;

    setLoading(true);
    setError(null);
    try {
      const result = await onGetLauncherStatus(provider.id);
      setLauncherStatus(result);
      if (options?.syncAlias) {
        setDraftAlias(result.info.name);
      }
      return result;
    } catch (e: any) {
      setError(e?.message || String(e));
      return null;
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    if (!open || !provider) return;

    let cancelled = false;
    setDraftEnabled(enabled);
    setDraftAlias(savedAlias);
    setDraftPermissionMode(savedPermissionMode);
    setAdvancedOpen(false);
    setLauncherStatus(null);
    setError(null);
    setCopied(null);
    setLoading(true);

    onGetLauncherStatus(provider.id)
      .then((result) => {
        if (!cancelled) {
          setLauncherStatus(result);
          setDraftAlias(result.info.name);
        }
      })
      .catch((e: any) => {
        if (!cancelled) setError(e?.message || String(e));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });

    return () => {
      cancelled = true;
    };
  }, [
    enabled,
    open,
    provider?.id,
    savedAlias,
    savedPermissionMode,
    onGetLauncherStatus,
  ]);

  if (!provider) return null;

  const handleCopy = (text: string, label: string) => {
    void navigator.clipboard.writeText(text);
    setCopied(label);
    setTimeout(() => setCopied(null), 2000);
  };

  const saveDraftSettings = async (
    nextEnabled = draftEnabled,
    options?: { includeAlias?: boolean },
  ) => {
    const settings: ClaudeLauncherSettingsUpdate = {
      enabled: nextEnabled,
      launcherPermissionMode: selectedPermissionMode,
    };

    if (nextEnabled && options?.includeAlias !== false) {
      settings.shortcutName = normalizedAlias || undefined;
    }

    return onSaveLauncherSettings(provider.id, settings);
  };

  const installDraftLauncher = async (options?: {
    removePreviousShortcut?: boolean;
  }) => {
    const result = options?.removePreviousShortcut
      ? await onInstallLauncher(
          provider.id,
          normalizedAlias,
          selectedPermissionMode,
          true,
        )
      : await onInstallLauncher(
          provider.id,
          normalizedAlias,
          selectedPermissionMode,
        );
    setLauncherStatus(result);
    setDraftAlias(result.info.name);
    if (result.error) {
      setError(result.error);
      throw new Error(result.error);
    }
    return result;
  };

  const handlePrimaryAction = async () => {
    if (primaryActionKind === "close") {
      onOpenChange(false);
      return;
    }
    if (primaryActionKind === "conflict") {
      return;
    }

    const installsShortcut =
      primaryActionKind === "enable-install" ||
      primaryActionKind === "install" ||
      primaryActionKind === "update";
    const removePreviousShortcut =
      installsShortcut && shouldConfirmAliasCleanup;

    maybeConfirmAliasCleanup(async () => {
      setLoading(true);
      setError(null);
      try {
        if (primaryActionKind === "disable") {
          await saveDraftSettings(false);
          await refreshLauncherStatus({ syncAlias: true });
          onOpenChange(false);
          return;
        }

        await saveDraftSettings(true, { includeAlias: !aliasDirty });

        if (installsShortcut) {
          await installDraftLauncher({ removePreviousShortcut });
        } else {
          await refreshLauncherStatus({ syncAlias: true });
        }

        onOpenChange(false);
      } catch (e: any) {
        setError(e?.message || String(e));
      } finally {
        setLoading(false);
      }
    }, removePreviousShortcut);
  };

  const handleReinstallLauncher = async () => {
    maybeConfirmBypass(async () => {
      setLoading(true);
      setError(null);
      try {
        await saveDraftSettings(true);
        await installDraftLauncher();
      } catch (e: any) {
        setError(e?.message || String(e));
      } finally {
        setLoading(false);
      }
    });
  };

  const handleRemoveLauncher = async () => {
    setLoading(true);
    setError(null);
    try {
      const result = await onRemoveLauncher(provider.id);
      setLauncherStatus(result);
      setDraftAlias(result.info.name);
      if (result.error) {
        setError(result.error);
      }
    } catch (e: any) {
      setError(e?.message || String(e));
    } finally {
      setLoading(false);
    }
  };

  const handleSyncProfile = async () => {
    maybeConfirmBypass(async () => {
      setSyncing(true);
      setError(null);
      try {
        await saveDraftSettings(draftEnabled);
        await onSyncProfile(provider.id);
        await refreshLauncherStatus({ syncAlias: true });
      } catch (e: any) {
        setError(e?.message || String(e));
      } finally {
        setSyncing(false);
      }
    });
  };

  return (
    <>
      <Dialog open={open} onOpenChange={onOpenChange}>
        <DialogContent className="sm:max-w-[560px]" zIndex="alert">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <Rocket className="h-5 w-5 text-teal-600" />
              {t("provider.launcher")} - {provider.name}
            </DialogTitle>
            <DialogDescription>
              {t("provider.launcherDeferredDescription", {
                defaultValue:
                  "Edit the launcher settings here, then apply them with the primary action.",
              })}
            </DialogDescription>
          </DialogHeader>

          <div className="space-y-4 overflow-y-auto px-6 py-4">
            <div className="rounded-md border border-border-default p-3">
              <div className="flex items-center justify-between gap-3">
                <div className="space-y-1">
                  <Label htmlFor="claude-launcher-enabled">
                    {t("provider.launcherEnabled")}
                  </Label>
                  <p className="text-xs text-muted-foreground">
                    {t("provider.launcherDeferredHint", {
                      defaultValue:
                        "Changes stay as a draft until you use the action below.",
                    })}
                  </p>
                </div>
                <Switch
                  id="claude-launcher-enabled"
                  checked={draftEnabled}
                  onCheckedChange={setDraftEnabled}
                  disabled={loading || syncing}
                />
              </div>
            </div>

            <div className="space-y-3 rounded-md border border-border-default p-3">
              <div className="flex items-center justify-between">
                <Label htmlFor="claude-launcher-alias">
                  {t("provider.launcherAlias")}
                </Label>
                <Badge
                  variant="outline"
                  className={
                    launcherBadgeClass[launcherInfo?.status || "missing"]
                  }
                >
                  {loading ? (
                    <Loader2 className="mr-1 h-3 w-3 animate-spin" />
                  ) : launcherInfo?.status === "conflict" ? (
                    <AlertCircle className="mr-1 h-3 w-3" />
                  ) : launcherInfo?.status === "installed" ? (
                    <Check className="mr-1 h-3 w-3" />
                  ) : null}
                  {launcherStatusLabel}
                </Badge>
              </div>

              <Input
                id="claude-launcher-alias"
                value={draftAlias}
                onChange={(event) => setDraftAlias(event.target.value)}
                className="text-xs font-mono"
                autoComplete="off"
                disabled={loading || syncing}
              />

              {aliasValidationMessage && (
                <p className="text-xs text-red-500">{aliasValidationMessage}</p>
              )}
              {unresolvedConflict && (
                <p className="text-xs text-red-500">
                  {t("provider.launcherAliasConflict", {
                    defaultValue:
                      "This command name is already used by another file. Choose a different alias.",
                  })}
                </p>
              )}

              <div className="space-y-2">
                <Label>{t("provider.launcherPermissionMode")}</Label>
                <Select
                  value={draftPermissionMode}
                  onValueChange={(value) =>
                    setDraftPermissionMode(value as PermissionModeInput)
                  }
                  disabled={loading || syncing}
                >
                  <SelectTrigger>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {permissionModeOptions.map((mode) => (
                      <SelectItem key={mode} value={mode}>
                        {t(`provider.launcherPermissionModes.${mode}`)}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
                {selectedModeDescription && (
                  <p className="text-xs text-muted-foreground">
                    {selectedModeDescription}
                  </p>
                )}
                {draftPermissionMode === "auto" && (
                  <p className="text-xs text-amber-600 dark:text-amber-400">
                    {t("provider.launcherPermissionAutoNote")}
                  </p>
                )}
              </div>
            </div>

            <div className="rounded-md border border-border-default">
              <button
                type="button"
                className="flex w-full items-center justify-between px-3 py-2 text-sm font-medium"
                onClick={() => setAdvancedOpen((value) => !value)}
                aria-expanded={advancedOpen}
              >
                <span>{t("provider.launcherAdvancedDetails")}</span>
                {advancedOpen ? (
                  <ChevronUp className="h-4 w-4 text-muted-foreground" />
                ) : (
                  <ChevronDown className="h-4 w-4 text-muted-foreground" />
                )}
              </button>

              {advancedOpen && (
                <div className="space-y-4 border-t border-border-default p-3">
                  {profilePath && (
                    <div className="space-y-1.5">
                      <Label>{t("provider.profilePath")}</Label>
                      <div className="flex items-center gap-2">
                        <Input
                          value={profilePath}
                          readOnly
                          className="text-xs font-mono"
                        />
                        <Button
                          size="icon"
                          variant="ghost"
                          onClick={() => handleCopy(profilePath, "path")}
                          title={t("common.copy")}
                          className="h-8 w-8 shrink-0"
                        >
                          {copied === "path" ? (
                            <Check className="h-4 w-4 text-green-500" />
                          ) : (
                            <Copy className="h-4 w-4" />
                          )}
                        </Button>
                        <Button
                          size="icon"
                          variant="ghost"
                          onClick={() => onOpenProfileDir(profilePath)}
                          title={t("common.open", { defaultValue: "Open" })}
                          className="h-8 w-8 shrink-0"
                        >
                          <FolderOpen className="h-4 w-4" />
                        </Button>
                      </div>
                    </div>
                  )}

                  {launcherInfo && (
                    <div className="space-y-1.5">
                      <Label>{t("provider.launcherScriptPath")}</Label>
                      <code className="block rounded bg-muted px-3 py-2 text-xs font-mono break-all">
                        {launcherInfo.targetPath}
                      </code>
                    </div>
                  )}

                  {launcherStatus?.launchCommand && (
                    <div className="space-y-1.5">
                      <Label>{t("provider.launcherLaunchCommand")}</Label>
                      <code className="block rounded bg-muted px-3 py-2 text-xs font-mono break-all">
                        {launcherStatus.launchCommand}
                      </code>
                    </div>
                  )}

                  {launcherStatus && !launcherStatus.pathOnPath && (
                    <div className="space-y-1">
                      <p className="text-xs text-amber-600 dark:text-amber-400">
                        {t("provider.launcherPathNotOnPath")}
                      </p>
                      {launcherStatus.pathExportSnippet && (
                        <div className="flex items-center gap-2">
                          <code className="flex-1 rounded bg-muted px-3 py-2 text-xs font-mono break-all">
                            {launcherStatus.pathExportSnippet}
                          </code>
                          <Button
                            size="icon"
                            variant="ghost"
                            onClick={() =>
                              handleCopy(
                                launcherStatus.pathExportSnippet || "",
                                "pathSnippet",
                              )
                            }
                            title={t("common.copy")}
                            className="h-8 w-8 shrink-0"
                          >
                            {copied === "pathSnippet" ? (
                              <Check className="h-4 w-4 text-green-500" />
                            ) : (
                              <Copy className="h-4 w-4" />
                            )}
                          </Button>
                        </div>
                      )}
                    </div>
                  )}

                  <div className="flex flex-wrap items-center gap-2">
                    <Button
                      size="sm"
                      variant="outline"
                      onClick={handleSyncProfile}
                      disabled={syncing || loading}
                    >
                      {syncing ? (
                        <Loader2 className="h-4 w-4 animate-spin" />
                      ) : (
                        <RefreshCw className="h-4 w-4" />
                      )}
                      {t("provider.launcherRepairSync")}
                    </Button>
                    <Button
                      size="sm"
                      variant="outline"
                      onClick={handleReinstallLauncher}
                      disabled={
                        loading || syncing || Boolean(aliasValidationMessage)
                      }
                    >
                      {loading ? (
                        <Loader2 className="h-4 w-4 animate-spin" />
                      ) : (
                        <Download className="h-4 w-4" />
                      )}
                      {t("provider.launcherRepairReinstall")}
                    </Button>
                    <Button
                      size="sm"
                      variant="destructive"
                      onClick={handleRemoveLauncher}
                      disabled={loading || syncing || !canRemoveLauncher}
                    >
                      <Trash2 className="h-4 w-4" />
                      {t("provider.launcherRemove")}
                    </Button>
                  </div>
                </div>
              )}
            </div>

            {error && <p className="text-sm text-red-500">{error}</p>}
          </div>

          <DialogFooter className="sm:justify-between">
            <div className="min-h-9">
              {enabled && draftEnabled && (
                <Button
                  variant="outline"
                  onClick={() => setDraftEnabled(false)}
                  disabled={loading || syncing}
                  className="border-red-200 text-red-600 hover:border-red-300 hover:text-red-700 dark:border-red-900 dark:text-red-300"
                >
                  {t("provider.launcherDisable")}
                </Button>
              )}
            </div>
            <div className="flex flex-col-reverse gap-2 sm:flex-row">
              <Button
                variant="outline"
                onClick={() => onOpenChange(false)}
                disabled={loading || syncing}
              >
                {t("common.cancel")}
              </Button>
              <Button
                onClick={handlePrimaryAction}
                disabled={primaryDisabled}
                variant={
                  primaryActionKind === "disable" ? "destructive" : "default"
                }
              >
                {loading || syncing ? (
                  <Loader2 className="h-4 w-4 animate-spin" />
                ) : null}
                {primaryLabel}
              </Button>
            </div>
          </DialogFooter>
        </DialogContent>
      </Dialog>
      <ConfirmDialog
        isOpen={Boolean(pendingAliasCleanupAction)}
        title={t("provider.launcherAliasChangeTitle")}
        message={t("provider.launcherAliasChangeMessage", {
          oldAlias: currentAlias,
          newAlias: normalizedAlias,
        })}
        confirmText={t("provider.launcherAliasChangeConfirm")}
        variant="destructive"
        zIndex="top"
        onConfirm={() => {
          const action = pendingAliasCleanupAction;
          setPendingAliasCleanupAction(null);
          if (action) {
            maybeConfirmBypass(action);
          }
        }}
        onCancel={() => {
          setPendingAliasCleanupAction(null);
        }}
      />
      <ConfirmDialog
        isOpen={Boolean(pendingBypassAction)}
        title={t("provider.launcherPermissionBypassTitle")}
        message={t("provider.launcherPermissionBypassMessage")}
        confirmText={t("provider.launcherPermissionBypassConfirm")}
        variant="destructive"
        zIndex="top"
        onConfirm={() => {
          const action = pendingBypassAction;
          setPendingBypassAction(null);
          void action?.();
        }}
        onCancel={() => {
          setPendingBypassAction(null);
          setDraftPermissionMode(savedPermissionMode);
        }}
      />
    </>
  );
}
