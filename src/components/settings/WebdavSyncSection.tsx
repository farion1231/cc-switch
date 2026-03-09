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
import type { RemoteSnapshotInfo, WebDavSyncSettings, S3SyncSettings } from "@/types";

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

// ─── S3 service presets ──────────────────────────────────────

interface S3Preset {
  id: string;
  label: string;
  hint: string;
  defaultEndpoint?: string;
  regionPlaceholder?: string;
}

const S3_PRESETS: S3Preset[] = [
  { id: "aws-s3", label: "settings.s3Sync.presets.awsS3", hint: "settings.s3Sync.presets.awsS3Hint", regionPlaceholder: "us-east-1" },
  { id: "s3-minio", label: "settings.s3Sync.presets.minio", hint: "settings.s3Sync.presets.minioHint", regionPlaceholder: "us-east-1" },
  { id: "s3-r2", label: "settings.s3Sync.presets.r2", hint: "settings.s3Sync.presets.r2Hint", regionPlaceholder: "auto" },
  { id: "s3-oss", label: "settings.s3Sync.presets.oss", hint: "settings.s3Sync.presets.ossHint", regionPlaceholder: "cn-hangzhou" },
  { id: "s3-cos", label: "settings.s3Sync.presets.cos", hint: "settings.s3Sync.presets.cosHint", regionPlaceholder: "ap-guangzhou" },
  { id: "s3-obs", label: "settings.s3Sync.presets.obs", hint: "settings.s3Sync.presets.obsHint", regionPlaceholder: "cn-north-4" },
  { id: "s3-custom", label: "settings.s3Sync.presets.custom", hint: "settings.s3Sync.presets.customHint", regionPlaceholder: "us-east-1" },
];

