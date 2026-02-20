import { useCallback, useEffect, useRef, useState } from "react";
import type { ReactNode } from "react";
import {
  Link2,
  UploadCloud,
  DownloadCloud,
  Loader2,
  Save,
  Check,
  Info,
  AlertTriangle,
} from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { settingsApi } from "@/lib/api";
import type { RemoteSnapshotInfo, WebDavSyncSettings } from "@/types";

// ─── WebDAV service presets ─────────────────────────────────

interface WebDavPreset {
  id: string;
  label: string;
  baseUrl: string;
  hint: string;
  matchPattern?: string; // substring match on URL
}

const WEBDAV_PRESETS: WebDavPreset[] = [
  {
    id: "jianguoyun",
    label: "settings.webdavSync.presets.jianguoyun",
    baseUrl: "https://dav.jianguoyun.com/dav/",
    hint: "settings.webdavSync.presets.jianguoyunHint",
    matchPattern: "jianguoyun.com",
  },
  {
    id: "nextcloud",
    label: "settings.webdavSync.presets.nextcloud",
    baseUrl: "https://your-server/remote.php/dav/files/USERNAME/",
    hint: "settings.webdavSync.presets.nextcloudHint",
    matchPattern: "remote.php/dav",
  },
  {
    id: "synology",
    label: "settings.webdavSync.presets.synology",
    baseUrl: "http://your-nas-ip:5005/",
    hint: "settings.webdavSync.presets.synologyHint",
    matchPattern: ":5005",
  },
  {
    id: "custom",
    label: "settings.webdavSync.presets.custom",
    baseUrl: "",
    hint: "",
  },
];

/** Match a URL to one of the preset providers, or "custom". */
function detectPreset(url: string): string {
  if (!url) return "custom";
  for (const preset of WEBDAV_PRESETS) {
    if (preset.matchPattern && url.includes(preset.matchPattern)) {
      return preset.id;
    }
  }
  return "custom";
}

/** Format an RFC 3339 date string for display; falls back to raw string. */
function formatDate(rfc3339: string): string {
  const d = new Date(rfc3339);
  return Number.isNaN(d.getTime()) ? rfc3339 : d.toLocaleString();
}

// ─── Types ──────────────────────────────────────────────────

type ActionState =
  | "idle"
  | "testing"
  | "saving"
  | "uploading"
  | "downloading"
  | "fetching_remote";

type DialogType = "upload" | "download" | null;

interface WebdavSyncSectionProps {
  config?: WebDavSyncSettings;
}

// ─── ActionButton ───────────────────────────────────────────

/** Reusable button with loading spinner. */
function ActionButton({
  actionState,
  targetState,
  alsoActiveFor,
  icon: Icon,
  activeLabel,
  idleLabel,
  disabled,
  ...props
}: {
  actionState: ActionState;
  targetState: ActionState;
  alsoActiveFor?: ActionState[];
  icon: LucideIcon;
  activeLabel: ReactNode;
  idleLabel: ReactNode;
} & Omit<React.ComponentPropsWithoutRef<typeof Button>, "children">) {
  const isActive =
    actionState === targetState ||
    (alsoActiveFor?.includes(actionState) ?? false);
  return (
    <Button {...props} disabled={actionState !== "idle" || disabled}>
      <span className="inline-flex items-center gap-2">
        {isActive ? (
          <Loader2 className="h-3.5 w-3.5 animate-spin" />
        ) : (
          <Icon className="h-3.5 w-3.5" />
        )}
        {isActive ? activeLabel : idleLabel}
      </span>
    </Button>
  );
}

// ─── Main component ─────────────────────────────────────────

