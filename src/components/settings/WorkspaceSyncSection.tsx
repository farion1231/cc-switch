import { useCallback, useEffect, useState } from "react";
import {
  UploadCloud,
  DownloadCloud,
  Loader2,
  Save,
  AlertTriangle,
  RefreshCw,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  workspaceSyncApi,
  type WorkspaceSyncSettings,
  type WorkspaceScanPreviewItem,
  type WorkspaceSyncReport,
} from "@/lib/api/workspaceSync";

const ALL_PROVIDERS = ["claude", "codex", "grokbuild", "opencode", "cursor"];

const PROVIDER_LABELS: Record<string, string> = {
  claude: "Claude Code",
  codex: "Codex",
  grokbuild: "Grok Build",
  opencode: "OpenCode",
  cursor: "Cursor",
};

export function WorkspaceSyncSection() {
  const { t } = useTranslation();
  const [settings, setSettings] = useState<WorkspaceSyncSettings>({
    enabled: false,
    transport: "webdav",
    providers: [],
    remoteRoot: "cc-switch-workspace",
    profile: "default",
  });
  const [preview, setPreview] = useState<WorkspaceScanPreviewItem[]>([]);
  const [loading, setLoading] = useState(false);
  const [busy, setBusy] = useState<null | "backup" | "merge" | "save">(null);
  const [report, setReport] = useState<WorkspaceSyncReport | null>(null);

  const refreshPreview = useCallback(async () => {
    try {
      const res = await workspaceSyncApi.scanPreview();
      setPreview(res.providers);
    } catch (e) {
      // Preview is best-effort; don't block the panel.
      console.warn("workspace scan preview failed", e);
    }
  }, []);

  useEffect(() => {
    let active = true;
    (async () => {
      setLoading(true);
      try {
        const loaded = await workspaceSyncApi.getSettings();
        if (active) setSettings(loaded);
      } catch (e) {
        console.warn("load workspace sync settings failed", e);
      } finally {
        if (active) setLoading(false);
      }
      await refreshPreview();
    })();
    return () => {
      active = false;
    };
  }, [refreshPreview]);

  const toggleProvider = (id: string, checked: boolean) => {
    setSettings((s) => ({
      ...s,
      providers: checked
        ? [...s.providers, id]
        : s.providers.filter((p) => p !== id),
    }));
  };

  const save = async () => {
    setBusy("save");
    try {
      await workspaceSyncApi.saveSettings(settings);
      toast.success(t("settings.workspaceSync.saved"));
    } catch (e) {
      toast.error(String(e));
    } finally {
      setBusy(null);
    }
  };

  const runBackup = async () => {
    setBusy("backup");
    setReport(null);
    try {
      await workspaceSyncApi.saveSettings(settings);
      const r = await workspaceSyncApi.backup();
      setReport(r);
      toast.success(
        t("settings.workspaceSync.backupDone", {
          items: r.itemsTotal,
          blobs: r.blobsUploaded,
        }),
      );
    } catch (e) {
      toast.error(String(e));
    } finally {
      setBusy(null);
    }
  };

  const runMerge = async () => {
    setBusy("merge");
    setReport(null);
    try {
      await workspaceSyncApi.saveSettings(settings);
      const r = await workspaceSyncApi.merge();
      setReport(r);
      toast.success(
        t("settings.workspaceSync.mergeDone", {
          files: r.filesWritten,
          conflicts: r.conflicts.length,
        }),
      );
    } catch (e) {
      toast.error(String(e));
    } finally {
      setBusy(null);
    }
  };

  const countFor = (id: string) =>
    preview.find((p) => p.provider === id)?.itemCount ?? 0;
  const installedFor = (id: string) =>
    preview.find((p) => p.provider === id)?.installed ?? false;

  if (loading) {
    return (
      <div className="flex items-center gap-2 text-sm text-muted-foreground">
        <Loader2 className="h-4 w-4 animate-spin" />
        {t("common.loading")}
      </div>
    );
  }

  return (
    <div className="space-y-5">
      {/* Enable + transport */}
      <div className="flex items-center justify-between">
        <div>
          <div className="text-sm font-medium">
            {t("settings.workspaceSync.enable")}
          </div>
          <div className="text-xs text-muted-foreground">
            {t("settings.workspaceSync.enableHint")}
          </div>
        </div>
        <Switch
          checked={settings.enabled}
          onCheckedChange={(v) => setSettings((s) => ({ ...s, enabled: v }))}
        />
      </div>

      <div className="flex items-center gap-3">
        <span className="text-sm">{t("settings.workspaceSync.transport")}</span>
        <Select
          value={settings.transport}
          onValueChange={(v) =>
            setSettings((s) => ({ ...s, transport: v as "webdav" | "s3" }))
          }
        >
          <SelectTrigger className="w-40">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="webdav">WebDAV</SelectItem>
            <SelectItem value="s3">S3</SelectItem>
          </SelectContent>
        </Select>
        <span className="text-xs text-muted-foreground">
          {t("settings.workspaceSync.transportHint")}
        </span>
      </div>

      {/* Provider selection */}
      <div className="space-y-2">
        <div className="flex items-center justify-between">
          <span className="text-sm font-medium">
            {t("settings.workspaceSync.providers")}
          </span>
          <Button variant="ghost" size="sm" onClick={refreshPreview}>
            <RefreshCw className="h-3.5 w-3.5 mr-1" />
            {t("settings.workspaceSync.refresh")}
          </Button>
        </div>
        <div className="grid grid-cols-2 gap-2">
          {ALL_PROVIDERS.map((id) => {
            const installed = installedFor(id);
            return (
              <label
                key={id}
                className={`flex items-center gap-2 rounded-lg border px-3 py-2 text-sm ${
                  installed ? "" : "opacity-50"
                }`}
              >
                <Checkbox
                  checked={settings.providers.includes(id)}
                  disabled={!installed}
                  onCheckedChange={(v) => toggleProvider(id, Boolean(v))}
                />
                <span className="flex-1">{PROVIDER_LABELS[id] ?? id}</span>
                <span className="text-xs text-muted-foreground">
                  {installed
                    ? t("settings.workspaceSync.itemCount", {
                        count: countFor(id),
                      })
                    : t("settings.workspaceSync.notInstalled")}
                </span>
              </label>
            );
          })}
        </div>
      </div>

      {/* Actions */}
      <div className="flex flex-wrap gap-2">
        <Button variant="outline" size="sm" onClick={save} disabled={!!busy}>
          {busy === "save" ? (
            <Loader2 className="h-4 w-4 mr-1 animate-spin" />
          ) : (
            <Save className="h-4 w-4 mr-1" />
          )}
          {t("common.save")}
        </Button>
        <Button
          size="sm"
          onClick={runBackup}
          disabled={!!busy || !settings.enabled || settings.providers.length === 0}
        >
          {busy === "backup" ? (
            <Loader2 className="h-4 w-4 mr-1 animate-spin" />
          ) : (
            <UploadCloud className="h-4 w-4 mr-1" />
          )}
          {t("settings.workspaceSync.backup")}
        </Button>
        <Button
          size="sm"
          variant="secondary"
          onClick={runMerge}
          disabled={!!busy || !settings.enabled || settings.providers.length === 0}
        >
          {busy === "merge" ? (
            <Loader2 className="h-4 w-4 mr-1 animate-spin" />
          ) : (
            <DownloadCloud className="h-4 w-4 mr-1" />
          )}
          {t("settings.workspaceSync.merge")}
        </Button>
      </div>

      {/* Last-run report + conflicts */}
      {report && (
        <div className="rounded-lg border p-3 text-sm space-y-2">
          <div className="text-muted-foreground">
            {t("settings.workspaceSync.reportSummary", {
              providers: report.providersScanned,
              items: report.itemsTotal,
              files: report.filesWritten,
              blobs: report.blobsUploaded,
            })}
          </div>
          {report.conflicts.length > 0 && (
            <div className="space-y-1">
              <div className="flex items-center gap-1 text-amber-600 dark:text-amber-500 font-medium">
                <AlertTriangle className="h-4 w-4" />
                {t("settings.workspaceSync.conflicts", {
                  count: report.conflicts.length,
                })}
              </div>
              <ul className="space-y-1 max-h-40 overflow-auto">
                {report.conflicts.map((c, i) => (
                  <li key={i} className="text-xs text-muted-foreground">
                    <span className="font-mono">
                      {c.provider}/{c.logicalId}
                    </span>
                    {" — "}
                    {c.resolution}
                  </li>
                ))}
              </ul>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