function isS3Preset(presetId: string): boolean {
  return presetId.startsWith("s3-") || presetId === "aws-s3";
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

type SyncType = "webdav" | "s3";

type DialogType = "upload" | "download" | "mutual_exclusion" | null;

interface WebdavSyncSectionProps {
  config?: WebDavSyncSettings;
  s3Config?: S3SyncSettings;
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

export function WebdavSyncSection({ config, s3Config }: WebdavSyncSectionProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [actionState, setActionState] = useState<ActionState>("idle");
  const [dirty, setDirty] = useState(false);
  const [passwordTouched, setPasswordTouched] = useState(false);
  const [justSaved, setJustSaved] = useState(false);
  const justSavedTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // ─── Sync type selector ────────────────────────────────────
  const [syncType, setSyncType] = useState<SyncType>(() =>
    s3Config?.enabled ? "s3" : "webdav",
  );
  const [pendingSyncType, setPendingSyncType] = useState<SyncType | null>(null);

  // Sync the selector when settings load asynchronously
  useEffect(() => {
    if (s3Config?.enabled) {
      setSyncType("s3");
    }
  }, [s3Config?.enabled]);

  // Local form state — credentials are only persisted on explicit "Save".
  const [form, setForm] = useState(() => ({
    baseUrl: config?.baseUrl ?? "",
    username: config?.username ?? "",
    password: config?.password ?? "",
    remoteRoot: config?.remoteRoot ?? "cc-switch-sync",
    profile: config?.profile ?? "default",
    autoSync: config?.autoSync ?? false,
  }));

  // ─── S3 form state ─────────────────────────────────────────
  const [s3Preset, setS3Preset] = useState("aws-s3");
  const [s3Region, setS3Region] = useState(s3Config?.region ?? "");
  const [s3Bucket, setS3Bucket] = useState(s3Config?.bucket ?? "");
  const [s3AccessKeyId, setS3AccessKeyId] = useState(s3Config?.accessKeyId ?? "");
  const [s3SecretAccessKey, setS3SecretAccessKey] = useState(s3Config?.secretAccessKey ?? "");
  const [s3Endpoint, setS3Endpoint] = useState(s3Config?.endpoint ?? "");
  const [s3RemoteRoot, setS3RemoteRoot] = useState(s3Config?.remoteRoot ?? "cc-switch-sync");
  const [s3Profile, setS3Profile] = useState(s3Config?.profile ?? "default");
  const [s3AutoSync, setS3AutoSync] = useState(s3Config?.autoSync ?? false);
  const [s3Enabled, setS3Enabled] = useState(s3Config?.enabled ?? false);
  const [s3SecretTouched, setS3SecretTouched] = useState(false);
  const [s3Dirty, setS3Dirty] = useState(false);
  const [s3JustSaved, setS3JustSaved] = useState(false);
  const s3JustSavedTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [s3ActionState, setS3ActionState] = useState<ActionState>("idle");
  const [s3DialogType, setS3DialogType] = useState<DialogType>(null);
  const [s3RemoteInfo, setS3RemoteInfo] = useState<RemoteSnapshotInfo | null>(null);

  const activeS3Preset = S3_PRESETS.find((p) => p.id === s3Preset);

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

  const closeS3Dialog = useCallback(() => {
    setS3DialogType(null);
    setS3RemoteInfo(null);
  }, []);

  // Cleanup justSaved timer on unmount
  useEffect(() => {
    return () => {
      if (justSavedTimerRef.current) clearTimeout(justSavedTimerRef.current);
      if (s3JustSavedTimerRef.current) clearTimeout(s3JustSavedTimerRef.current);
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

  // Sync S3 form when s3Config is loaded/updated from backend
  useEffect(() => {
    if (!s3Config || s3Dirty) return;
    setS3Region(s3Config.region ?? "");
    setS3Bucket(s3Config.bucket ?? "");
    setS3AccessKeyId(s3Config.accessKeyId ?? "");
    setS3SecretAccessKey(s3Config.secretAccessKey ?? "");
    setS3Endpoint(s3Config.endpoint ?? "");
    setS3RemoteRoot(s3Config.remoteRoot ?? "cc-switch-sync");
    setS3Profile(s3Config.profile ?? "default");
    setS3AutoSync(s3Config.autoSync ?? false);
    setS3Enabled(s3Config.enabled ?? false);
    setS3SecretTouched(false);
  }, [s3Config, s3Dirty]);

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

  // ─── S3 helpers ────────────────────────────────────────────

  const markS3Dirty = useCallback(() => {
    setS3Dirty(true);
    setS3JustSaved(false);
    if (s3JustSavedTimerRef.current) {
      clearTimeout(s3JustSavedTimerRef.current);
      s3JustSavedTimerRef.current = null;
    }
  }, []);

  const handleS3PresetChange = useCallback((id: string) => {
    setS3Preset(id);
    markS3Dirty();
  }, [markS3Dirty]);

  const buildS3Settings = useCallback((): S3SyncSettings => {
    return {
      enabled: s3Enabled,
      autoSync: s3AutoSync,
      region: s3Region.trim(),
      bucket: s3Bucket.trim(),
      accessKeyId: s3AccessKeyId.trim(),
      secretAccessKey: s3SecretAccessKey,
      endpoint: s3Endpoint.trim() || undefined,
      remoteRoot: s3RemoteRoot.trim() || "cc-switch-sync",
      profile: s3Profile.trim() || "default",
    };
  }, [s3Enabled, s3AutoSync, s3Region, s3Bucket, s3AccessKeyId, s3SecretAccessKey, s3Endpoint, s3RemoteRoot, s3Profile]);

  // ─── S3 Handlers ──────────────────────────────────────────

  const handleS3Test = useCallback(async () => {
    const s3Settings = buildS3Settings();
    if (!s3Settings.bucket) {
      toast.error(t("settings.s3Sync.missingBucket"));
      return;
    }
    setS3ActionState("testing");
    try {
      await settingsApi.s3TestConnection(s3Settings, !s3SecretTouched);
      toast.success(t("settings.s3Sync.testSuccess"));
    } catch (error) {
      toast.error(
        t("settings.s3Sync.testFailed", {
          error: (error as Error)?.message ?? String(error),
        }),
      );
    } finally {
      setS3ActionState("idle");
    }
  }, [buildS3Settings, s3SecretTouched, t]);

  const handleS3Save = useCallback(async () => {
    const s3Settings = buildS3Settings();
    if (!s3Settings.bucket) {
      toast.error(t("settings.s3Sync.missingBucket"));
      return;
    }
    setS3ActionState("saving");
    try {
      await settingsApi.s3SyncSaveSettings(s3Settings, s3SecretTouched);
      setS3Dirty(false);
      setS3SecretTouched(false);
      setS3JustSaved(true);
      if (s3JustSavedTimerRef.current) clearTimeout(s3JustSavedTimerRef.current);
      s3JustSavedTimerRef.current = setTimeout(() => {
        setS3JustSaved(false);
        s3JustSavedTimerRef.current = null;
      }, 2000);
      await queryClient.invalidateQueries();
    } catch (error) {
      toast.error(
        t("settings.s3Sync.saveFailed", {
          error: (error as Error)?.message ?? String(error),
        }),
      );
      setS3ActionState("idle");
      return;
    }

    // Auto-test connection after save
    setS3ActionState("testing");
    try {
      await settingsApi.s3TestConnection(s3Settings, true);
      toast.success(t("settings.s3Sync.saveAndTestSuccess"));
    } catch (error) {
      toast.warning(
        t("settings.s3Sync.saveAndTestFailed", {
          error: (error as Error)?.message ?? String(error),
        }),
      );
    } finally {
      setS3ActionState("idle");
    }
  }, [buildS3Settings, s3SecretTouched, queryClient, t]);

  const handleS3UploadClick = useCallback(async () => {
    if (s3Dirty) {
      toast.error(t("settings.s3Sync.unsavedChanges"));
      return;
    }
    setS3ActionState("fetching_remote");
    try {
      const info = await settingsApi.s3SyncFetchRemoteInfo();
      if ("empty" in info) {
        setS3RemoteInfo(null);
      } else {
        setS3RemoteInfo(info);
      }
      setS3DialogType("upload");
    } catch {
      setS3RemoteInfo(null);
      toast.error(t("settings.s3Sync.fetchRemoteFailed"));
      setS3ActionState("idle");
      return;
    }
    setS3ActionState("idle");
  }, [s3Dirty, t]);

  const handleS3UploadConfirm = useCallback(async () => {
    if (s3Dirty) {
      toast.error(t("settings.s3Sync.unsavedChanges"));
      return;
    }
    closeS3Dialog();
    setS3ActionState("uploading");
    try {
      await settingsApi.s3SyncUpload();
      toast.success(t("settings.s3Sync.uploadSuccess"));
      await queryClient.invalidateQueries();
    } catch (error) {
      toast.error(
        t("settings.s3Sync.uploadFailed", {
          error: (error as Error)?.message ?? String(error),
        }),
      );
    } finally {
      setS3ActionState("idle");
    }
  }, [closeS3Dialog, s3Dirty, queryClient, t]);

  const handleS3DownloadClick = useCallback(async () => {
    if (s3Dirty) {
      toast.error(t("settings.s3Sync.unsavedChanges"));
      return;
    }
    setS3ActionState("fetching_remote");
    try {
      const info = await settingsApi.s3SyncFetchRemoteInfo();
      if ("empty" in info) {
        toast.info(t("settings.s3Sync.noRemoteData"));
        return;
      }
      if (!info.compatible) {
        toast.error(
          t("settings.s3Sync.incompatibleVersion", {
            version: info.version,
          }),
        );
        return;
      }
      setS3RemoteInfo(info);
      setS3DialogType("download");
    } catch (error) {
      toast.error(
        t("settings.s3Sync.downloadFailed", {
          error: (error as Error)?.message ?? String(error),
        }),
      );
    } finally {
      setS3ActionState("idle");
    }
  }, [s3Dirty, t]);

  const handleS3DownloadConfirm = useCallback(async () => {
    if (s3Dirty) {
      toast.error(t("settings.s3Sync.unsavedChanges"));
      return;
    }
    closeS3Dialog();
    setS3ActionState("downloading");
    try {
      await settingsApi.s3SyncDownload();
      toast.success(t("settings.s3Sync.downloadSuccess"));
      await queryClient.invalidateQueries();
    } catch (error) {
      toast.error(
        t("settings.s3Sync.downloadFailed", {
          error: (error as Error)?.message ?? String(error),
        }),
      );
    } finally {
      setS3ActionState("idle");
    }
  }, [closeS3Dialog, s3Dirty, queryClient, t]);

  // ─── Sync type switching with mutual exclusion ─────────────

  const handleSyncTypeChange = useCallback((value: string) => {
    const next = value as SyncType;
    if (next === syncType) return;

    // Check if the "other" type is currently enabled
    const otherEnabled =
      next === "s3" ? (config?.enabled !== false && Boolean(config?.baseUrl?.trim()))
                    : (s3Config?.enabled === true);

    if (otherEnabled) {
      setPendingSyncType(next);
      setDialogType("mutual_exclusion");
    } else {
      setSyncType(next);
    }
  }, [syncType, config, s3Config]);

  const handleMutualExclusionConfirm = useCallback(async () => {
    if (!pendingSyncType) return;
    closeDialog();
    try {
      if (pendingSyncType === "s3") {
        // Disable WebDAV
        const disabledWebdav: WebDavSyncSettings = {
          ...config,
          enabled: false,
          autoSync: false,
        };
        await settingsApi.webdavSyncSaveSettings(disabledWebdav, false);
      } else {
        // Disable S3
        const disabledS3: S3SyncSettings = {
          ...s3Config,
          enabled: false,
          autoSync: false,
        };
        await settingsApi.s3SyncSaveSettings(disabledS3, false);
        setS3Enabled(false);
        setS3AutoSync(false);
      }
      await queryClient.invalidateQueries();
      setSyncType(pendingSyncType);
    } catch (error) {
      toast.error(
        t("settings.s3Sync.mutualExclusionFailed", {
          error: (error as Error)?.message ?? String(error),
        }),
      );
    }
    setPendingSyncType(null);
  }, [pendingSyncType, closeDialog, config, s3Config, queryClient, t]);

  const handleMutualExclusionCancel = useCallback(() => {
    closeDialog();
    setPendingSyncType(null);
  }, [closeDialog]);

  // ─── Derived state ──────────────────────────────────────

  const isLoading = actionState !== "idle";
  const isS3Loading = s3ActionState !== "idle";
  const hasSavedConfig = Boolean(
    config?.baseUrl?.trim() && config?.username?.trim(),
  );
  const hasS3SavedConfig = Boolean(
    s3Config?.bucket?.trim() && s3Config?.accessKeyId?.trim(),
  );

  const lastSyncAt = config?.status?.lastSyncAt;
  const lastSyncDisplay = lastSyncAt
    ? new Date(lastSyncAt * 1000).toLocaleString()
    : null;
  const lastError = config?.status?.lastError?.trim();
  const showAutoSyncError =
    !!lastError && config?.status?.lastErrorSource === "auto";

  const s3LastSyncAt = s3Config?.status?.lastSyncAt;
  const s3LastSyncDisplay = s3LastSyncAt
    ? new Date(s3LastSyncAt * 1000).toLocaleString()
    : null;
  const s3LastError = s3Config?.status?.lastError?.trim();
  const s3ShowAutoSyncError =
    !!s3LastError && s3Config?.status?.lastErrorSource === "auto";

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

      {/* ─── Sync type selector ───────────────────────────── */}
      <div className="flex items-center gap-4">
        <label className="w-40 text-xs font-medium text-foreground shrink-0">
          {t("settings.syncType.label")}
        </label>
        <Select
          value={syncType}
          onValueChange={handleSyncTypeChange}
          disabled={isLoading || isS3Loading}
        >
          <SelectTrigger className="text-xs flex-1">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="webdav">{t("settings.syncType.webdav")}</SelectItem>
            <SelectItem value="s3">{t("settings.syncType.s3")}</SelectItem>
          </SelectContent>
        </Select>
      </div>

      {/* ─── WebDAV form ──────────────────────────────────── */}
      {syncType === "webdav" && (
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
      )}

      {/* ─── S3 form ──────────────────────────────────────── */}
      {syncType === "s3" && (
      <div className="space-y-4 rounded-lg border border-border bg-muted/40 p-6">
        <div className="space-y-3">
          {/* S3 preset selector */}
          <div className="flex items-center gap-4">
            <label className="w-40 text-xs font-medium text-foreground shrink-0">
              {t("settings.s3Sync.presets.label")}
            </label>
            <Select
              value={s3Preset}
              onValueChange={handleS3PresetChange}
              disabled={isS3Loading}
            >
              <SelectTrigger className="text-xs flex-1">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {S3_PRESETS.map((preset) => (
                  <SelectItem key={preset.id} value={preset.id}>
                    {t(preset.label)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          {/* Preset hint */}
          {activeS3Preset?.hint && (
            <div className="flex items-start gap-2 pl-44 text-xs text-muted-foreground">
              <Info className="h-3.5 w-3.5 shrink-0 mt-0.5" />
              <span>{t(activeS3Preset.hint)}</span>
            </div>
          )}

          {/* Region */}
          <div className="flex items-center gap-4">
            <label className="w-40 text-xs font-medium text-foreground shrink-0">
              {t("settings.s3Sync.region")}
            </label>
            <Input
              value={s3Region}
              onChange={(e) => { setS3Region(e.target.value); markS3Dirty(); }}
              placeholder={activeS3Preset?.regionPlaceholder ?? "us-east-1"}
              className="text-xs flex-1"
              disabled={isS3Loading}
            />
          </div>

          {/* Bucket */}
          <div className="flex items-center gap-4">
            <label className="w-40 text-xs font-medium text-foreground shrink-0">
              {t("settings.s3Sync.bucket")}
            </label>
            <Input
              value={s3Bucket}
              onChange={(e) => { setS3Bucket(e.target.value); markS3Dirty(); }}
              placeholder={t("settings.s3Sync.bucketPlaceholder")}
              className="text-xs flex-1"
              disabled={isS3Loading}
            />
          </div>

          {/* Access Key ID */}
          <div className="flex items-center gap-4">
            <label className="w-40 text-xs font-medium text-foreground shrink-0">
              {t("settings.s3Sync.accessKeyId")}
            </label>
            <Input
              value={s3AccessKeyId}
              onChange={(e) => { setS3AccessKeyId(e.target.value); markS3Dirty(); }}
              placeholder={t("settings.s3Sync.accessKeyIdPlaceholder")}
              className="text-xs flex-1"
              disabled={isS3Loading}
            />
          </div>

          {/* Secret Access Key */}
          <div className="flex items-center gap-4">
            <label className="w-40 text-xs font-medium text-foreground shrink-0">
              {t("settings.s3Sync.secretAccessKey")}
            </label>
            <Input
              type="password"
              value={s3SecretAccessKey}
              onChange={(e) => { setS3SecretAccessKey(e.target.value); setS3SecretTouched(true); markS3Dirty(); }}
              placeholder={t("settings.s3Sync.secretAccessKeyPlaceholder")}
              className="text-xs flex-1"
              autoComplete="off"
              disabled={isS3Loading}
            />
          </div>

          {/* Endpoint (optional) */}
          <div className="flex items-center gap-4">
            <label className="w-40 text-xs font-medium text-foreground shrink-0">
              {t("settings.s3Sync.endpoint")}
              <span className="block text-[10px] font-normal text-muted-foreground">
                {t("settings.s3Sync.endpointHint")}
              </span>
            </label>
            <Input
              value={s3Endpoint}
              onChange={(e) => { setS3Endpoint(e.target.value); markS3Dirty(); }}
              placeholder={t("settings.s3Sync.endpointPlaceholder")}
              className="text-xs flex-1"
              disabled={isS3Loading}
            />
          </div>

          {/* Remote Root */}
          <div className="flex items-center gap-4">
            <label className="w-40 text-xs font-medium text-foreground shrink-0">
              {t("settings.s3Sync.remoteRoot")}
              <span className="block text-[10px] font-normal text-muted-foreground">
                {t("settings.s3Sync.remoteRootDefault")}
              </span>
            </label>
            <Input
              value={s3RemoteRoot}
              onChange={(e) => { setS3RemoteRoot(e.target.value); markS3Dirty(); }}
              placeholder="cc-switch-sync"
              className="text-xs flex-1"
              disabled={isS3Loading}
            />
          </div>

          {/* Profile */}
          <div className="flex items-center gap-4">
            <label className="w-40 text-xs font-medium text-foreground shrink-0">
              {t("settings.s3Sync.profile")}
              <span className="block text-[10px] font-normal text-muted-foreground">
                {t("settings.s3Sync.profileDefault")}
              </span>
            </label>
            <Input
              value={s3Profile}
              onChange={(e) => { setS3Profile(e.target.value); markS3Dirty(); }}
              placeholder="default"
              className="text-xs flex-1"
              disabled={isS3Loading}
            />
          </div>

          {/* Auto Sync toggle */}
          <div className="flex items-start gap-4">
            <label className="w-40 text-xs font-medium text-foreground shrink-0">
              {t("settings.s3Sync.autoSync")}
              <span className="block text-[10px] font-normal text-muted-foreground">
                {t("settings.s3Sync.autoSyncHint")}
              </span>
            </label>
            <div className="pt-1">
              <Switch
                checked={s3AutoSync}
                onCheckedChange={(checked) => { setS3AutoSync(checked); markS3Dirty(); }}
                aria-label={t("settings.s3Sync.autoSync")}
                disabled={isS3Loading}
              />
            </div>
          </div>

          {/* Enabled toggle */}
          <div className="flex items-start gap-4">
            <label className="w-40 text-xs font-medium text-foreground shrink-0">
              {t("settings.s3Sync.enabled")}
              <span className="block text-[10px] font-normal text-muted-foreground">
                {t("settings.s3Sync.enabledHint")}
              </span>
            </label>
            <div className="pt-1">
              <Switch
                checked={s3Enabled}
                onCheckedChange={(checked) => { setS3Enabled(checked); markS3Dirty(); }}
                aria-label={t("settings.s3Sync.enabled")}
                disabled={isS3Loading}
              />
            </div>
          </div>
        </div>

        {/* Last sync time */}
        {s3LastSyncDisplay && (
          <p className="text-xs text-muted-foreground">
            {t("settings.s3Sync.lastSync", { time: s3LastSyncDisplay })}
          </p>
        )}
        {s3ShowAutoSyncError && (
          <div className="rounded-lg border border-red-300/70 bg-red-50/80 px-3 py-2 text-xs text-red-900 dark:border-red-500/50 dark:bg-red-950/30 dark:text-red-200">
            <p className="font-medium">
              {t("settings.s3Sync.autoSyncLastErrorTitle")}
            </p>
            <p className="mt-1 break-all whitespace-pre-wrap">{s3LastError}</p>
            <p className="mt-1 text-[11px] text-red-700/90 dark:text-red-300/80">
              {t("settings.s3Sync.autoSyncLastErrorHint")}
            </p>
          </div>
        )}

        {/* Config buttons + save status */}
        <div className="flex flex-wrap items-center gap-3 pt-2">
          <ActionButton
            type="button"
            variant="outline"
            size="sm"
            onClick={handleS3Test}
            actionState={s3ActionState}
            targetState="testing"
            icon={Link2}
            activeLabel={t("settings.s3Sync.testing")}
            idleLabel={t("settings.s3Sync.test")}
          />
          <ActionButton
            type="button"
            variant="outline"
            size="sm"
            onClick={handleS3Save}
            actionState={s3ActionState}
            targetState="saving"
            icon={Save}
            activeLabel={t("settings.s3Sync.saving")}
            idleLabel={t("settings.s3Sync.save")}
          />

          {/* Save status indicator */}
          {s3Dirty && (
            <span className="inline-flex items-center gap-1.5 text-xs text-amber-500 dark:text-amber-400 animate-in fade-in duration-200">
              <span className="h-1.5 w-1.5 rounded-full bg-amber-500 dark:bg-amber-400" />
              {t("settings.s3Sync.unsaved")}
            </span>
          )}
          {!s3Dirty && s3JustSaved && (
            <span className="inline-flex items-center gap-1.5 text-xs text-emerald-600 dark:text-emerald-400 animate-in fade-in duration-200">
              <Check className="h-3 w-3" />
              {t("settings.s3Sync.saved")}
            </span>
          )}
        </div>

        {/* Sync buttons */}
        <div className="flex flex-wrap items-center gap-3 border-t border-border pt-4">
          <ActionButton
            type="button"
            size="sm"
            onClick={handleS3UploadClick}
            disabled={!hasS3SavedConfig}
            actionState={s3ActionState}
            targetState="uploading"
            alsoActiveFor={["fetching_remote"]}
            icon={UploadCloud}
            activeLabel={
              s3ActionState === "fetching_remote"
                ? t("settings.s3Sync.fetchingRemote")
                : t("settings.s3Sync.uploading")
            }
            idleLabel={t("settings.s3Sync.upload")}
          />
          <ActionButton
            type="button"
            variant="secondary"
            size="sm"
            onClick={handleS3DownloadClick}
            disabled={!hasS3SavedConfig}
            actionState={s3ActionState}
            targetState="downloading"
            alsoActiveFor={["fetching_remote"]}
            icon={DownloadCloud}
            activeLabel={
              s3ActionState === "fetching_remote"
                ? t("settings.s3Sync.fetchingRemote")
                : t("settings.s3Sync.downloading")
            }
            idleLabel={t("settings.s3Sync.download")}
          />
        </div>
        {!hasS3SavedConfig && (
          <p className="text-xs text-muted-foreground">
            {t("settings.s3Sync.saveBeforeSync")}
          </p>
        )}
      </div>
      )}

      {/* ─── WebDAV Upload confirmation dialog ───────────── */}
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

      {/* ─── WebDAV Download confirmation dialog ─────────── */}
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

      {/* ─── S3 Upload confirmation dialog ───────────────── */}
      <Dialog
        open={s3DialogType === "upload"}
        onOpenChange={(open) => {
          if (!open) closeS3Dialog();
        }}
      >
        <DialogContent className="max-w-sm" zIndex="alert">
          <DialogHeader className="space-y-3 border-b-0 bg-transparent pb-0">
            <DialogTitle className="flex items-center gap-2 text-lg font-semibold">
              <AlertTriangle className="h-5 w-5 text-destructive" />
              {t("settings.s3Sync.confirmUpload.title")}
            </DialogTitle>
            <DialogDescription asChild>
              <div className="space-y-3 text-sm leading-relaxed">
                <p>{t("settings.s3Sync.confirmUpload.content")}</p>
                <ul className="list-disc pl-5 space-y-1 text-muted-foreground">
                  <li>{t("settings.s3Sync.confirmUpload.dbItem")}</li>
                  <li>{t("settings.s3Sync.confirmUpload.skillsItem")}</li>
                </ul>
                <p className="text-muted-foreground">
                  {t("settings.s3Sync.confirmUpload.targetPath")}
                  {": "}
                  <code className="ml-1 text-xs bg-muted px-1.5 py-0.5 rounded">
                    {s3Bucket.trim() || "bucket"}/{s3RemoteRoot.trim() || "cc-switch-sync"}/v2/
                    {s3Profile.trim() || "default"}
                  </code>
                </p>
                {s3RemoteInfo && (
                  <div className="rounded-lg border border-border bg-muted/50 p-3 space-y-2">
                    <p className="text-xs font-medium text-foreground">
                      {t("settings.s3Sync.confirmUpload.existingData")}
                    </p>
                    <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1.5 text-xs text-muted-foreground">
                      <dt className="font-medium text-foreground">
                        {t("settings.s3Sync.confirmUpload.deviceName")}
                      </dt>
                      <dd>
                        <code className="bg-muted px-1.5 py-0.5 rounded">
                          {s3RemoteInfo.deviceName}
                        </code>
                      </dd>
                      <dt className="font-medium text-foreground">
                        {t("settings.s3Sync.confirmUpload.createdAt")}
                      </dt>
                      <dd>{formatDate(s3RemoteInfo.createdAt)}</dd>
                    </dl>
                  </div>
                )}
                {s3RemoteInfo && (
                  <p className="text-destructive font-medium">
                    {t("settings.s3Sync.confirmUpload.warning")}
                  </p>
                )}
              </div>
            </DialogDescription>
          </DialogHeader>
          <DialogFooter className="flex gap-2 border-t-0 bg-transparent pt-2 sm:justify-end">
            <Button variant="outline" onClick={closeS3Dialog}>
              {t("common.cancel")}
            </Button>
            <Button variant="destructive" onClick={handleS3UploadConfirm}>
              {t("settings.s3Sync.confirmUpload.confirm")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* ─── S3 Download confirmation dialog ─────────────── */}
      <Dialog
        open={s3DialogType === "download"}
        onOpenChange={(open) => {
          if (!open) closeS3Dialog();
        }}
      >
        <DialogContent className="max-w-sm" zIndex="alert">
          <DialogHeader className="space-y-3 border-b-0 bg-transparent pb-0">
            <DialogTitle className="flex items-center gap-2 text-lg font-semibold">
              <AlertTriangle className="h-5 w-5 text-destructive" />
              {t("settings.s3Sync.confirmDownload.title")}
            </DialogTitle>
            <DialogDescription asChild>
              <div className="space-y-3 text-sm leading-relaxed">
                {s3RemoteInfo && (
                  <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1.5 text-muted-foreground">
                    <dt className="font-medium text-foreground">
                      {t("settings.s3Sync.confirmDownload.deviceName")}
                    </dt>
                    <dd>
                      <code className="text-xs bg-muted px-1.5 py-0.5 rounded">
                        {s3RemoteInfo.deviceName}
                      </code>
                    </dd>
                    <dt className="font-medium text-foreground">
                      {t("settings.s3Sync.confirmDownload.createdAt")}
                    </dt>
                    <dd>{formatDate(s3RemoteInfo.createdAt)}</dd>
                    <dt className="font-medium text-foreground">
                      {t("settings.s3Sync.confirmDownload.artifacts")}
                    </dt>
                    <dd>{s3RemoteInfo.artifacts.join(", ")}</dd>
                  </dl>
                )}
                <p className="text-destructive font-medium">
                  {t("settings.s3Sync.confirmDownload.warning")}
                </p>
              </div>
            </DialogDescription>
          </DialogHeader>
          <DialogFooter className="flex gap-2 border-t-0 bg-transparent pt-2 sm:justify-end">
            <Button variant="outline" onClick={closeS3Dialog}>
              {t("common.cancel")}
            </Button>
            <Button variant="destructive" onClick={handleS3DownloadConfirm}>
              {t("settings.s3Sync.confirmDownload.confirm")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* ─── Mutual exclusion confirmation dialog ────────── */}
      <Dialog
        open={dialogType === "mutual_exclusion"}
        onOpenChange={(open) => {
          if (!open) handleMutualExclusionCancel();
        }}
      >
        <DialogContent className="max-w-sm" zIndex="alert">
          <DialogHeader className="space-y-3 border-b-0 bg-transparent pb-0">
            <DialogTitle className="flex items-center gap-2 text-lg font-semibold">
              <AlertTriangle className="h-5 w-5 text-destructive" />
              {t("settings.s3Sync.mutualExclusionTitle")}
            </DialogTitle>
            <DialogDescription asChild>
              <div className="space-y-3 text-sm leading-relaxed">
                <p>{t("settings.s3Sync.mutualExclusionMessage")}</p>
              </div>
            </DialogDescription>
          </DialogHeader>
          <DialogFooter className="flex gap-2 border-t-0 bg-transparent pt-2 sm:justify-end">
            <Button variant="outline" onClick={handleMutualExclusionCancel}>
              {t("common.cancel")}
            </Button>
            <Button variant="destructive" onClick={handleMutualExclusionConfirm}>
              {t("common.confirm")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </section>
  );
}
