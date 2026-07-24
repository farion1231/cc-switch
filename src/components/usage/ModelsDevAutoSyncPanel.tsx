import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import {
  Check,
  FolderOpen,
  Loader2,
  RefreshCw,
  Search,
  Settings2,
} from "lucide-react";
import { Alert, AlertDescription } from "@/components/ui/alert";
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
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { settingsApi } from "@/lib/api/settings";
import { usageApi } from "@/lib/api/usage";
import {
  MODELS_DEV_SYNC_CONFIG_QUERY_KEY,
  syncModelsDevPricing,
} from "@/lib/modelsDevAutoSync";
import {
  fetchModelsDevPricing,
  flattenModels,
  formatPrice,
  getCommonModelKeys,
  type ModelsDevEntry,
} from "@/lib/modelsDevPricing";
import { usageKeys } from "@/lib/query/usage";
import type { ModelsDevSyncConfig, ModelsDevSyncState } from "@/types/usage";
import { isTextEditableTarget } from "@/utils/domUtils";

const MODELS_DEV_QUERY_KEY = ["models-dev-pricing"] as const;
const DEFAULT_VISIBLE_ROWS = 80;
const MAX_VISIBLE_ROWS = 300;

interface AutoSyncDialogProps {
  state: ModelsDevSyncState;
  onClose: () => void;
  onSaved: (state: ModelsDevSyncState) => void;
}

