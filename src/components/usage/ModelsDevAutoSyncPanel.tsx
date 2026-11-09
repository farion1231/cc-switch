import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "@/lib/toast";
import {
  Check,
  FolderOpen,
  Loader2,
  MoreHorizontal,
  RefreshCw,
  Search,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { HoverTip } from "@/components/ui/hover-tip";
import { ConfirmDialog } from "@/components/ConfirmDialog";
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
import { HelpTip } from "@/components/ui/help-tip";
import { Notice } from "@/components/ui/notice";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { settingsApi } from "@/lib/api/settings";
import { usageApi } from "@/lib/api/usage";
import {
  MODELS_DEV_SYNC_CONFIG_QUERY_KEY,
  syncModelsDevPricing,
} from "@/lib/modelsDevAutoSync";
import { modelsDevQueryOptions } from "@/lib/modelsDev";
import {
  flattenModels,
  formatPrice,
  getCommonModelKeys,
  type ModelsDevEntry,
} from "@/lib/modelsDevPricing";
import { usageKeys } from "@/lib/query/usage";
import { formatRelativeTime } from "./format";
import type { ModelsDevSyncConfig, ModelsDevSyncState } from "@/types/usage";
import { isTextEditableTarget } from "@/utils/domUtils";

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
    ...modelsDevQueryOptions,
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
          <div className="flex items-center justify-between gap-4 rounded-panel border border-border bg-subtle px-3 py-2.5">
            <div>
              <div className="text-body font-medium">
                {t("usage.modelsDevAutoSync.commonModels")}
              </div>
              <div className="text-caption text-fg-2">
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
              <Loader2 className="h-6 w-6 animate-spin text-fg-3" />
            </div>
          ) : error ? (
            <Notice
              tone="danger"
              title={`${t("usage.modelsDevLoadError")}: ${String(error)}`}
              actions={
                <Button
                  variant="neutral"
                  size="compact"
                  onClick={() => refetch()}
                >
                  {t("usage.modelsDevRetry")}
                </Button>
              }
            />
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
                  <Search className="absolute left-2.5 top-1/2 h-4 w-4 -translate-y-1/2 text-fg-2" />
                  <Input
                    value={search}
                    onChange={(event) => setSearch(event.target.value)}
                    placeholder={t("usage.modelsDevSearchPlaceholder")}
                    className="pl-8"
                  />
                </div>
                <Button
                  variant="neutral"
                  size="compact"
                  onClick={selectFiltered}
                  disabled={filtered.length === 0}
                >
                  {t("usage.modelsDevAutoSync.selectFiltered", {
                    count: filtered.length,
                  })}
                </Button>
                <Button
                  variant="neutral"
                  size="compact"
                  onClick={clearFiltered}
                  disabled={filtered.length === 0}
                >
                  {t("usage.modelsDevAutoSync.clearFiltered")}
                </Button>
              </div>

              <div className="flex items-center justify-between text-caption text-fg-2">
                <span>
                  {t("usage.modelsDevAutoSync.selectedCount", {
                    count: effectiveSelectedKeys.size,
                  })}
                </span>
                <span>{t("usage.modelsDevAutoSync.selectionHint")}</span>
              </div>

              <div className="flex-1 min-h-0 overflow-y-auto rounded-panel border border-border">
                {filtered.length === 0 ? (
                  <div className="flex h-full items-center justify-center py-8 text-body text-fg-2">
                    {t("usage.modelsDevNoResults")}
                  </div>
                ) : (
                  <div className="divide-y divide-border">
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
                            selected ? "bg-selected" : "hover:bg-subtle"
                          }`}
                        >
                          <span
                            className={`flex h-4 w-4 shrink-0 items-center justify-center rounded border ${
                              selected
                                ? "border-action bg-action text-action-fg"
                                : "border-border-strong"
                            }`}
                          >
                            {selected && <Check className="h-3 w-3" />}
                          </span>
                          <div className="min-w-0 flex-1">
                            <div className="flex items-center gap-2">
                              <span className="truncate text-body font-medium">
                                {entry.modelName}
                              </span>
                              <span className="shrink-0 text-caption text-fg-2">
                                {entry.providerName}
                              </span>
                              {common && (
                                <span className="rounded bg-subtle px-1.5 text-badge text-fg-2">
                                  {t("usage.modelsDevAutoSync.commonBadge")}
                                </span>
                              )}
                              {entry.releaseDate && (
                                <span className="shrink-0 text-badge text-fg-3">
                                  {entry.releaseDate}
                                </span>
                              )}
                            </div>
                            <div
                              className="truncate font-mono text-caption text-fg-2"
                              title={entry.modelId}
                            >
                              {entry.normalizedId}
                            </div>
                          </div>
                          <div className="flex shrink-0 gap-3 text-right">
                            {priceColumns(entry).map((column) => (
                              <div key={column.label} className="w-16">
                                <div className="text-badge text-fg-2">
                                  {column.label}
                                </div>
                                <div className="font-mono text-caption">
                                  ${formatPrice(column.value)}
                                </div>
                              </div>
                            ))}
                          </div>
                        </button>
                      );
                    })}
                    {filtered.length > visible.length && (
                      <div className="px-3 py-2 text-center text-caption text-fg-2">
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
          <Button
            variant="neutral"
            size="regular"
            onClick={onClose}
            disabled={isSaving}
          >
            {t("common.cancel")}
          </Button>
          <Button
            variant="solid"
            size="regular"
            onClick={save}
            disabled={isSaving || isLoading || !!error}
          >
            {isSaving && <Loader2 className="h-4 w-4 animate-spin" />}
            {t("common.save")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

/**
 * 定价页签第一行（v7 S6）：models.dev 同步状态 + 选择模型 + 立即同步；
 * 开关、本地定价文件（打开目录 / 重新加载）收进右侧和 ⋯ 菜单。
 */
export function ModelsDevAutoSyncPanel() {
  const { t, i18n } = useTranslation();
  const queryClient = useQueryClient();
  const [isDialogOpen, setIsDialogOpen] = useState(false);
  const [isSaving, setIsSaving] = useState(false);
  const [isSyncing, setIsSyncing] = useState(false);
  const [isReloading, setIsReloading] = useState(false);
  const [showEnableConfirm, setShowEnableConfirm] = useState(false);

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
      await Promise.all([
        refetch(),
        queryClient.invalidateQueries({ queryKey: usageKeys.all }),
      ]);
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
      <div className="flex h-[58px] items-center justify-center rounded-panel bg-subtle">
        <Loader2 className="h-4 w-4 animate-spin text-fg-3" />
      </div>
    );
  }

  if (error || !data) {
    return (
      <Notice
        tone="danger"
        title={t("usage.modelsDevAutoSync.configLoadFailed", {
          error: String(error),
        })}
        actions={
          <Button
            type="button"
            variant="neutral"
            size="compact"
            onClick={() => refetch()}
          >
            {t("usage.modelsDevRetry")}
          </Button>
        }
      />
    );
  }

  const enabled = data.config.autoSyncEnabled;
  const lastSync = data.config.lastSyncAt
    ? t("usage.pricing.lastSync", {
        time: formatRelativeTime(data.config.lastSyncAt, t),
      })
    : t("usage.modelsDevAutoSync.neverSynced");
  const lastSyncTitle = data.config.lastSyncAt
    ? new Date(data.config.lastSyncAt).toLocaleString(i18n.resolvedLanguage)
    : undefined;

  const handleAutoSyncChange = (autoSyncEnabled: boolean) => {
    if (autoSyncEnabled) {
      setShowEnableConfirm(true);
      return;
    }
    void saveConfig({ ...data.config, autoSyncEnabled: false });
  };

  return (
    <>
      <div className="flex flex-col gap-2">
        <div className="flex flex-wrap items-center gap-x-3 gap-y-2 rounded-panel bg-subtle px-4 py-2.5">
          <RefreshCw
            aria-hidden="true"
            className="h-4 w-4 shrink-0 text-fg-2"
            strokeWidth={1.5}
          />
          <div className="flex min-w-0 flex-1 flex-col">
            <div className="flex min-w-0 items-center gap-0.5">
              <span className="truncate text-body font-semibold text-fg-1">
                {t("usage.pricing.modelsDevTitle", {
                  status: enabled
                    ? t("usage.modelsDevAutoSync.enabled")
                    : t("usage.modelsDevAutoSync.disabled"),
                })}
              </span>
              <HelpTip title={t("usage.pricing.modelsDevHelpTitle")}>
                {t("usage.pricing.modelsDevHelp")}
              </HelpTip>
            </div>
            <span
              className="truncate text-caption text-fg-2"
              title={lastSyncTitle}
            >
              {data.config.includeCommonModels
                ? `${lastSync} · ${t("usage.pricing.commonIncluded")}`
                : lastSync}
            </span>
          </div>
          <div className="flex shrink-0 items-center gap-2">
            <Switch
              checked={enabled}
              disabled={isSaving}
              onCheckedChange={handleAutoSyncChange}
              aria-label={t("usage.modelsDevAutoSync.title")}
            />
            <Button
              type="button"
              variant="quiet"
              size="compact"
              onClick={() => setIsDialogOpen(true)}
            >
              {t("usage.modelsDevAutoSync.configure")}
            </Button>
            <Button
              type="button"
              variant="neutral"
              size="compact"
              onClick={() => void syncNow()}
              disabled={isSyncing}
            >
              {isSyncing && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
              {t("usage.modelsDevAutoSync.syncNow")}
            </Button>
            <DropdownMenu>
              <HoverTip content={t("common.more")}>
                <DropdownMenuTrigger asChild>
                  <Button
                    type="button"
                    variant="quiet"
                    size="icon-compact"
                    aria-label={t("usage.pricing.moreActions")}
                  >
                    <MoreHorizontal className="h-4 w-4" />
                  </Button>
                </DropdownMenuTrigger>
              </HoverTip>
              <DropdownMenuContent
                align="end"
                className="min-w-[220px] max-w-[320px] rounded-panel border-border bg-surface shadow-v7-md"
              >
                <DropdownMenuLabel className="flex flex-col gap-0.5 font-normal">
                  <span className="text-caption text-fg-2">
                    {t("usage.modelsDevAutoSync.localFile")}
                  </span>
                  <span
                    className="truncate font-mono text-caption text-fg-1"
                    title={data.configPath}
                  >
                    {data.configPath}
                  </span>
                </DropdownMenuLabel>
                <DropdownMenuSeparator />
                <DropdownMenuItem
                  className="text-body"
                  onSelect={() => void openLocalFileFolder()}
                >
                  <FolderOpen className="h-4 w-4" strokeWidth={1.5} />
                  {t("usage.modelsDevAutoSync.openFolder")}
                </DropdownMenuItem>
                <DropdownMenuItem
                  className="text-body"
                  disabled={isReloading}
                  onSelect={() => void reloadLocalFile()}
                >
                  {isReloading ? (
                    <Loader2 className="h-4 w-4 animate-spin" />
                  ) : (
                    <RefreshCw className="h-4 w-4" strokeWidth={1.5} />
                  )}
                  {t("usage.modelsDevAutoSync.reloadLocalFile")}
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          </div>
        </div>

        {data.config.lastSyncError && (
          <Notice
            tone="danger"
            title={t("usage.modelsDevAutoSync.lastError", {
              error: data.config.lastSyncError,
            })}
          />
        )}
      </div>

      {isDialogOpen && (
        <AutoSyncDialog
          state={data}
          onClose={() => setIsDialogOpen(false)}
          onSaved={updateCachedState}
        />
      )}
      <ConfirmDialog
        isOpen={showEnableConfirm}
        title={t("usage.modelsDevAutoSync.enableConfirmTitle")}
        message={t("usage.modelsDevAutoSync.enableConfirmMessage")}
        confirmText={t("usage.modelsDevAutoSync.enableConfirmAction")}
        variant="destructive"
        onConfirm={() => {
          setShowEnableConfirm(false);
          void saveConfig({ ...data.config, autoSyncEnabled: true });
        }}
        onCancel={() => setShowEnableConfirm(false)}
      />
    </>
  );
}
