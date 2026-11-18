import { useCallback, useContext, useEffect, useRef, useState } from "react";
import { appHas } from "@/config/appSpecs";
import { QueryClientContext } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { toast } from "@/lib/toast";
import {
  ArrowLeft,
  ChevronDown,
  ChevronRight,
  Eye,
  History,
  Pin,
  RotateCcw,
  Trash2,
  X,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { APP_DISPLAY_NAME, AppGlyph } from "@/components/shell/AppGlyph";
import { formatBytes } from "@/components/settings/BackupStorageSection";
import {
  backupsApi,
  type ConfigBackupEntry,
  type ConfigBackupPreview,
  type ConfigBackupReason,
} from "@/lib/api/settings";
import type { AppId } from "@/lib/api";
import { extractErrorMessage } from "@/utils/errorUtils";

const AUTO_COLLAPSE_FILE_BYTES = 16 * 1024;
const AUXILIARY_LARGE_FILES = new Set(["cc-switch-model-catalog.json"]);

export function formatHistoryDate(isoString: string): string {
  try {
    const date = new Date(isoString);
    return Number.isNaN(date.getTime()) ? isoString : date.toLocaleString();
  } catch {
    return isoString;
  }
}

interface ProviderConfigHistoryPanelProps {
  appId: AppId;
  standalone?: boolean;
  onClose: () => void;
  onRolledBack?: () => Promise<void> | void;
}

export function ProviderConfigHistoryPanel({
  appId,
  standalone = false,
  onClose,
  onRolledBack,
}: ProviderConfigHistoryPanelProps) {
  const { t } = useTranslation();
  const queryClient = useContext(QueryClientContext);
  const previewCacheRef = useRef<Map<string, ConfigBackupPreview>>(new Map());

  const [backups, setBackups] = useState<ConfigBackupEntry[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const request = useRef(0);
  const refresh = useCallback(async () => {
    const version = ++request.current;
    setIsLoading(true);
    try {
      const entries = await backupsApi.listAppConfigBackups(appId);
      if (version === request.current) setBackups(entries);
    } catch (error) {
      if (version === request.current) {
        setBackups([]);
        toast.error(
          extractErrorMessage(error) || t("provider.configHistory.loadFailed"),
        );
      }
    } finally {
      if (version === request.current) setIsLoading(false);
    }
  }, [appId, t]);

  const [isRestoring, setIsRestoring] = useState(false);
  const [isDeleting, setIsDeleting] = useState(false);
  const [previewLoadingId, setPreviewLoadingId] = useState<string | null>(null);
  const [previewData, setPreviewData] = useState<ConfigBackupPreview | null>(
    null,
  );
  const [expandedFiles, setExpandedFiles] = useState<Record<string, boolean>>(
    {},
  );
  const [confirmRestoreId, setConfirmRestoreId] = useState<string | null>(null);
  const [confirmDeleteId, setConfirmDeleteId] = useState<string | null>(null);

  useEffect(() => {
    setBackups([]);
    void refresh();
    previewCacheRef.current.clear();
    setPreviewData(null);
    setExpandedFiles({});
    setConfirmRestoreId(null);
    setConfirmDeleteId(null);
    return () => {
      ++request.current;
    };
  }, [refresh]);

  const appName = APP_DISPLAY_NAME[appId] ?? appId;
  const reasonLabel = (reason: ConfigBackupReason) =>
    t(`provider.configHistory.reasons.${reason}`, { defaultValue: reason });

  const restoreMessage = appHas(appId, "routeMode")
    ? "provider.configHistory.confirmRestoreDirect"
    : "provider.configHistory.confirmRestoreMessage";
  const previewLabel = t("provider.configHistory.preview");
  const restoreLabel = t("provider.configHistory.restore");
  const deleteLabel = t("provider.configHistory.delete");

  const invalidateAll = async () => {
    await refresh();
    if (queryClient) {
      await queryClient.invalidateQueries();
    }
  };

  const applyPreviewData = (data: ConfigBackupPreview) => {
    const initialExpanded: Record<string, boolean> = {};
    for (const file of data.files) {
      const collapseByDefault =
        file.existed &&
        !file.binary &&
        (file.sizeBytes > AUTO_COLLAPSE_FILE_BYTES ||
          AUXILIARY_LARGE_FILES.has(file.key));
      initialExpanded[file.key] = !collapseByDefault;
    }
    setExpandedFiles(initialExpanded);
    setPreviewData(data);
  };

  const handlePreview = async (backup: ConfigBackupEntry) => {
    setConfirmRestoreId(null);
    setConfirmDeleteId(null);

    const cached = previewCacheRef.current.get(backup.id);
    if (cached) {
      applyPreviewData(cached);
      return;
    }

    setPreviewLoadingId(backup.id);
    try {
      const data = await backupsApi.previewAppConfigBackup(appId, backup.id);
      previewCacheRef.current.set(backup.id, data);
      applyPreviewData(data);
    } catch (error) {
      toast.error(
        extractErrorMessage(error) || t("provider.configHistory.previewFailed"),
      );
    } finally {
      setPreviewLoadingId(null);
    }
  };

  const handleRestore = async (backup: ConfigBackupEntry) => {
    setIsRestoring(true);
    try {
      await backupsApi.restoreAppConfigBackup(appId, backup.id);
      setConfirmRestoreId(null);
      setPreviewData(null);
      previewCacheRef.current.clear();
      await invalidateAll();
      await onRolledBack?.();
      toast.success(
        t("provider.configHistory.restoreSuccess", { app: appName }),
      );
    } catch (error) {
      toast.error(
        extractErrorMessage(error) || t("provider.configHistory.restoreFailed"),
      );
    } finally {
      setIsRestoring(false);
    }
  };

  const handleDelete = async (backup: ConfigBackupEntry) => {
    setIsDeleting(true);
    try {
      await backupsApi.deleteAppConfigBackup(appId, backup.id);
      previewCacheRef.current.delete(backup.id);
      setConfirmDeleteId(null);
      if (previewData?.entry.id === backup.id) {
        setPreviewData(null);
      }
      await invalidateAll();
      toast.success(t("provider.configHistory.deleteSuccess"));
    } catch (error) {
      toast.error(
        extractErrorMessage(error) || t("provider.configHistory.deleteFailed"),
      );
    } finally {
      setIsDeleting(false);
    }
  };

  return (
    <aside
      aria-label={t("provider.configHistory.panelTitle", { app: appName })}
      className={`flex min-h-0 flex-col border-border bg-surface/50 ${standalone ? "w-full" : "w-[380px] shrink-0 border-l xl:w-[440px]"}`}
    >
      <div className="shrink-0 border-b border-border px-4 py-3 space-y-1">
        <div className="flex items-center justify-between gap-2">
          <div className="flex items-center gap-2 min-w-0">
            <AppGlyph app={appId} size={18} />
            <h3 className="text-sm font-semibold text-fg-1 truncate m-0">
              {t("provider.configHistory.panelTitle", { app: appName })}
            </h3>
          </div>
          <Button
            type="button"
            variant="ghost"
            size="icon"
            className="h-7 w-7 shrink-0"
            onClick={onClose}
            title={t("common.close", { defaultValue: "Close" })}
            aria-label={t("common.close", { defaultValue: "Close" })}
          >
            <X className="h-4 w-4" />
          </Button>
        </div>
        <p className="text-xs text-fg-2 m-0">
          {t("provider.configHistory.panelDescription", { app: appName })}
        </p>
      </div>

      {previewData ? (
        <div className="flex-1 min-h-0 flex flex-col">
          <div className="shrink-0 border-b border-border bg-subtle/60 px-4 py-2 space-y-2">
            <div className="flex items-center justify-between gap-2">
              <Button
                type="button"
                variant="ghost"
                size="sm"
                className="h-7 px-2 text-xs"
                onClick={() => {
                  setPreviewData(null);
                  setConfirmRestoreId(null);
                }}
              >
                <ArrowLeft className="h-3.5 w-3.5 mr-1" />
                {t("provider.configHistory.backToList")}
              </Button>
              <Button
                type="button"
                variant="outline"
                size="sm"
                className="h-7 px-2.5 text-xs"
                disabled={isRestoring}
                onClick={() => setConfirmRestoreId(previewData.entry.id)}
              >
                <RotateCcw className="h-3 w-3 mr-1" />
                {restoreLabel}
              </Button>
            </div>
            <div className="flex items-center gap-1.5 text-xs text-fg-2">
              <span className="inline-flex items-center gap-1 rounded px-1.5 py-px text-[11px] font-medium border border-border bg-surface text-fg-1">
                {previewData.entry.pinned && <Pin className="h-2.5 w-2.5" />}
                {reasonLabel(previewData.entry.reason)}
              </span>
              <span className="truncate">
                {formatHistoryDate(previewData.entry.createdAt)}
              </span>
            </div>
            {confirmRestoreId === previewData.entry.id && (
              <div className="rounded-md border border-border bg-surface p-2.5 space-y-2">
                <p className="text-xs text-fg-1 m-0">
                  {t(restoreMessage, {
                    app: appName,
                    date: formatHistoryDate(previewData.entry.createdAt),
                  })}
                </p>
                <div className="flex items-center justify-end gap-1.5">
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    className="h-7 px-2.5 text-xs"
                    disabled={isRestoring}
                    onClick={() => setConfirmRestoreId(null)}
                  >
                    {t("common.cancel")}
                  </Button>
                  <Button
                    type="button"
                    size="sm"
                    className="h-7 px-2.5 text-xs"
                    disabled={isRestoring}
                    onClick={() => void handleRestore(previewData.entry)}
                  >
                    {isRestoring
                      ? t("provider.configHistory.restoring")
                      : t("provider.configHistory.confirmRestoreAction")}
                  </Button>
                </div>
              </div>
            )}
          </div>

          <div className="flex-1 min-h-0 overflow-y-auto scroll-stable p-3 space-y-3">
            {previewData.files.map((file) => {
              const isExpanded = expandedFiles[file.key] ?? true;
              return (
                <div
                  key={file.key}
                  className="rounded-lg border border-border overflow-hidden bg-surface"
                >
                  <button
                    type="button"
                    className="w-full flex items-center justify-between gap-2 bg-subtle px-3 py-1.5 text-xs border-b border-border text-left hover:bg-subtle/80"
                    onClick={() =>
                      setExpandedFiles((prev) => ({
                        ...prev,
                        [file.key]: !isExpanded,
                      }))
                    }
                  >
                    <span className="flex items-center gap-1.5 font-mono font-medium truncate">
                      {isExpanded ? (
                        <ChevronDown className="h-3.5 w-3.5 shrink-0 text-fg-2" />
                      ) : (
                        <ChevronRight className="h-3.5 w-3.5 shrink-0 text-fg-2" />
                      )}
                      <span className="truncate">{file.key}</span>
                    </span>
                    <span
                      className="text-fg-2 truncate shrink-0"
                      title={file.originalPath}
                    >
                      {file.existed
                        ? formatBytes(file.sizeBytes)
                        : t("provider.configHistory.notExisted")}
                    </span>
                  </button>
                  {isExpanded &&
                    (!file.existed ? (
                      <div className="p-3 text-xs text-fg-2">
                        {t("provider.configHistory.missingFileHint")}
                      </div>
                    ) : file.binary ? (
                      <div className="p-3 text-xs text-fg-2">
                        {t("provider.configHistory.binaryFileHint")}
                      </div>
                    ) : (
                      <div className="p-2.5">
                        <pre className="m-0 text-xs font-mono whitespace-pre overflow-x-auto max-h-72 overflow-y-auto">
                          {file.content ?? ""}
                        </pre>
                        {file.truncated && (
                          <p className="mt-2 mb-0 text-xs text-fg-2">
                            {t("provider.configHistory.truncatedHint")}
                          </p>
                        )}
                      </div>
                    ))}
                </div>
              );
            })}
          </div>
        </div>
      ) : (
        <div className="flex-1 min-h-0 overflow-y-auto scroll-stable p-3 space-y-2">
          {isLoading ? (
            <div className="py-10 text-center text-sm text-fg-2">
              {t("common.loading", { defaultValue: "Loading..." })}
            </div>
          ) : backups.length === 0 ? (
            <div className="py-10 px-4 text-center space-y-1.5">
              <History className="h-5 w-5 mx-auto text-fg-3" />
              <p className="text-xs text-fg-2 m-0">
                {t("provider.configHistory.empty")}
              </p>
            </div>
          ) : (
            backups.map((backup) => {
              const fileNames = backup.files.map((f) => f.key).join(", ");
              const confirmingRestore = confirmRestoreId === backup.id;
              const confirmingDelete = confirmDeleteId === backup.id;
              return (
                <div
                  key={backup.id}
                  className="rounded-lg border border-border bg-surface p-2.5 space-y-2 text-sm"
                >
                  <div className="flex items-start justify-between gap-2">
                    <div className="flex-1 min-w-0 space-y-0.5">
                      <div className="flex items-center gap-1.5 flex-wrap">
                        <span className="inline-flex items-center gap-1 rounded px-1.5 py-px text-[11px] font-medium border border-border bg-subtle text-fg-1">
                          {backup.pinned && <Pin className="h-2.5 w-2.5" />}
                          {reasonLabel(backup.reason)}
                        </span>
                        <span className="text-xs text-fg-2">
                          {formatHistoryDate(backup.createdAt)}
                        </span>
                      </div>
                      <div
                        className="font-mono text-xs text-fg-2 truncate"
                        title={fileNames}
                      >
                        {fileNames} &middot;{" "}
                        {formatBytes(backup.totalSizeBytes)}
                      </div>
                    </div>

                    <div className="flex items-center gap-1 shrink-0">
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon"
                        className="h-7 w-7"
                        onClick={() => void handlePreview(backup)}
                        disabled={
                          isRestoring ||
                          isDeleting ||
                          previewLoadingId === backup.id
                        }
                        title={previewLabel}
                        aria-label={previewLabel}
                      >
                        <Eye className="h-3.5 w-3.5" />
                      </Button>
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon"
                        className="h-7 w-7 text-destructive hover:text-destructive"
                        onClick={() => {
                          setConfirmRestoreId(null);
                          setConfirmDeleteId(
                            confirmingDelete ? null : backup.id,
                          );
                        }}
                        disabled={isRestoring || isDeleting}
                        title={deleteLabel}
                        aria-label={deleteLabel}
                      >
                        <Trash2 className="h-3.5 w-3.5" />
                      </Button>
                      <Button
                        type="button"
                        variant="outline"
                        size="sm"
                        className="h-7 px-2 text-xs"
                        disabled={isRestoring || isDeleting}
                        onClick={() => {
                          setConfirmDeleteId(null);
                          setConfirmRestoreId(
                            confirmingRestore ? null : backup.id,
                          );
                        }}
                      >
                        <RotateCcw className="h-3 w-3 mr-1" />
                        {restoreLabel}
                      </Button>
                    </div>
                  </div>

                  {confirmingRestore && (
                    <div className="rounded-md border border-border bg-subtle p-2 space-y-2">
                      <p className="text-xs text-fg-1 m-0">
                        {t(restoreMessage, {
                          app: appName,
                          date: formatHistoryDate(backup.createdAt),
                        })}
                      </p>
                      <div className="flex items-center justify-end gap-1.5">
                        <Button
                          type="button"
                          variant="outline"
                          size="sm"
                          className="h-6 px-2 text-xs"
                          disabled={isRestoring}
                          onClick={() => setConfirmRestoreId(null)}
                        >
                          {t("common.cancel")}
                        </Button>
                        <Button
                          type="button"
                          size="sm"
                          className="h-6 px-2 text-xs"
                          disabled={isRestoring}
                          onClick={() => void handleRestore(backup)}
                        >
                          {isRestoring
                            ? t("provider.configHistory.restoring")
                            : t("provider.configHistory.confirmRestoreAction")}
                        </Button>
                      </div>
                    </div>
                  )}

                  {confirmingDelete && (
                    <div className="rounded-md border border-destructive/40 bg-subtle p-2 space-y-2">
                      <p className="text-xs text-fg-1 m-0">
                        {backup.pinned
                          ? t(
                              "provider.configHistory.confirmDeletePinnedMessage",
                            )
                          : t("provider.configHistory.confirmDeleteMessage")}
                      </p>
                      <div className="flex items-center justify-end gap-1.5">
                        <Button
                          type="button"
                          variant="outline"
                          size="sm"
                          className="h-6 px-2 text-xs"
                          disabled={isDeleting}
                          onClick={() => setConfirmDeleteId(null)}
                        >
                          {t("common.cancel")}
                        </Button>
                        <Button
                          type="button"
                          variant="destructive"
                          size="sm"
                          className="h-6 px-2 text-xs"
                          disabled={isDeleting}
                          onClick={() => void handleDelete(backup)}
                        >
                          {isDeleting
                            ? t("provider.configHistory.deleting")
                            : deleteLabel}
                        </Button>
                      </div>
                    </div>
                  )}
                </div>
              );
            })
          )}
        </div>
      )}
    </aside>
  );
}
