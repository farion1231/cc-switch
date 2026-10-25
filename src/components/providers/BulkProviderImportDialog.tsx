import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQueryClient } from "@tanstack/react-query";
import { FileUp, Loader2 } from "lucide-react";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import { APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { toast } from "@/lib/toast";
import { BULK_IMPORT_FORMATS } from "@/config/bulkImport";
import {
  providersApi,
  type BulkImportApp,
  type BulkProviderPreview,
} from "@/lib/api/providers";

const MAX_BYTES = 1024 * 1024;
const ERROR_KEYS = new Set([
  "unsupportedApp",
  "tooLarge",
  "invalidJson",
  "notArray",
  "empty",
  "tooManyRows",
  "invalidRow",
  "unknownFields",
  "required",
  "invalidValue",
  "invalidUrl",
  "maskedKey",
  "invalidFormat",
  "invalidConfig",
  "previewChanged",
  "storageFailed",
  "validationFailed",
  "fileRead",
  "invalidContext",
  "unsupportedContext",
  "dynamicValue",
]);

/** Mounted for a captured target app; unmount on close so credentials and previews are discarded. */
export function BulkProviderImportDialog({
  appId,
  onClose,
  onNavigationBlockedChange,
}: {
  appId: BulkImportApp;
  onClose: () => void;
  onNavigationBlockedChange?: (blocked: boolean) => void;
}) {
  const { t } = useTranslation();
  const spec = BULK_IMPORT_FORMATS[appId];
  const example = JSON.stringify(
    [
      {
        name: "Team gateway",
        base_url: "https://gateway.example.com",
        api_key: "YOUR_API_KEY",
        api_format: spec.defaultFormat,
        ...(spec.modelRequired ? { model: "YOUR_MODEL_ID" } : {}),
        ...(appId === "kimi" ? { context_window: 32768 } : {}),
      },
    ],
    null,
    2,
  );
  const queryClient = useQueryClient();
  const [source, setSource] = useState("");
  const [preview, setPreview] = useState<BulkProviderPreview | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [importing, setImporting] = useState(false);
  const generation = useRef(0);
  const importLock = useRef(false);
  const alive = useRef(true);
  const fileInput = useRef<HTMLInputElement>(null);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
      generation.current += 1;
    };
  }, []);
  useEffect(() => {
    onNavigationBlockedChange?.(importing);
    return () => onNavigationBlockedChange?.(false);
  }, [importing, onNavigationBlockedChange]);

  const errorText = (code: string, limit?: number | null) =>
    t(
      `provider.bulkImport.errors.${ERROR_KEYS.has(code) ? code : "storageFailed"}`,
      { limit },
    );
  const changeSource = (value: string) => {
    generation.current += 1;
    setSource(value);
    setPreview(null);
    setError(null);
    setLoading(false);
  };
  const loadFile = async (file?: File) => {
    if (!file || importLock.current) return;
    const current = ++generation.current;
    setPreview(null);
    setError(null);
    setLoading(true);
    try {
      if (file.size > MAX_BYTES) {
        setError("tooLarge");
        return;
      }
      const text = await file.text();
      if (alive.current && current === generation.current) setSource(text);
    } catch {
      if (alive.current && current === generation.current) setError("fileRead");
    } finally {
      if (alive.current && current === generation.current) setLoading(false);
    }
  };
  const check = async () => {
    if (importLock.current) return;
    const current = ++generation.current;
    setPreview(null);
    setError(null);
    if (new Blob([source]).size > MAX_BYTES) {
      setError("tooLarge");
      return;
    }
    setLoading(true);
    try {
      const result = await providersApi.previewBulkImport(appId, source);
      if (alive.current && current === generation.current) setPreview(result);
    } catch {
      if (alive.current && current === generation.current)
        setError("storageFailed");
    } finally {
      if (alive.current && current === generation.current) setLoading(false);
    }
  };
  const commit = async () => {
    if (
      importLock.current ||
      !preview?.token ||
      preview.errors.length ||
      !preview.newCount
    )
      return;
    importLock.current = true;
    setImporting(true);
    setError(null);
    try {
      const result = await providersApi.importBulk(
        appId,
        source,
        preview.token,
      );
      // A failed refresh must never turn an already committed import into an import failure.
      await Promise.allSettled([
        queryClient.invalidateQueries({ queryKey: ["providers", appId] }),
        providersApi.updateTrayMenu(),
      ]);
      toast.success(
        t("provider.bulkImport.success", {
          count: result.addedIds.length,
          skipped: result.skipped,
        }),
      );
      if (alive.current) onClose();
    } catch (reason) {
      if (alive.current) {
        setPreview(null);
        setError(
          reason === "provider.bulkImport.errors.previewChanged"
            ? "previewChanged"
            : "storageFailed",
        );
      }
    } finally {
      importLock.current = false;
      if (alive.current) setImporting(false);
    }
  };
  const ready = preview?.token && preview.errors.length === 0;
  return (
    <FullScreenPanel
      isOpen
      trackUnsavedChanges
      title={t("provider.bulkImport.title")}
      subtitle={APP_DISPLAY_NAME[appId]}
      onClose={() => {
        if (!importLock.current) onClose();
      }}
      footer={
        <div className="flex justify-end gap-2">
          {ready && (
            <Button
              variant="outline"
              disabled={importing}
              onClick={() => {
                setPreview(null);
                setError(null);
              }}
            >
              {t("provider.bulkImport.edit")}
            </Button>
          )}
          <Button
            variant="solid"
            disabled={
              loading ||
              importing ||
              (ready ? !preview.newCount : !source.trim())
            }
            onClick={() => void (ready ? commit() : check())}
          >
            {(loading || importing) && (
              <Loader2 className="mr-2 h-4 w-4 animate-spin" />
            )}
            {ready
              ? t("provider.bulkImport.confirm", { count: preview.newCount })
              : t("provider.bulkImport.preview")}
          </Button>
        </div>
      }
    >
      <div className="mx-auto max-w-5xl space-y-5">
        <p className="text-sm text-muted-foreground">
          {t("provider.bulkImport.description")}
        </p>
        {error && (
          <p role="alert" className="text-sm text-destructive">
            {errorText(error, MAX_BYTES)}
          </p>
        )}
        {!ready ? (
          <>
            <div className="flex items-center justify-between gap-3">
              <label
                htmlFor="bulk-provider-source"
                className="text-sm font-medium"
              >
                {t("provider.bulkImport.source")}
              </label>
              <input
                ref={fileInput}
                type="file"
                accept=".json,application/json"
                className="hidden"
                aria-label={t("provider.bulkImport.chooseFile")}
                onChange={(event) => {
                  const file = event.target.files?.[0];
                  event.target.value = "";
                  void loadFile(file);
                }}
              />
              <Button
                variant="outline"
                disabled={loading || importing}
                onClick={() => fileInput.current?.click()}
              >
                <FileUp className="mr-2 h-4 w-4" />
                {t("provider.bulkImport.chooseFile")}
              </Button>
            </div>
            <Textarea
              id="bulk-provider-source"
              className="min-h-[260px] font-mono text-xs"
              placeholder={example}
              value={source}
              maxLength={MAX_BYTES + 1}
              onChange={(event) => changeSource(event.target.value)}
              aria-describedby="bulk-provider-help"
            />
            <p
              id="bulk-provider-help"
              className="text-xs leading-6 text-muted-foreground"
            >
              {t("provider.bulkImport.help", {
                formats: spec.formats.join(", "),
                defaultFormat: spec.defaultFormat,
              })}
              {spec.modelRequired &&
                ` ${t("provider.bulkImport.modelRequired")}`}
              {appId === "kimi" &&
                ` ${t("provider.bulkImport.contextRequired")}`}
              {appId === "claude-desktop" &&
                ` ${t("provider.bulkImport.desktopModels")}`}
              {appId === "omp" && ` ${t("provider.bulkImport.ompKey")}`}
            </p>
            {!!preview?.errors.length && (
              <ul role="alert" className="space-y-1 text-sm text-destructive">
                {preview.errors.map((item, index) => (
                  <li key={index}>
                    {item.row &&
                      `${t("provider.bulkImport.row", { row: item.row })} · `}
                    {item.field && `${item.field}: `}
                    {errorText(item.code, item.limit)}
                  </li>
                ))}
              </ul>
            )}
          </>
        ) : (
          <>
            <div
              role="status"
              className="rounded-lg border bg-muted/30 px-4 py-3 text-sm font-medium"
            >
              {t("provider.bulkImport.summary", {
                count: preview.newCount,
                skipped: preview.duplicateCount,
              })}
            </div>
            {appId === "omp" &&
              preview.rows.some((row) => row.keyMayResolveFromEnvironment) && (
                <div
                  role="note"
                  aria-label={t("provider.bulkImport.ompWarningTitle")}
                  className="rounded-lg border border-amber-500/40 bg-amber-500/10 px-4 py-3 text-sm"
                >
                  <p className="font-medium">
                    {t("provider.bulkImport.ompWarningTitle")}
                  </p>
                  <p className="mt-1 leading-6 text-muted-foreground">
                    {t("provider.bulkImport.ompWarning")}
                  </p>
                </div>
              )}
            <div className="overflow-x-auto rounded-lg border">
              <table className="w-full text-left text-sm">
                <thead className="bg-muted/40 text-xs text-muted-foreground">
                  <tr>
                    {["name", "endpoint", "key", "model", "status"].map(
                      (label) => (
                        <th key={label} className="px-4 py-3 font-medium">
                          {t(`provider.bulkImport.columns.${label}`)}
                        </th>
                      ),
                    )}
                  </tr>
                </thead>
                <tbody className="divide-y">
                  {preview.rows.map((row) => (
                    <tr
                      key={row.row}
                      className={row.duplicate ? "text-muted-foreground" : ""}
                    >
                      <td className="max-w-[180px] break-words px-4 py-4 font-medium">
                        {row.name}
                        <div className="mt-1 text-xs font-normal text-muted-foreground">
                          #{row.row}
                        </div>
                      </td>
                      <td className="max-w-[260px] break-all px-4 py-4">
                        <div>{row.baseUrl}</div>
                        <div className="mt-1 text-xs text-muted-foreground">
                          {row.apiFormat}
                          {row.requiresRouting &&
                            ` · ${t("provider.bulkImport.routing")}`}
                        </div>
                      </td>
                      <td className="whitespace-nowrap px-4 py-4 font-mono text-xs">
                        {row.keyHint}
                        {row.keyMayResolveFromEnvironment && (
                          <div className="mt-1 font-sans text-muted-foreground">
                            {t("provider.bulkImport.keyMayResolve")}
                          </div>
                        )}
                      </td>
                      <td className="max-w-[180px] break-all px-4 py-4 text-xs">
                        {row.model ?? t("provider.bulkImport.defaultModel")}
                        {row.contextWindow && (
                          <div className="mt-1 text-muted-foreground">
                            {t("provider.bulkImport.context", {
                              count: row.contextWindow,
                            })}
                          </div>
                        )}
                      </td>
                      <td className="px-4 py-4 text-xs">
                        {t(
                          `provider.bulkImport.${row.duplicate ? "duplicate" : row.sameName ? "sameName" : "new"}`,
                        )}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </>
        )}
      </div>
    </FullScreenPanel>
  );
}