export function WebdavSyncSection({ config }: WebdavSyncSectionProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [actionState, setActionState] = useState<ActionState>("idle");
  const [dirty, setDirty] = useState(false);
  const [passwordTouched, setPasswordTouched] = useState(false);
  const [justSaved, setJustSaved] = useState(false);
  const justSavedTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Local form state — credentials are only persisted on explicit "Save".
  const [form, setForm] = useState(() => ({
    baseUrl: config?.baseUrl ?? "",
    username: config?.username ?? "",
    password: config?.password ?? "",
    remoteRoot: config?.remoteRoot ?? "cc-switch-sync",
    profile: config?.profile ?? "default",
    autoSync: config?.autoSync ?? false,
  }));

  // Preset selector — derived from initial URL, updated on user selection
  const [presetId, setPresetId] = useState(() =>
    detectPreset(config?.baseUrl ?? ""),
  );

  const activePreset = WEBDAV_PRESETS.find((p) => p.id === presetId);

  // Confirmation dialog state
  const [dialogType, setDialogType] = useState<DialogType>(null);
  const [remoteInfo, setRemoteInfo] = useState<RemoteSnapshotInfo | null>(null);

  const closeDialog = useCallback(() => {
    setDialogType(null);
    setRemoteInfo(null);
  }, []);

  // Cleanup justSaved timer on unmount
  useEffect(() => {
    return () => {
      if (justSavedTimerRef.current) clearTimeout(justSavedTimerRef.current);
    };
  }, []);

  // Sync form when config is loaded/updated from backend, but not while user is editing
  useEffect(() => {
    if (!config || dirty) return;
    setForm({
      baseUrl: config.baseUrl ?? "",
      username: config.username ?? "",
      password: config.password ?? "",
      remoteRoot: config.remoteRoot ?? "cc-switch-sync",
      profile: config.profile ?? "default",
      autoSync: config.autoSync ?? false,
    });
    setPasswordTouched(false);
    setPresetId(detectPreset(config.baseUrl ?? ""));
  }, [config, dirty]);

  const updateField = useCallback((field: keyof typeof form, value: string) => {
    setForm((prev) => ({ ...prev, [field]: value }));
    if (field === "password") {
      setPasswordTouched(true);
    }
    setDirty(true);
    setJustSaved(false);
    if (justSavedTimerRef.current) {
      clearTimeout(justSavedTimerRef.current);
      justSavedTimerRef.current = null;
    }
  }, []);

  const handlePresetChange = useCallback((id: string) => {
    setPresetId(id);
    const preset = WEBDAV_PRESETS.find((p) => p.id === id);
    if (preset?.baseUrl) {
      setForm((prev) => ({ ...prev, baseUrl: preset.baseUrl }));
      setDirty(true);
      setJustSaved(false);
      if (justSavedTimerRef.current) {
        clearTimeout(justSavedTimerRef.current);
        justSavedTimerRef.current = null;
      }
    }
  }, []);

  // When user edits the URL, check if it still matches the current preset on blur
  const handleBaseUrlBlur = useCallback(() => {
    if (presetId === "custom") return;
    const detected = detectPreset(form.baseUrl);
    if (detected !== presetId) {
      setPresetId("custom");
    }
  }, [form.baseUrl, presetId]);

  const handleAutoSyncChange = useCallback((checked: boolean) => {
    setForm((prev) => ({ ...prev, autoSync: checked }));
    setDirty(true);
    setJustSaved(false);
    if (justSavedTimerRef.current) {
      clearTimeout(justSavedTimerRef.current);
      justSavedTimerRef.current = null;
    }
  }, []);

  const buildSettings = useCallback((): WebDavSyncSettings | null => {
    const baseUrl = form.baseUrl.trim();
    if (!baseUrl) return null;
    return {
      enabled: true,
      baseUrl,
      username: form.username.trim(),
      password: form.password,
      remoteRoot: form.remoteRoot.trim() || "cc-switch-sync",
      profile: form.profile.trim() || "default",
      autoSync: form.autoSync,
    };
  }, [form]);

  // ─── Handlers ───────────────────────────────────────────

  const handleTest = useCallback(async () => {
    const settings = buildSettings();
    if (!settings) {
      toast.error(t("settings.webdavSync.missingUrl"));
      return;
    }
    setActionState("testing");
    try {
      await settingsApi.webdavTestConnection(settings, !passwordTouched);
      toast.success(t("settings.webdavSync.testSuccess"));
    } catch (error) {
      toast.error(
        t("settings.webdavSync.testFailed", {
          error: (error as Error)?.message ?? String(error),
        }),
      );
    } finally {
      setActionState("idle");
    }
  }, [buildSettings, passwordTouched, t]);

  const handleSave = useCallback(async () => {
    const settings = buildSettings();
    if (!settings) {
      toast.error(t("settings.webdavSync.missingUrl"));
      return;
    }
    setActionState("saving");
    try {
      await settingsApi.webdavSyncSaveSettings(settings, passwordTouched);
      setDirty(false);
      setPasswordTouched(false);
      // Show "saved" indicator for 2 seconds
      setJustSaved(true);
      if (justSavedTimerRef.current) clearTimeout(justSavedTimerRef.current);
      justSavedTimerRef.current = setTimeout(() => {
        setJustSaved(false);
        justSavedTimerRef.current = null;
      }, 2000);
      await queryClient.invalidateQueries();
    } catch (error) {
      toast.error(
        t("settings.webdavSync.saveFailed", {
          error: (error as Error)?.message ?? String(error),
        }),
      );
      setActionState("idle");
      return;
    }

    // Auto-test connection after save
    setActionState("testing");
    try {
      await settingsApi.webdavTestConnection(settings, true);
      toast.success(t("settings.webdavSync.saveAndTestSuccess"));
    } catch (error) {
      toast.warning(
        t("settings.webdavSync.saveAndTestFailed", {
          error: (error as Error)?.message ?? String(error),
        }),
      );
    } finally {
      setActionState("idle");
    }
  }, [buildSettings, passwordTouched, queryClient, t]);

  /** Fetch remote info, then open upload confirmation dialog. */
  const handleUploadClick = useCallback(async () => {
    if (dirty) {
      toast.error(t("settings.webdavSync.unsavedChanges"));
      return;
    }
    setActionState("fetching_remote");
    try {
      const info = await settingsApi.webdavSyncFetchRemoteInfo();
      if ("empty" in info) {
        setRemoteInfo(null);
      } else {
        setRemoteInfo(info);
      }
      setDialogType("upload");
    } catch {
      setRemoteInfo(null);
      toast.error(t("settings.webdavSync.fetchRemoteFailed"));
      setActionState("idle");
      return;
    }
    setActionState("idle");
  }, [dirty, t]);

  /** Actually perform the upload after user confirms. */
  const handleUploadConfirm = useCallback(async () => {
    if (dirty) {
      toast.error(t("settings.webdavSync.unsavedChanges"));
      return;
    }
    closeDialog();
    setActionState("uploading");
    try {
      await settingsApi.webdavSyncUpload();
      toast.success(t("settings.webdavSync.uploadSuccess"));
      await queryClient.invalidateQueries();
    } catch (error) {
      toast.error(
        t("settings.webdavSync.uploadFailed", {
          error: (error as Error)?.message ?? String(error),
        }),
      );
    } finally {
      setActionState("idle");
    }
  }, [closeDialog, dirty, queryClient, t]);

  /** Fetch remote info, then open download confirmation dialog. */
  const handleDownloadClick = useCallback(async () => {
    if (dirty) {
      toast.error(t("settings.webdavSync.unsavedChanges"));
      return;
    }
    setActionState("fetching_remote");
    try {
      const info = await settingsApi.webdavSyncFetchRemoteInfo();
      if ("empty" in info) {
        toast.info(t("settings.webdavSync.noRemoteData"));
        return;
      }
      if (!info.compatible) {
        toast.error(
          t("settings.webdavSync.incompatibleVersion", {
            version: info.version,
          }),
        );
        return;
      }
      setRemoteInfo(info);
      setDialogType("download");
    } catch (error) {
      toast.error(
        t("settings.webdavSync.downloadFailed", {
          error: (error as Error)?.message ?? String(error),
        }),
      );
    } finally {
      setActionState("idle");
    }
  }, [dirty, t]);

  /** Actually perform the download after user confirms. */
  const handleDownloadConfirm = useCallback(async () => {
    if (dirty) {
      toast.error(t("settings.webdavSync.unsavedChanges"));
      return;
    }
    closeDialog();
    setActionState("downloading");
    try {
      await settingsApi.webdavSyncDownload();
      toast.success(t("settings.webdavSync.downloadSuccess"));
      await queryClient.invalidateQueries();
    } catch (error) {
      toast.error(
        t("settings.webdavSync.downloadFailed", {
          error: (error as Error)?.message ?? String(error),
        }),
      );
    } finally {
      setActionState("idle");
    }
  }, [closeDialog, dirty, queryClient, t]);

  // ─── Derived state ──────────────────────────────────────

  const isLoading = actionState !== "idle";
  const hasSavedConfig = Boolean(
    config?.baseUrl?.trim() && config?.username?.trim(),
  );

  const lastSyncAt = config?.status?.lastSyncAt;
  const lastSyncDisplay = lastSyncAt
    ? new Date(lastSyncAt * 1000).toLocaleString()
    : null;
  const lastError = config?.status?.lastError?.trim();
  const showAutoSyncError =
    !!lastError && config?.status?.lastErrorSource === "auto";

  // ─── Render ─────────────────────────────────────────────

  return (
    <section className="space-y-4">
      <header className="space-y-2">
        <h3 className="text-base font-semibold text-foreground">
          {t("settings.webdavSync.title")}
        </h3>
        <p className="text-sm text-muted-foreground">
          {t("settings.webdavSync.description")}
        </p>
      </header>

      <div className="space-y-4 rounded-lg border border-border bg-muted/40 p-6">
        {/* Config fields */}
        <div className="space-y-3">
          {/* Service preset selector */}
          <div className="flex items-center gap-4">
            <label className="w-40 text-xs font-medium text-foreground shrink-0">
              {t("settings.webdavSync.presets.label")}
            </label>
            <Select
              value={presetId}
              onValueChange={handlePresetChange}
              disabled={isLoading}
            >
              <SelectTrigger className="text-xs flex-1">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {WEBDAV_PRESETS.map((preset) => (
                  <SelectItem key={preset.id} value={preset.id}>
                    {t(preset.label)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          {/* Server URL */}
          <div className="flex items-center gap-4">
            <label className="w-40 text-xs font-medium text-foreground shrink-0">
              {t("settings.webdavSync.baseUrl")}
            </label>
            <Input
              value={form.baseUrl}
              onChange={(e) => updateField("baseUrl", e.target.value)}
              onBlur={handleBaseUrlBlur}
              placeholder={t("settings.webdavSync.baseUrlPlaceholder")}
              className="text-xs flex-1"
              disabled={isLoading}
            />
          </div>

          {/* Username */}
          <div className="flex items-center gap-4">
            <label className="w-40 text-xs font-medium text-foreground shrink-0">
              {t("settings.webdavSync.username")}
            </label>
            <Input
              value={form.username}
              onChange={(e) => updateField("username", e.target.value)}
              placeholder={t("settings.webdavSync.usernamePlaceholder")}
              className="text-xs flex-1"
              disabled={isLoading}
            />
          </div>

          {/* Password */}
          <div className="flex items-center gap-4">
            <label className="w-40 text-xs font-medium text-foreground shrink-0">
              {t("settings.webdavSync.password")}
            </label>
            <Input
              type="password"
              value={form.password}
              onChange={(e) => updateField("password", e.target.value)}
              placeholder={t("settings.webdavSync.passwordPlaceholder")}
              className="text-xs flex-1"
              autoComplete="off"
              disabled={isLoading}
            />
          </div>

          {/* Preset hint */}
          {activePreset?.hint && (
            <div className="flex items-start gap-2 pl-44 text-xs text-muted-foreground">
              <Info className="h-3.5 w-3.5 shrink-0 mt-0.5" />
              <span>{t(activePreset.hint)}</span>
            </div>
          )}

          {/* Remote Root */}
          <div className="flex items-center gap-4">
            <label className="w-40 text-xs font-medium text-foreground shrink-0">
              {t("settings.webdavSync.remoteRoot")}
              <span className="block text-[10px] font-normal text-muted-foreground">
                {t("settings.webdavSync.remoteRootDefault")}
              </span>
            </label>
            <Input
              value={form.remoteRoot}
              onChange={(e) => updateField("remoteRoot", e.target.value)}
              placeholder="cc-switch-sync"
              className="text-xs flex-1"
              disabled={isLoading}
            />
          </div>

          {/* Profile */}
          <div className="flex items-center gap-4">
            <label className="w-40 text-xs font-medium text-foreground shrink-0">
              {t("settings.webdavSync.profile")}
              <span className="block text-[10px] font-normal text-muted-foreground">
                {t("settings.webdavSync.profileDefault")}
              </span>
            </label>
            <Input
              value={form.profile}
              onChange={(e) => updateField("profile", e.target.value)}
              placeholder="default"
              className="text-xs flex-1"
              disabled={isLoading}
            />
          </div>

          <div className="flex items-start gap-4">
            <label className="w-40 text-xs font-medium text-foreground shrink-0">
              {t("settings.webdavSync.autoSync")}
              <span className="block text-[10px] font-normal text-muted-foreground">
                {t("settings.webdavSync.autoSyncHint")}
              </span>
            </label>
            <div className="pt-1">
              <Switch
                checked={form.autoSync}
                onCheckedChange={handleAutoSyncChange}
                aria-label={t("settings.webdavSync.autoSync")}
                disabled={isLoading}
              />
            </div>
          </div>
        </div>

        {/* Last sync time */}
        {lastSyncDisplay && (
          <p className="text-xs text-muted-foreground">
            {t("settings.webdavSync.lastSync", { time: lastSyncDisplay })}
          </p>
        )}
        {showAutoSyncError && (
          <div className="rounded-lg border border-red-300/70 bg-red-50/80 px-3 py-2 text-xs text-red-900 dark:border-red-500/50 dark:bg-red-950/30 dark:text-red-200">
            <p className="font-medium">
              {t("settings.webdavSync.autoSyncLastErrorTitle")}
            </p>
            <p className="mt-1 break-all whitespace-pre-wrap">{lastError}</p>
            <p className="mt-1 text-[11px] text-red-700/90 dark:text-red-300/80">
              {t("settings.webdavSync.autoSyncLastErrorHint")}
            </p>
          </div>
        )}

        {/* Config buttons + save status */}
        <div className="flex flex-wrap items-center gap-3 pt-2">
          <ActionButton
            type="button"
            variant="outline"
            size="sm"
            onClick={handleTest}
            actionState={actionState}
            targetState="testing"
            icon={Link2}
            activeLabel={t("settings.webdavSync.testing")}
            idleLabel={t("settings.webdavSync.test")}
          />
          <ActionButton
            type="button"
            variant="outline"
            size="sm"
            onClick={handleSave}
            actionState={actionState}
            targetState="saving"
            icon={Save}
            activeLabel={t("settings.webdavSync.saving")}
            idleLabel={t("settings.webdavSync.save")}
          />

          {/* Save status indicator */}
          {dirty && (
            <span className="inline-flex items-center gap-1.5 text-xs text-amber-500 dark:text-amber-400 animate-in fade-in duration-200">
              <span className="h-1.5 w-1.5 rounded-full bg-amber-500 dark:bg-amber-400" />
              {t("settings.webdavSync.unsaved")}
            </span>
          )}
          {!dirty && justSaved && (
            <span className="inline-flex items-center gap-1.5 text-xs text-emerald-600 dark:text-emerald-400 animate-in fade-in duration-200">
              <Check className="h-3 w-3" />
              {t("settings.webdavSync.saved")}
            </span>
          )}
        </div>

        {/* Sync buttons */}
        <div className="flex flex-wrap items-center gap-3 border-t border-border pt-4">
          <ActionButton
            type="button"
            size="sm"
            onClick={handleUploadClick}
            disabled={!hasSavedConfig}
            actionState={actionState}
            targetState="uploading"
            alsoActiveFor={["fetching_remote"]}
            icon={UploadCloud}
            activeLabel={
              actionState === "fetching_remote"
                ? t("settings.webdavSync.fetchingRemote")
                : t("settings.webdavSync.uploading")
            }
            idleLabel={t("settings.webdavSync.upload")}
          />
          <ActionButton
            type="button"
            variant="secondary"
            size="sm"
            onClick={handleDownloadClick}
            disabled={!hasSavedConfig}
            actionState={actionState}
            targetState="downloading"
            alsoActiveFor={["fetching_remote"]}
            icon={DownloadCloud}
            activeLabel={
              actionState === "fetching_remote"
                ? t("settings.webdavSync.fetchingRemote")
                : t("settings.webdavSync.downloading")
            }
            idleLabel={t("settings.webdavSync.download")}
          />
        </div>
        {!hasSavedConfig && (
          <p className="text-xs text-muted-foreground">
            {t("settings.webdavSync.saveBeforeSync")}
          </p>
        )}
      </div>

      {/* ─── Upload confirmation dialog ──────────────────── */}
      <Dialog
        open={dialogType === "upload"}
        onOpenChange={(open) => {
          if (!open) closeDialog();
        }}
      >
        <DialogContent className="max-w-sm" zIndex="alert">
          <DialogHeader className="space-y-3 border-b-0 bg-transparent pb-0">
            <DialogTitle className="flex items-center gap-2 text-lg font-semibold">
              <AlertTriangle className="h-5 w-5 text-destructive" />
              {t("settings.webdavSync.confirmUpload.title")}
            </DialogTitle>
            <DialogDescription asChild>
              <div className="space-y-3 text-sm leading-relaxed">
                <p>{t("settings.webdavSync.confirmUpload.content")}</p>
                <ul className="list-disc pl-5 space-y-1 text-muted-foreground">
                  <li>{t("settings.webdavSync.confirmUpload.dbItem")}</li>
                  <li>{t("settings.webdavSync.confirmUpload.skillsItem")}</li>
                </ul>
                <p className="text-muted-foreground">
                  {t("settings.webdavSync.confirmUpload.targetPath")}
                  {": "}
                  <code className="ml-1 text-xs bg-muted px-1.5 py-0.5 rounded">
                    /{form.remoteRoot.trim() || "cc-switch-sync"}/v2/
                    {form.profile.trim() || "default"}
                  </code>
                </p>
                {remoteInfo && (
                  <div className="rounded-lg border border-border bg-muted/50 p-3 space-y-2">
                    <p className="text-xs font-medium text-foreground">
                      {t("settings.webdavSync.confirmUpload.existingData")}
                    </p>
                    <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1.5 text-xs text-muted-foreground">
                      <dt className="font-medium text-foreground">
                        {t("settings.webdavSync.confirmUpload.deviceName")}
                      </dt>
                      <dd>
                        <code className="bg-muted px-1.5 py-0.5 rounded">
                          {remoteInfo.deviceName}
                        </code>
                      </dd>
                      <dt className="font-medium text-foreground">
                        {t("settings.webdavSync.confirmUpload.createdAt")}
                      </dt>
                      <dd>{formatDate(remoteInfo.createdAt)}</dd>
                    </dl>
                  </div>
                )}
                {remoteInfo && (
                  <p className="text-destructive font-medium">
                    {t("settings.webdavSync.confirmUpload.warning")}
                  </p>
                )}
              </div>
            </DialogDescription>
          </DialogHeader>
          <DialogFooter className="flex gap-2 border-t-0 bg-transparent pt-2 sm:justify-end">
            <Button variant="outline" onClick={closeDialog}>
              {t("common.cancel")}
            </Button>
            <Button variant="destructive" onClick={handleUploadConfirm}>
              {t("settings.webdavSync.confirmUpload.confirm")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* ─── Download confirmation dialog ────────────────── */}
      <Dialog
        open={dialogType === "download"}
        onOpenChange={(open) => {
          if (!open) closeDialog();
        }}
      >
        <DialogContent className="max-w-sm" zIndex="alert">
          <DialogHeader className="space-y-3 border-b-0 bg-transparent pb-0">
            <DialogTitle className="flex items-center gap-2 text-lg font-semibold">
              <AlertTriangle className="h-5 w-5 text-destructive" />
              {t("settings.webdavSync.confirmDownload.title")}
            </DialogTitle>
            <DialogDescription asChild>
              <div className="space-y-3 text-sm leading-relaxed">
                {remoteInfo && (
                  <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1.5 text-muted-foreground">
                    <dt className="font-medium text-foreground">
                      {t("settings.webdavSync.confirmDownload.deviceName")}
                    </dt>
                    <dd>
                      <code className="text-xs bg-muted px-1.5 py-0.5 rounded">
                        {remoteInfo.deviceName}
                      </code>
                    </dd>
                    <dt className="font-medium text-foreground">
                      {t("settings.webdavSync.confirmDownload.createdAt")}
                    </dt>
                    <dd>{formatDate(remoteInfo.createdAt)}</dd>
                    <dt className="font-medium text-foreground">
                      {t("settings.webdavSync.confirmDownload.artifacts")}
                    </dt>
                    <dd>{remoteInfo.artifacts.join(", ")}</dd>
                  </dl>
                )}
                <p className="text-destructive font-medium">
                  {t("settings.webdavSync.confirmDownload.warning")}
                </p>
              </div>
            </DialogDescription>
          </DialogHeader>
          <DialogFooter className="flex gap-2 border-t-0 bg-transparent pt-2 sm:justify-end">
            <Button variant="outline" onClick={closeDialog}>
              {t("common.cancel")}
            </Button>
            <Button variant="destructive" onClick={handleDownloadConfirm}>
              {t("settings.webdavSync.confirmDownload.confirm")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </section>
  );
}