function AutoSyncDialog({ state, onClose, onSaved }: AutoSyncDialogProps) {
  const { t } = useTranslation();
  const [search, setSearch] = useState("");
  const [providerFilter, setProviderFilter] = useState("all");
  const [includeCommonModels, setIncludeCommonModels] = useState(
    state.config.includeCommonModels,
  );
  const [selectedModelKeys, setSelectedModelKeys] = useState(
    () => new Set(state.config.selectedModelKeys),
  );
  const [excludedCommonModelKeys, setExcludedCommonModelKeys] = useState(
    () => new Set(state.config.excludedCommonModelKeys),
  );
  const [isSaving, setIsSaving] = useState(false);

  const { data, isLoading, error, refetch } = useQuery({
    queryKey: MODELS_DEV_QUERY_KEY,
    queryFn: fetchModelsDevPricing,
    staleTime: 60 * 60 * 1000,
    retry: 1,
  });
  const entries = useMemo(() => (data ? flattenModels(data) : []), [data]);
  const commonModelKeys = useMemo(() => getCommonModelKeys(entries), [entries]);

  const effectiveSelectedKeys = useMemo(() => {
    const selected = new Set(selectedModelKeys);
    if (includeCommonModels) {
      for (const key of commonModelKeys) {
        if (!excludedCommonModelKeys.has(key)) selected.add(key);
      }
    }
    return selected;
  }, [
    commonModelKeys,
    excludedCommonModelKeys,
    includeCommonModels,
    selectedModelKeys,
  ]);

  const providers = useMemo(() => {
    const map = new Map<string, string>();
    for (const entry of entries) {
      if (!map.has(entry.providerId)) {
        map.set(entry.providerId, entry.providerName);
      }
    }
    return Array.from(map, ([id, name]) => ({ id, name })).sort((a, b) =>
      a.name.localeCompare(b.name),
    );
  }, [entries]);

  const isFiltering = search.trim() !== "" || providerFilter !== "all";
  const filtered = useMemo(() => {
    const query = search.trim().toLowerCase();
    return entries.filter(
      (entry) =>
        (providerFilter === "all" || entry.providerId === providerFilter) &&
        (!query ||
          entry.modelId.toLowerCase().includes(query) ||
          entry.normalizedId.includes(query) ||
          entry.modelName.toLowerCase().includes(query) ||
          entry.providerName.toLowerCase().includes(query)),
    );
  }, [entries, providerFilter, search]);
  const visible = useMemo(
    () =>
      filtered.slice(0, isFiltering ? MAX_VISIBLE_ROWS : DEFAULT_VISIBLE_ROWS),
    [filtered, isFiltering],
  );

  const toggleEntry = (entry: ModelsDevEntry) => {
    const isSelected = effectiveSelectedKeys.has(entry.key);
    setSelectedModelKeys((previous) => {
      const next = new Set(previous);
      if (isSelected) next.delete(entry.key);
      else next.add(entry.key);
      return next;
    });
    setExcludedCommonModelKeys((previous) => {
      const next = new Set(previous);
      if (isSelected && includeCommonModels && commonModelKeys.has(entry.key)) {
        next.add(entry.key);
      } else {
        next.delete(entry.key);
      }
      return next;
    });
  };

  const selectFiltered = () => {
    setSelectedModelKeys((previous) => {
      const next = new Set(previous);
      for (const entry of filtered) next.add(entry.key);
      return next;
    });
    setExcludedCommonModelKeys((previous) => {
      const next = new Set(previous);
      for (const entry of filtered) next.delete(entry.key);
      return next;
    });
  };

  const clearFiltered = () => {
    setSelectedModelKeys((previous) => {
      const next = new Set(previous);
      for (const entry of filtered) next.delete(entry.key);
      return next;
    });
    if (includeCommonModels) {
      setExcludedCommonModelKeys((previous) => {
        const next = new Set(previous);
        for (const entry of filtered) {
          if (commonModelKeys.has(entry.key)) next.add(entry.key);
        }
        return next;
      });
    }
  };

  const save = async () => {
    setIsSaving(true);
    try {
      const config: ModelsDevSyncConfig = {
        ...state.config,
        includeCommonModels,
        selectedModelKeys: Array.from(selectedModelKeys).sort(),
        excludedCommonModelKeys: Array.from(excludedCommonModelKeys).sort(),
      };
      await usageApi.saveModelsDevSyncConfig(config);
      onSaved({ ...state, config });
      toast.success(t("usage.modelsDevAutoSync.selectionSaved"));
      onClose();
    } catch (saveError) {
      toast.error(String(saveError));
    } finally {
      setIsSaving(false);
    }
  };

  const priceColumns = (entry: ModelsDevEntry) =>
    [
      { label: t("usage.inputCost"), value: entry.input },
      { label: t("usage.outputCost"), value: entry.output },
      { label: t("usage.cacheReadCost"), value: entry.cacheRead },
      { label: t("usage.cacheWriteCost"), value: entry.cacheWrite },
    ] as const;

  return (
    <Dialog open onOpenChange={(open) => !open && !isSaving && onClose()}>
      <DialogContent
        zIndex="top"
        className="max-w-4xl h-[84vh]"
        onEscapeKeyDown={(event) => {
          if (isTextEditableTarget(event.target)) event.preventDefault();
        }}
      >
        <DialogHeader>
          <DialogTitle>
            {t("usage.modelsDevAutoSync.configureTitle")}
          </DialogTitle>
          <DialogDescription>
            {t("usage.modelsDevAutoSync.configureDescription")}
          </DialogDescription>
        </DialogHeader>

        <div className="flex flex-1 min-h-0 flex-col gap-3 px-6 py-4">
          <div className="flex items-center justify-between gap-4 rounded-lg border border-border/50 bg-muted/20 px-3 py-2.5">
            <div>
              <div className="text-sm font-medium">
                {t("usage.modelsDevAutoSync.commonModels")}
              </div>
              <div className="text-xs text-muted-foreground">
                {t("usage.modelsDevAutoSync.commonModelsDescription", {
                  count: commonModelKeys.size,
                })}
              </div>
            </div>
            <Switch
              checked={includeCommonModels}
              onCheckedChange={setIncludeCommonModels}
              aria-label={t("usage.modelsDevAutoSync.commonModels")}
            />
          </div>

          {isLoading ? (
            <div className="flex flex-1 items-center justify-center">
              <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
            </div>
          ) : error ? (
            <Alert variant="destructive">
              <AlertDescription className="flex items-center justify-between gap-3">
                <span>
                  {t("usage.modelsDevLoadError")}: {String(error)}
                </span>
                <Button variant="outline" size="sm" onClick={() => refetch()}>
                  {t("usage.modelsDevRetry")}
                </Button>
              </AlertDescription>
            </Alert>
          ) : (
            <>
              <div className="flex items-center gap-2">
                <Select
                  value={providerFilter}
                  onValueChange={setProviderFilter}
                >
                  <SelectTrigger className="w-48 shrink-0">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent className="z-[120] max-h-[min(24rem,var(--radix-select-content-available-height))]">
                    <SelectItem value="all">
                      {t("usage.modelsDevAllProviders")}
                    </SelectItem>
                    {providers.map((provider) => (
                      <SelectItem key={provider.id} value={provider.id}>
                        {provider.name}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
                <div className="relative flex-1">
                  <Search className="absolute left-2.5 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
                  <Input
                    value={search}
                    onChange={(event) => setSearch(event.target.value)}
                    placeholder={t("usage.modelsDevSearchPlaceholder")}
                    className="pl-8"
                  />
                </div>
                <Button
                  variant="outline"
                  size="sm"
                  onClick={selectFiltered}
                  disabled={filtered.length === 0}
                >
                  {t("usage.modelsDevAutoSync.selectFiltered", {
                    count: filtered.length,
                  })}
                </Button>
                <Button
                  variant="outline"
                  size="sm"
                  onClick={clearFiltered}
                  disabled={filtered.length === 0}
                >
                  {t("usage.modelsDevAutoSync.clearFiltered")}
                </Button>
              </div>

              <div className="flex items-center justify-between text-xs text-muted-foreground">
                <span>
                  {t("usage.modelsDevAutoSync.selectedCount", {
                    count: effectiveSelectedKeys.size,
                  })}
                </span>
                <span>{t("usage.modelsDevAutoSync.selectionHint")}</span>
              </div>

              <div className="flex-1 min-h-0 overflow-y-auto rounded-md border border-border/50">
                {filtered.length === 0 ? (
                  <div className="flex h-full items-center justify-center py-8 text-sm text-muted-foreground">
                    {t("usage.modelsDevNoResults")}
                  </div>
                ) : (
                  <div className="divide-y divide-border/30">
                    {visible.map((entry) => {
                      const selected = effectiveSelectedKeys.has(entry.key);
                      const common = commonModelKeys.has(entry.key);
                      return (
                        <button
                          key={entry.key}
                          type="button"
                          aria-pressed={selected}
                          onClick={() => toggleEntry(entry)}
                          className={`flex w-full items-center gap-3 px-3 py-2 text-left ${
                            selected ? "bg-accent/50" : "hover:bg-muted/40"
                          }`}
                        >
                          <span
                            className={`flex h-4 w-4 shrink-0 items-center justify-center rounded border ${
                              selected
                                ? "border-primary bg-primary text-primary-foreground"
                                : "border-muted-foreground/50"
                            }`}
                          >
                            {selected && <Check className="h-3 w-3" />}
                          </span>
                          <div className="min-w-0 flex-1">
                            <div className="flex items-center gap-2">
                              <span className="truncate text-sm font-medium">
                                {entry.modelName}
                              </span>
                              <span className="shrink-0 text-xs text-muted-foreground">
                                {entry.providerName}
                              </span>
                              {common && (
                                <span className="rounded bg-primary/10 px-1.5 py-0.5 text-[10px] text-primary">
                                  {t("usage.modelsDevAutoSync.commonBadge")}
                                </span>
                              )}
                              {entry.releaseDate && (
                                <span className="shrink-0 text-[10px] text-muted-foreground/70">
                                  {entry.releaseDate}
                                </span>
                              )}
                            </div>
                            <div
                              className="truncate font-mono text-xs text-muted-foreground"
                              title={entry.modelId}
                            >
                              {entry.normalizedId}
                            </div>
                          </div>
                          <div className="flex shrink-0 gap-3 text-right">
                            {priceColumns(entry).map((column) => (
                              <div key={column.label} className="w-16">
                                <div className="text-[10px] text-muted-foreground">
                                  {column.label}
                                </div>
                                <div className="font-mono text-xs">
                                  ${formatPrice(column.value)}
                                </div>
                              </div>
                            ))}
                          </div>
                        </button>
                      );
                    })}
                    {filtered.length > visible.length && (
                      <div className="px-3 py-2 text-center text-xs text-muted-foreground">
                        {isFiltering
                          ? t("usage.modelsDevTruncated", {
                              shown: visible.length,
                              total: filtered.length,
                            })
                          : t("usage.modelsDevDefaultHint", {
                              shown: visible.length,
                              total: filtered.length,
                            })}
                      </div>
                    )}
                  </div>
                )}
              </div>
            </>
          )}
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={onClose} disabled={isSaving}>
            {t("common.cancel")}
          </Button>
          <Button onClick={save} disabled={isSaving || isLoading || !!error}>
            {isSaving && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
            {t("common.save")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

export function ModelsDevAutoSyncPanel() {
  const { t, i18n } = useTranslation();
  const queryClient = useQueryClient();
  const [isDialogOpen, setIsDialogOpen] = useState(false);
  const [isSaving, setIsSaving] = useState(false);
  const [isSyncing, setIsSyncing] = useState(false);
  const [isReloading, setIsReloading] = useState(false);

  const { data, isLoading, error, refetch } = useQuery({
    queryKey: MODELS_DEV_SYNC_CONFIG_QUERY_KEY,
    queryFn: usageApi.getModelsDevSyncConfig,
    staleTime: Number.POSITIVE_INFINITY,
  });

  const updateCachedState = (state: ModelsDevSyncState) => {
    queryClient.setQueryData(MODELS_DEV_SYNC_CONFIG_QUERY_KEY, state);
  };

  const saveConfig = async (config: ModelsDevSyncConfig) => {
    if (!data) return;
    setIsSaving(true);
    try {
      await usageApi.saveModelsDevSyncConfig(config);
      updateCachedState({ ...data, config });
    } catch (saveError) {
      toast.error(String(saveError));
    } finally {
      setIsSaving(false);
    }
  };

  const syncNow = async () => {
    if (!data) return;
    setIsSyncing(true);
    try {
      const result = await syncModelsDevPricing(data, true);
      await Promise.all([
        refetch(),
        queryClient.invalidateQueries({ queryKey: usageKeys.all }),
      ]);
      toast.success(
        t("usage.modelsDevAutoSync.syncSuccess", {
          imported: result.imported,
          changed: result.changed,
        }),
      );
    } catch (syncError) {
      await refetch();
      toast.error(
        t("usage.modelsDevAutoSync.syncFailed", { error: String(syncError) }),
      );
    } finally {
      setIsSyncing(false);
    }
  };

  const reloadLocalFile = async () => {
    setIsReloading(true);
    try {
      await usageApi.getModelPricing();
      await queryClient.invalidateQueries({ queryKey: usageKeys.all });
      toast.success(t("usage.modelsDevAutoSync.localFileReloaded"));
    } catch (reloadError) {
      toast.error(
        t("usage.modelsDevAutoSync.localFileReloadFailed", {
          error: String(reloadError),
        }),
      );
    } finally {
      setIsReloading(false);
    }
  };

  const openLocalFileFolder = async () => {
    try {
      await settingsApi.openAppConfigFolder();
    } catch (openError) {
      toast.error(
        t("usage.modelsDevAutoSync.openFolderFailed", {
          error: String(openError),
        }),
      );
    }
  };

  if (isLoading) {
    return (
      <div className="flex items-center justify-center rounded-lg border border-border/50 py-6">
        <Loader2 className="h-5 w-5 animate-spin text-muted-foreground" />
      </div>
    );
  }

  if (error || !data) {
    return (
      <Alert variant="destructive">
        <AlertDescription className="flex items-center justify-between gap-3">
          <span>
            {t("usage.modelsDevAutoSync.configLoadFailed", {
              error: String(error),
            })}
          </span>
          <Button variant="outline" size="sm" onClick={() => refetch()}>
            {t("usage.modelsDevRetry")}
          </Button>
        </AlertDescription>
      </Alert>
    );
  }

  const lastSync = data.config.lastSyncAt
    ? new Date(data.config.lastSyncAt).toLocaleString(i18n.resolvedLanguage)
    : t("usage.modelsDevAutoSync.neverSynced");

  return (
    <>
      <div className="space-y-3 rounded-lg border border-border/50 bg-muted/15 p-4">
        <div className="flex items-start justify-between gap-4">
          <div>
            <h5 className="text-sm font-medium">
              {t("usage.modelsDevAutoSync.title")}
            </h5>
            <p className="mt-0.5 text-xs text-muted-foreground">
              {t("usage.modelsDevAutoSync.description")}
            </p>
          </div>
          <div className="flex items-center gap-2">
            <span className="text-xs text-muted-foreground">
              {data.config.autoSyncEnabled
                ? t("usage.modelsDevAutoSync.enabled")
                : t("usage.modelsDevAutoSync.disabled")}
            </span>
            <Switch
              checked={data.config.autoSyncEnabled}
              disabled={isSaving}
              onCheckedChange={(autoSyncEnabled) =>
                void saveConfig({ ...data.config, autoSyncEnabled })
              }
              aria-label={t("usage.modelsDevAutoSync.title")}
            />
          </div>
        </div>

        <div className="grid gap-2 text-xs text-muted-foreground md:grid-cols-2">
          <div>
            {t("usage.modelsDevAutoSync.lastSync")}: {lastSync}
          </div>
          <div>
            {t("usage.modelsDevAutoSync.commonStatus")}:{" "}
            {data.config.includeCommonModels
              ? t("usage.modelsDevAutoSync.enabled")
              : t("usage.modelsDevAutoSync.disabled")}
          </div>
        </div>

        {data.config.lastSyncError && (
          <Alert variant="destructive">
            <AlertDescription>
              {t("usage.modelsDevAutoSync.lastError", {
                error: data.config.lastSyncError,
              })}
            </AlertDescription>
          </Alert>
        )}

        <div className="rounded-md bg-background/60 px-3 py-2">
          <div className="text-[11px] text-muted-foreground">
            {t("usage.modelsDevAutoSync.localFile")}
          </div>
          <div className="truncate font-mono text-xs" title={data.configPath}>
            {data.configPath}
          </div>
        </div>

        <div className="flex flex-wrap justify-end gap-2">
          <Button
            variant="outline"
            size="sm"
            onClick={() => void openLocalFileFolder()}
          >
            <FolderOpen className="mr-1.5 h-3.5 w-3.5" />
            {t("usage.modelsDevAutoSync.openFolder")}
          </Button>
          <Button
            variant="outline"
            size="sm"
            onClick={() => void reloadLocalFile()}
            disabled={isReloading}
          >
            {isReloading ? (
              <Loader2 className="mr-1.5 h-3.5 w-3.5 animate-spin" />
            ) : (
              <RefreshCw className="mr-1.5 h-3.5 w-3.5" />
            )}
            {t("usage.modelsDevAutoSync.reloadLocalFile")}
          </Button>
          <Button
            variant="outline"
            size="sm"
            onClick={() => setIsDialogOpen(true)}
          >
            <Settings2 className="mr-1.5 h-3.5 w-3.5" />
            {t("usage.modelsDevAutoSync.configure")}
          </Button>
          <Button size="sm" onClick={() => void syncNow()} disabled={isSyncing}>
            {isSyncing ? (
              <Loader2 className="mr-1.5 h-3.5 w-3.5 animate-spin" />
            ) : (
              <RefreshCw className="mr-1.5 h-3.5 w-3.5" />
            )}
            {t("usage.modelsDevAutoSync.syncNow")}
          </Button>
        </div>
      </div>

      {isDialogOpen && (
        <AutoSyncDialog
          state={data}
          onClose={() => setIsDialogOpen(false)}
          onSaved={updateCachedState}
        />
      )}
    </>
  );
}
