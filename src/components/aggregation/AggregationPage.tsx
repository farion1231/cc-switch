import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Check,
  ChevronDown,
  ChevronRight,
  Copy,
  Edit2,
  Layers,
  Loader2,
  Plus,
  Search,
  Settings2,
  SlidersHorizontal,
  Trash2,
  X,
} from "lucide-react";
import { toast } from "@/lib/toast";
import type { UniversalProvider } from "@/types";
import {
  aggregationApi,
  type AggregationAppProviderItem,
  type AggregationAppState,
} from "@/lib/api/aggregation";
import type { AppId } from "@/lib/api";
import { proxyKeys, useProxyStack } from "@/lib/query/proxy";
import { AppPageHeader } from "@/components/shell/AppPageHeader";
import { APP_DISPLAY_NAME, AppGlyph } from "@/components/shell/AppGlyph";
import { useVisibleAppIds } from "@/components/mcp/useVisibleAppIds";
import { ProviderIcon } from "@/components/ProviderIcon";
import { StackSettingsSheet } from "@/components/providers/mode/StackSettingsSheet";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { HelpTip } from "@/components/ui/help-tip";
import { HoverTip } from "@/components/ui/hover-tip";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { extractErrorMessage } from "@/utils/errorUtils";
import { deepClone } from "@/utils/deepClone";
import { appSupportsOfficialLogin } from "@/utils/providerCapabilities";
import { cn } from "@/lib/utils";
import { AggregationProviderModal } from "./AggregationProviderModal";
import {
  COMPATIBLE_APP_IDS,
  getVisibleEnabledUniversalApps,
} from "./aggregationApps";
import {
  type AggregationPresetEntry,
  extractHost,
  getFeaturedAggregationPresets,
  maskSecretKey,
} from "./aggregationPresets";

interface AggregationPageProps {
  onOpenApp: (app: AppId) => void;
  onOpenRoutingSettings: () => void;
}

const FORMAT_LABELS: Record<string, string> = {
  anthropic: "Anthropic",
  openai_chat: "OpenAI",
  openai_responses: "Responses",
  gemini_native: "Gemini",
  official: "Official",
};

function isLocalUrl(url?: string): boolean {
  if (!url) return false;
  const lower = url.toLowerCase();
  return (
    lower.startsWith("http://localhost") ||
    lower.startsWith("https://localhost") ||
    lower.startsWith("http://127.0.0.1") ||
    lower.startsWith("https://127.0.0.1")
  );
}

interface DeleteTarget {
  sourceApp: AppId | null;
  providerId: string;
  name: string;
}

export function AggregationPage({
  onOpenApp,
  onOpenRoutingSettings,
}: AggregationPageProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const visibleAppIds = useVisibleAppIds(COMPATIBLE_APP_IDS);
  const visibleSet = useMemo(
    () => new Set<string>(visibleAppIds),
    [visibleAppIds],
  );
  const featuredPresets = useMemo(() => getFeaturedAggregationPresets(), []);

  const { data: overview, isLoading } = useQuery({
    queryKey: ["aggregation", "overview"],
    queryFn: () => aggregationApi.getOverview(),
  });
  const { data: codexStack } = useProxyStack("codex", visibleSet.has("codex"));

  const [searchOpen, setSearchOpen] = useState(false);
  const [searchTerm, setSearchTerm] = useState("");
  const [appFilter, setAppFilter] = useState<string>("all");
  const [togglingMode, setTogglingMode] = useState(false);
  const [assigningKey, setAssigningKey] = useState<string | null>(null);
  const [assignDraft, setAssignDraft] = useState<AppId[]>([]);
  const [savingAssignKey, setSavingAssignKey] = useState<string | null>(null);
  const [appModeBusy, setAppModeBusy] = useState<string | null>(null);
  const [modalOpen, setModalOpen] = useState(false);
  const [editingUniversal, setEditingUniversal] =
    useState<UniversalProvider | null>(null);
  const [editingAppProvider, setEditingAppProvider] =
    useState<AggregationAppProviderItem | null>(null);
  const [initialPreset, setInitialPreset] =
    useState<AggregationPresetEntry | null>(null);
  const [secondaryFolded, setSecondaryFolded] = useState(true);
  const [codexStackSettingsOpen, setCodexStackSettingsOpen] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<DeleteTarget | null>(null);

  const invalidateAll = async () => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: ["aggregation"] }),
      queryClient.invalidateQueries({ queryKey: ["settings"] }),
      queryClient.invalidateQueries({ queryKey: proxyKeys.status }),
      queryClient.invalidateQueries({ queryKey: proxyKeys.takeoverStatus }),
      queryClient.invalidateQueries({ queryKey: ["app-mode"] }),
      queryClient.invalidateQueries({ queryKey: ["proxy-stack"] }),
      queryClient.invalidateQueries({ queryKey: ["providers"] }),
      queryClient.invalidateQueries({ queryKey: ["universal-providers"] }),
      queryClient.invalidateQueries({ queryKey: ["pi", "currentState"] }),
      queryClient.invalidateQueries({ queryKey: ["omp", "currentState"] }),
      queryClient.invalidateQueries({ queryKey: ["opencodeLiveProviderIds"] }),
      queryClient.invalidateQueries({
        queryKey: ["openclaw", "liveProviderIds"],
      }),
      queryClient.invalidateQueries({ queryKey: ["hermes"] }),
    ]);
  };

  const handleToggleGlobalMode = async (enabled: boolean) => {
    setTogglingMode(true);
    try {
      await aggregationApi.setGlobalMode(enabled);
      await invalidateAll();
      toast.success(
        enabled
          ? t("aggregation.toast.enabled", {
              defaultValue: "已开启全局聚合模式",
            })
          : t("aggregation.toast.disabled", {
              defaultValue: "已关闭全局聚合模式",
            }),
      );
    } catch (error) {
      toast.error(
        t("aggregation.toast.toggleFailed", {
          defaultValue: "切换聚合模式失败",
        }),
        { description: extractErrorMessage(error) || undefined },
      );
    } finally {
      setTogglingMode(false);
    }
  };

  const handleSaveAndSyncUniversal = async (provider: UniversalProvider) => {
    try {
      const result = await aggregationApi.saveAndSyncUniversal(provider);
      await invalidateAll();
      toast.success(
        result.syncedApps.length > 0
          ? t("aggregation.toast.appliedToApps", {
              count: result.syncedApps.length,
              defaultValue: `已保存并应用到 ${result.syncedApps.length} 个指定应用`,
            })
          : t("aggregation.toast.savedUnassigned", {
              defaultValue: "已保存外部供应商（暂未分配应用）",
            }),
      );
      if (result.warnings.length > 0) {
        toast.warning(result.warnings.slice(0, 3).join("；"));
      }
    } catch (error) {
      toast.error(
        t("aggregation.toast.syncFailed", { defaultValue: "保存同步失败" }),
        { description: extractErrorMessage(error) || undefined },
      );
    }
  };

  const handlePromoteAppProvider = async (
    sourceApp: AppId,
    sourceProviderId: string,
    provider: UniversalProvider,
  ) => {
    try {
      const result = await aggregationApi.promoteAppProviderToUniversal(
        sourceApp,
        sourceProviderId,
        provider,
      );
      await invalidateAll();
      toast.success(
        result.syncedApps.length > 0
          ? t("aggregation.toast.appliedToApps", {
              count: result.syncedApps.length,
              defaultValue: `已保存并应用到 ${result.syncedApps.length} 个指定应用`,
            })
          : t("aggregation.toast.savedUnassigned", {
              defaultValue: "已保存外部供应商（暂未分配应用）",
            }),
      );
      if (result.warnings.length > 0) {
        toast.warning(result.warnings.slice(0, 3).join("；"));
      }
    } catch (error) {
      toast.error(
        t("aggregation.toast.syncFailed", { defaultValue: "保存同步失败" }),
        { description: extractErrorMessage(error) || undefined },
      );
    }
  };

  const openAssignPanel = (key: string, currentApps: AppId[]) => {
    if (assigningKey === key) {
      setAssigningKey(null);
      return;
    }
    setAssigningKey(key);
    setAssignDraft(currentApps);
  };

  const toggleDraftApp = (appId: AppId) => {
    setAssignDraft((prev) =>
      prev.includes(appId)
        ? prev.filter((id) => id !== appId)
        : [...prev, appId],
    );
  };

  const handleSaveAssignment = async (
    sourceApp: AppId | null,
    providerId: string,
    key: string,
  ) => {
    setSavingAssignKey(key);
    try {
      const result = await aggregationApi.setProviderApps(
        sourceApp,
        providerId,
        assignDraft,
      );
      await invalidateAll();
      setAssigningKey(null);
      toast.success(
        assignDraft.length > 0
          ? t("aggregation.toast.assignedApps", {
              count: assignDraft.length,
              defaultValue: `已更新应用分配（${assignDraft.length} 个应用）`,
            })
          : t("aggregation.toast.clearedApps", {
              defaultValue: "已取消所有应用分配",
            }),
      );
      if (result.warnings.length > 0) {
        toast.warning(result.warnings.slice(0, 3).join("；"));
      }
    } catch (error) {
      toast.error(
        t("aggregation.toast.assignFailed", {
          defaultValue: "更新应用分配失败",
        }),
        { description: extractErrorMessage(error) || undefined },
      );
    } finally {
      setSavingAssignKey(null);
    }
  };

  const handleDuplicateUniversal = async (provider: UniversalProvider) => {
    const copy: UniversalProvider = {
      ...deepClone(provider),
      id: crypto.randomUUID(),
      name: `${provider.name} copy`,
      createdAt: Date.now(),
    };
    await handleSaveAndSyncUniversal(copy);
  };

  const handleConfirmDelete = async () => {
    if (!deleteTarget) return;
    try {
      const result = await aggregationApi.deleteProvider(
        deleteTarget.sourceApp,
        deleteTarget.providerId,
      );
      await invalidateAll();
      toast.success(
        t("aggregation.toast.deleted", { defaultValue: "外部供应商已删除" }),
      );
      if (result.warnings.length > 0) {
        toast.warning(result.warnings.slice(0, 3).join("；"));
      }
    } catch (error) {
      toast.error(
        t("aggregation.toast.deleteFailed", {
          defaultValue: "删除外部供应商失败",
        }),
        { description: extractErrorMessage(error) || undefined },
      );
    } finally {
      setDeleteTarget(null);
    }
  };

  const handleToggleAppMode = async (
    appState: AggregationAppState,
    currentlyAttached: boolean,
  ) => {
    const entering = !currentlyAttached;
    setAppModeBusy(appState.appType);
    try {
      await aggregationApi.setAppMode(
        appState.appType,
        entering,
        appState.routeProviderId ?? null,
      );
      if (!entering && appFilter === appState.appType) {
        setAppFilter("all");
      }
      await invalidateAll();
      toast.success(
        entering
          ? t("aggregation.toast.appAttached", {
              app: APP_DISPLAY_NAME[appState.appType],
              defaultValue: `${APP_DISPLAY_NAME[appState.appType]} 已接入聚合`,
            })
          : appState.supportsLocalProxy &&
              appSupportsOfficialLogin(appState.appType)
            ? t("aggregation.toast.appDetached", {
                app: APP_DISPLAY_NAME[appState.appType],
                defaultValue: `${APP_DISPLAY_NAME[appState.appType]} 已回到直连`,
              })
            : t("aggregation.toast.appDisconnected", {
                app: APP_DISPLAY_NAME[appState.appType],
                defaultValue: `${APP_DISPLAY_NAME[appState.appType]} 已取消接入`,
              }),
      );
    } catch (error) {
      toast.error(
        t("mode.toast.failed", {
          detail: extractErrorMessage(error) || t("common.unknown"),
        }),
      );
    } finally {
      setAppModeBusy(null);
    }
  };

  const universalProviders = overview?.universalProviders ?? [];
  // 聚合页仅管理外部供应商，排除官方登录账号
  const externalAppProviders = useMemo(
    () =>
      (overview?.appProviders ?? [])
        .filter((item) => !item.isOfficial && visibleSet.has(item.appType))
        .map((item) => ({
          ...item,
          syncedApps: item.syncedApps.filter((a) => visibleSet.has(a)),
        })),
    [overview?.appProviders, visibleSet],
  );
  const appStates = useMemo(
    () =>
      (overview?.appStates ?? []).filter((appState) =>
        visibleSet.has(appState.appType),
      ),
    [overview?.appStates, visibleSet],
  );

  const isItemReady = (item: AggregationAppProviderItem) =>
    item.canSync ?? Boolean(item.baseUrl);

  const readyAppProviders = useMemo(
    () => externalAppProviders.filter(isItemReady),
    [externalAppProviders],
  );
  const incompleteAppProviders = useMemo(
    () => externalAppProviders.filter((item) => !isItemReady(item)),
    [externalAppProviders],
  );

  // 为每个外部供应商计算它当前指定的可见应用列表
  const universalAssignedMap = useMemo(() => {
    const map = new Map<string, AppId[]>();
    for (const u of universalProviders) {
      map.set(u.id, getVisibleEnabledUniversalApps(u.apps, visibleAppIds));
    }
    return map;
  }, [universalProviders, visibleAppIds]);

  // 统计每个应用被分配了多少外部供应商（用于按应用筛选）
  const assignedCountsByApp = useMemo(() => {
    const map = new Map<AppId, number>();
    for (const u of universalProviders) {
      for (const appId of universalAssignedMap.get(u.id) ?? []) {
        map.set(appId, (map.get(appId) ?? 0) + 1);
      }
    }
    for (const item of readyAppProviders) {
      for (const rawApp of item.syncedApps) {
        const appId = rawApp as AppId;
        if (visibleSet.has(appId)) {
          map.set(appId, (map.get(appId) ?? 0) + 1);
        }
      }
    }
    return map;
  }, [universalProviders, universalAssignedMap, readyAppProviders, visibleSet]);

  const unassignedCount = useMemo(() => {
    let count = 0;
    for (const u of universalProviders) {
      if ((universalAssignedMap.get(u.id) ?? []).length === 0) {
        count += 1;
      }
    }
    for (const item of readyAppProviders) {
      if (item.syncedApps.length === 0) {
        count += 1;
      }
    }
    return count;
  }, [universalProviders, universalAssignedMap, readyAppProviders]);

  const activeFilter =
    appFilter !== "all" &&
    appFilter !== "unassigned" &&
    (!visibleSet.has(appFilter) ||
      (assignedCountsByApp.get(appFilter as AppId) ?? 0) === 0)
      ? "all"
      : appFilter;

  const needle = searchTerm.trim().toLowerCase();

  const filteredUniversal = useMemo(() => {
    return universalProviders.filter((u) => {
      const assigned = universalAssignedMap.get(u.id) ?? [];
      if (activeFilter === "unassigned" && assigned.length > 0) return false;
      if (
        activeFilter !== "all" &&
        activeFilter !== "unassigned" &&
        !assigned.includes(activeFilter as AppId)
      ) {
        return false;
      }
      if (!needle) return true;
      return (
        u.name.toLowerCase().includes(needle) ||
        u.baseUrl.toLowerCase().includes(needle) ||
        (u.notes ?? "").toLowerCase().includes(needle) ||
        assigned.some((a) =>
          APP_DISPLAY_NAME[a].toLowerCase().includes(needle),
        )
      );
    });
  }, [universalProviders, universalAssignedMap, activeFilter, needle]);

  const matchesAppItem = (item: AggregationAppProviderItem) => {
    if (activeFilter === "unassigned" && item.syncedApps.length > 0) {
      return false;
    }
    if (
      activeFilter !== "all" &&
      activeFilter !== "unassigned" &&
      !item.syncedApps.includes(activeFilter)
    ) {
      return false;
    }
    if (!needle) return true;
    return (
      item.provider.name.toLowerCase().includes(needle) ||
      (item.baseUrl ?? "").toLowerCase().includes(needle) ||
      item.syncedApps.some((a) =>
        (APP_DISPLAY_NAME[a as AppId] ?? a).toLowerCase().includes(needle),
      ) ||
      item.models.some((m) => m.toLowerCase().includes(needle))
    );
  };

  const filteredReadyAppProviders = useMemo(
    () => readyAppProviders.filter(matchesAppItem),
    [readyAppProviders, activeFilter, needle],
  );

  const filteredIncompleteAppProviders = useMemo(
    () => incompleteAppProviders.filter(matchesAppItem),
    [incompleteAppProviders, activeFilter, needle],
  );

  const readyTotalCount = universalProviders.length + readyAppProviders.length;

  const openModalWithPreset = (preset: AggregationPresetEntry | null) => {
    setEditingUniversal(null);
    setEditingAppProvider(null);
    setInitialPreset(preset);
    setModalOpen(true);
  };

  const renderInlineAssignEditor = (
    key: string,
    sourceApp: AppId | null,
    providerId: string,
  ) => {
    const allSelected =
      visibleAppIds.length > 0 && assignDraft.length === visibleAppIds.length;
    const isSaving = savingAssignKey === key;

    return (
      <div className="border-t border-border/70 bg-subtle/35 px-4 py-3 space-y-2.5">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <span className="text-caption font-medium text-fg-2">
            {t("aggregation.assignAppsTitle", {
              count: assignDraft.length,
              total: visibleAppIds.length,
              defaultValue: `选择使用此供应商的应用 (${assignDraft.length}/${visibleAppIds.length})`,
            })}
          </span>
          <div className="flex items-center gap-1.5">
            <Button
              type="button"
              variant="quiet"
              size="compact"
              onClick={() =>
                setAssignDraft(allSelected ? [] : [...visibleAppIds])
              }
            >
              {allSelected
                ? t("providerForm.modelPicker.selectNone", {
                    defaultValue: "清空",
                  })
                : t("providerForm.modelPicker.selectAll", {
                    defaultValue: "全选",
                  })}
            </Button>
          </div>
        </div>

        <div className="grid grid-cols-2 gap-1.5 sm:grid-cols-3 md:grid-cols-4">
          {visibleAppIds.map((appId) => {
            const checked = assignDraft.includes(appId);
            return (
              <button
                key={appId}
                type="button"
                aria-pressed={checked}
                onClick={() => toggleDraftApp(appId)}
                className={cn(
                  "flex items-center gap-2 rounded-control border px-2.5 py-1.5 text-start text-caption transition-colors",
                  checked
                    ? "border-stack-border bg-stack-soft text-fg-1 font-medium"
                    : "border-border bg-surface text-fg-3 hover:text-fg-2",
                )}
              >
                <AppGlyph
                  app={appId}
                  size={13}
                  badgeClassName="bg-transparent"
                />
                <span className="min-w-0 flex-1 truncate">
                  {APP_DISPLAY_NAME[appId]}
                </span>
                {checked && (
                  <Check className="h-3.5 w-3.5 shrink-0 text-stack-text" />
                )}
              </button>
            );
          })}
        </div>

        <div className="flex items-center justify-end gap-2 pt-1">
          <Button
            type="button"
            variant="quiet"
            size="compact"
            disabled={isSaving}
            onClick={() => setAssigningKey(null)}
          >
            {t("common.cancel")}
          </Button>
          <Button
            type="button"
            variant="solid"
            size="compact"
            disabled={isSaving}
            onClick={() =>
              void handleSaveAssignment(sourceApp, providerId, key)
            }
          >
            {isSaving && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
            {t("aggregation.saveAssignment", { defaultValue: "保存分配" })}
          </Button>
        </div>
      </div>
    );
  };

  return (
    <>
      <AppPageHeader
        icon={<Layers className="h-5 w-5" strokeWidth={1.5} />}
        title={t("aggregation.title", { defaultValue: "聚合" })}
        titleExtra={
          <>
            <HelpTip title={t("aggregation.title", { defaultValue: "聚合" })}>
              {t("aggregation.help", {
                defaultValue:
                  "所有外部供应商统一在此添加与配置，并可为每个供应商单独指定适用应用；各应用页面仅保留直连官方登录。",
              })}
            </HelpTip>
            <Badge
              variant="outline"
              className={cn(
                "ms-1 h-5 px-2 text-badge font-medium",
                overview?.enabled
                  ? "border-stack-border bg-stack-soft text-stack-text"
                  : "text-fg-3",
              )}
            >
              {overview?.enabled
                ? t("aggregation.statusOn", { defaultValue: "聚合已开启" })
                : t("aggregation.statusOff", { defaultValue: "聚合未开启" })}
            </Badge>
          </>
        }
        actions={
          <>
            <HoverTip
              content={t("aggregation.searchTip", {
                defaultValue: "按名称、地址、模型或应用筛选",
              })}
            >
              <Button
                variant="quiet"
                size="icon-compact"
                className="h-8 w-8"
                aria-label={t("provider.searchAriaLabel")}
                aria-pressed={searchOpen}
                onClick={() => setSearchOpen((v) => !v)}
              >
                <Search className="h-4 w-4" />
              </Button>
            </HoverTip>
            <Button
              variant="solid"
              size="regular"
              onClick={() => openModalWithPreset(null)}
            >
              <Plus className="h-4 w-4" />
              {t("aggregation.addProvider", {
                defaultValue: "添加外部供应商",
              })}
            </Button>
          </>
        }
      />

      <div
        id="main-content"
        className="min-h-0 flex-1 overflow-y-auto scroll-stable px-6 pb-12 pt-4"
      >
        <div className="mx-auto max-w-5xl space-y-4">
          {/* 1. 顶部控制卡：全局开关 + 代理状态 + 各应用分配与接入状态 */}
          <div className="rounded-panel border border-border bg-surface p-4 shadow-v7-sm">
            <div className="flex flex-wrap items-center justify-between gap-4">
              <div className="min-w-0 flex-1 space-y-1">
                <div className="flex items-center gap-2">
                  <span className="text-body font-semibold text-fg-1">
                    {t("aggregation.enableMode", {
                      defaultValue: "开启聚合模式",
                    })}
                  </span>
                  {overview?.proxyRunning ? (
                    <button
                      type="button"
                      onClick={onOpenRoutingSettings}
                      className="inline-flex items-center gap-1 rounded-full bg-subtle px-2 py-0.5 text-caption text-fg-2 hover:text-fg-1"
                    >
                      <span className="h-1.5 w-1.5 rounded-full bg-route" />
                      {overview.proxyAddress}:{overview.proxyPort}
                    </button>
                  ) : (
                    <button
                      type="button"
                      onClick={onOpenRoutingSettings}
                      className="inline-flex items-center gap-1 rounded-full bg-subtle px-2 py-0.5 text-caption text-fg-3 hover:text-fg-1"
                    >
                      <span className="h-1.5 w-1.5 rounded-full bg-fg-3" />
                      {t("routingSettings.stopped")}
                    </button>
                  )}
                </div>
                <p className="text-caption text-fg-2">
                  {t("aggregation.enableModeHelp", {
                    defaultValue:
                      "所有外部供应商统一在此添加和配置，按每个供应商勾选的目标应用精准同步并转发；各应用页面仅保留官方直连登录。",
                  })}
                </p>
              </div>
              <div className="flex items-center gap-3">
                {visibleSet.has("codex") && (
                  <Button
                    variant="quiet"
                    size="compact"
                    onClick={() => setCodexStackSettingsOpen(true)}
                  >
                    <Settings2 className="h-3.5 w-3.5" />
                    {t("aggregation.codexStackSettings", {
                      defaultValue: "Codex 聚合设置",
                    })}
                  </Button>
                )}
                <Switch
                  checked={Boolean(overview?.enabled)}
                  disabled={togglingMode || isLoading}
                  onCheckedChange={(checked) =>
                    void handleToggleGlobalMode(checked)
                  }
                  aria-label={t("aggregation.enableMode", {
                    defaultValue: "开启聚合模式",
                  })}
                />
              </div>
            </div>

            {appStates.length > 0 && (
              <div className="mt-3.5 border-t border-border pt-3">
                <div className="mb-2 text-[11px] font-medium text-fg-3">
                  {t("aggregation.appsStatusTitle", {
                    defaultValue: "各应用外部供应商分配与接入状态",
                  })}
                </div>
                <div className="flex flex-wrap gap-1.5">
                  {appStates.map((appState) => {
                    const assignedCount =
                      appState.assignedProviderCount ??
                      assignedCountsByApp.get(appState.appType) ??
                      appState.syncedProviderCount;
                    const isProxyAttached = appState.supportsStackCatalog
                      ? appState.mode === "stack"
                      : appState.supportsLocalProxy
                        ? appState.mode !== "direct"
                        : Boolean(overview?.enabled && assignedCount > 0);
                    const busy = appModeBusy === appState.appType;
                    const canAttach =
                      isProxyAttached ||
                      (Boolean(overview?.enabled) && assignedCount > 0);
                    return (
                      <div
                        key={appState.appType}
                        className={cn(
                          "inline-flex items-center gap-1.5 rounded-control border px-2 py-1 text-caption",
                          isProxyAttached
                            ? "border-stack-border bg-stack-soft text-fg-1"
                            : "border-border bg-app text-fg-2",
                        )}
                      >
                        <button
                          type="button"
                          onClick={() => onOpenApp(appState.appType)}
                          className="inline-flex items-center gap-1.5 hover:underline"
                        >
                          <AppGlyph
                            app={appState.appType}
                            size={13}
                            badgeClassName="bg-transparent"
                          />
                          <span className="font-medium">
                            {APP_DISPLAY_NAME[appState.appType]}
                          </span>
                        </button>
                        <span className="text-[11px] text-fg-3">
                          {assignedCount > 0
                            ? t("aggregation.appAssignedCount", {
                                count: assignedCount,
                                defaultValue: `${assignedCount} 家`,
                              })
                            : appSupportsOfficialLogin(appState.appType)
                              ? t("aggregation.appUnassigned", {
                                  defaultValue: "官方直连",
                                })
                              : t("aggregation.appUnassignedExternal", {
                                  defaultValue: "未分配",
                                })}
                        </span>
                        {appState.supportsLocalProxy && canAttach ? (
                          <button
                            type="button"
                            disabled={busy}
                            title={
                              isProxyAttached
                                ? t("aggregation.detachAppTip", {
                                    defaultValue: "点击取消接入",
                                  })
                                : undefined
                            }
                            onClick={() =>
                              void handleToggleAppMode(appState, isProxyAttached)
                            }
                            className={cn(
                              "rounded px-1.5 py-0.5 text-[11px] font-medium transition-colors",
                              isProxyAttached
                                ? "bg-stack text-stack-on hover:opacity-90"
                                : "bg-subtle text-fg-2 hover:bg-selected hover:text-fg-1",
                            )}
                          >
                            {busy
                              ? "…"
                              : isProxyAttached
                                ? t("aggregation.appAttached", {
                                    defaultValue: "已接入",
                                  })
                                : t("aggregation.attachApp", {
                                    defaultValue: "接入",
                                  })}
                          </button>
                        ) : !appState.supportsLocalProxy && isProxyAttached ? (
                          <button
                            type="button"
                            disabled={busy}
                            title={t("aggregation.detachAppTip", {
                              defaultValue: "点击取消接入",
                            })}
                            onClick={() =>
                              void handleToggleAppMode(appState, true)
                            }
                            className="rounded bg-stack px-1.5 py-0.5 text-[11px] font-medium text-stack-on transition-opacity hover:opacity-90"
                          >
                            {busy
                              ? "…"
                              : t("aggregation.appAttached", {
                                  defaultValue: "已接入",
                                })}
                          </button>
                        ) : null}
                      </div>
                    );
                  })}
                </div>
              </div>
            )}
          </div>

          {/* 2. 按指定应用筛选与搜索框 */}
          <div className="flex flex-wrap items-center justify-between gap-2">
            <div className="flex flex-wrap items-center gap-1.5">
              <FilterPill
                active={activeFilter === "all"}
                label={t("aggregation.filterAll", {
                  count: readyTotalCount,
                  defaultValue: `外部供应商 (${readyTotalCount})`,
                })}
                onClick={() => setAppFilter("all")}
              />
              {visibleAppIds
                .filter((appId) => (assignedCountsByApp.get(appId) ?? 0) > 0)
                .map((appId) => (
                  <FilterPill
                    key={appId}
                    active={activeFilter === appId}
                    label={`${APP_DISPLAY_NAME[appId]} (${assignedCountsByApp.get(appId) ?? 0})`}
                    onClick={() => setAppFilter(appId)}
                  />
                ))}
              {unassignedCount > 0 && (
                <FilterPill
                  active={activeFilter === "unassigned"}
                  label={t("aggregation.filterUnassigned", {
                    count: unassignedCount,
                    defaultValue: `未分配 (${unassignedCount})`,
                  })}
                  onClick={() => setAppFilter("unassigned")}
                />
              )}
            </div>

            {searchOpen && (
              <div className="relative w-60">
                <Search className="pointer-events-none absolute start-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-fg-3" />
                <Input
                  autoFocus
                  value={searchTerm}
                  onChange={(e) => setSearchTerm(e.target.value)}
                  placeholder={t("provider.searchPlaceholder", {
                    defaultValue: "搜索名称、地址或模型...",
                  })}
                  className="h-8 ps-8 pe-7 text-caption"
                />
                {searchTerm && (
                  <button
                    type="button"
                    onClick={() => setSearchTerm("")}
                    className="absolute end-2 top-1/2 -translate-y-1/2 text-fg-3 hover:text-fg-1"
                  >
                    <X className="h-3.5 w-3.5" />
                  </button>
                )}
              </div>
            )}
          </div>

          {/* 3. 外部供应商列表（每个供应商可单独指定适用应用） */}
          {isLoading ? (
            <div className="flex items-center justify-center py-16 text-fg-3">
              <Loader2 className="h-5 w-5 animate-spin" />
            </div>
          ) : (
            <div className="space-y-4">
              {filteredUniversal.length === 0 &&
              filteredReadyAppProviders.length === 0 ? (
                <div className="rounded-panel border border-dashed border-border bg-surface/50 px-6 py-10 text-center">
                  <Layers className="mx-auto mb-2.5 h-8 w-8 text-fg-3" />
                  <p className="text-body font-medium text-fg-1">
                    {t("aggregation.emptyTitle", {
                      defaultValue: "暂无外部供应商",
                    })}
                  </p>
                  <p className="mt-1 text-caption text-fg-3">
                    {t("aggregation.emptyHint", {
                      defaultValue:
                        "选择一个常用预设并填入 API Key，再指定需要使用该供应商的应用即可。",
                    })}
                  </p>
                  {featuredPresets.length > 0 && (
                    <div className="mx-auto mt-5 flex max-w-2xl flex-wrap justify-center gap-2">
                      {featuredPresets.map((preset) => (
                        <button
                          key={preset.id}
                          type="button"
                          onClick={() => openModalWithPreset(preset)}
                          className="inline-flex items-center gap-2 rounded-control border border-border bg-surface px-3 py-1.5 text-caption font-medium text-fg-1 transition-colors hover:border-stack-border hover:bg-stack-soft/50"
                        >
                          <ProviderIcon
                            icon={preset.icon}
                            name={preset.name}
                            color={preset.iconColor}
                            size={16}
                          />
                          <span>{preset.name}</span>
                        </button>
                      ))}
                    </div>
                  )}
                </div>
              ) : (
                <div className="overflow-hidden rounded-panel border border-border bg-surface divide-y divide-border">
                  {filteredUniversal.map((u) => {
                    const key = `universal:${u.id}`;
                    const assignedApps = universalAssignedMap.get(u.id) ?? [];
                    const modelList = [
                      u.models?.claude?.model,
                      u.models?.codex?.model,
                      u.models?.gemini?.model,
                      ...(u.extraModels ?? []),
                    ].filter(
                      (m, i, arr): m is string =>
                        Boolean(m) && arr.indexOf(m) === i,
                    );
                    const host = extractHost(u.baseUrl) || u.baseUrl;
                    const masked = maskSecretKey(u.apiKey);
                    const isEditingAssign = assigningKey === key;

                    return (
                      <div key={key}>
                        <div className="flex flex-wrap items-center justify-between gap-3 px-4 py-3 transition-colors hover:bg-subtle/40">
                          <div className="flex min-w-0 flex-1 items-center gap-3">
                            <ProviderIcon
                              icon={u.icon}
                              name={u.name}
                              color={u.iconColor}
                              size={28}
                            />
                            <div className="min-w-0 flex-1">
                              <div className="flex flex-wrap items-center gap-1.5">
                                <span className="truncate text-body font-semibold text-fg-1">
                                  {u.name}
                                </span>
                                <Badge
                                  variant="outline"
                                  className="h-5 px-1.5 text-[11px] text-fg-2"
                                >
                                  {FORMAT_LABELS[u.apiFormat || "openai_chat"] ??
                                    u.apiFormat ??
                                    "OpenAI"}
                                </Badge>
                                <span className="rounded bg-subtle px-1.5 py-0.5 font-mono text-[11px] text-fg-3">
                                  {isLocalUrl(u.baseUrl) && !masked
                                    ? t("aggregation.localNoKey", {
                                        defaultValue: "本地免 Key",
                                      })
                                    : masked || "Key"}
                                </span>
                              </div>
                              <div className="mt-0.5 flex flex-wrap items-center gap-1.5 text-caption text-fg-3">
                                <span title={u.baseUrl} className="truncate">
                                  {host}
                                </span>
                                {modelList.length > 0 && (
                                  <>
                                    <span>·</span>
                                    <span
                                      title={modelList.join(", ")}
                                      className="truncate font-mono text-[11px]"
                                    >
                                      {modelList[0]}
                                      {modelList.length > 1
                                        ? ` +${modelList.length - 1}`
                                        : ""}
                                    </span>
                                  </>
                                )}
                              </div>
                              {/* 已指定的适用应用标签 */}
                              <div className="mt-1.5 flex flex-wrap items-center gap-1">
                                {assignedApps.length > 0 ? (
                                  assignedApps.map((appId) => (
                                    <button
                                      key={appId}
                                      type="button"
                                      onClick={() => onOpenApp(appId)}
                                      className="inline-flex items-center gap-1 rounded-control border border-stack-border/60 bg-stack-soft/70 px-1.5 py-0.5 text-[11px] font-medium text-stack-text hover:bg-stack-soft"
                                    >
                                      <AppGlyph
                                        app={appId}
                                        size={11}
                                        badgeClassName="bg-transparent"
                                      />
                                      {APP_DISPLAY_NAME[appId]}
                                    </button>
                                  ))
                                ) : (
                                  <span className="rounded bg-subtle px-1.5 py-0.5 text-[11px] text-fg-3">
                                    {t("aggregation.noAppsAssigned", {
                                      defaultValue: "未分配应用",
                                    })}
                                  </span>
                                )}
                              </div>
                            </div>
                          </div>

                          <div className="flex shrink-0 flex-wrap items-center gap-1.5">
                            <Button
                              type="button"
                              variant={isEditingAssign ? "solid" : "neutral"}
                              size="compact"
                              onClick={() => openAssignPanel(key, assignedApps)}
                            >
                              <SlidersHorizontal className="h-3.5 w-3.5" />
                              {t("aggregation.assignApps", {
                                defaultValue: "分配应用",
                              })}
                            </Button>

                            <div className="flex items-center gap-0.5">
                              <HoverTip content={t("common.edit")}>
                                <Button
                                  variant="quiet"
                                  size="icon-compact"
                                  className="h-7 w-7"
                                  aria-label={t("common.edit")}
                                  onClick={() => {
                                    setEditingAppProvider(null);
                                    setEditingUniversal(u);
                                    setInitialPreset(null);
                                    setModalOpen(true);
                                  }}
                                >
                                  <Edit2 className="h-3.5 w-3.5" />
                                </Button>
                              </HoverTip>
                              <HoverTip
                                content={t("universalProvider.duplicate", {
                                  defaultValue: "复制",
                                })}
                              >
                                <Button
                                  variant="quiet"
                                  size="icon-compact"
                                  className="h-7 w-7"
                                  aria-label={t("universalProvider.duplicate", {
                                    defaultValue: "复制",
                                  })}
                                  onClick={() =>
                                    void handleDuplicateUniversal(u)
                                  }
                                >
                                  <Copy className="h-3.5 w-3.5" />
                                </Button>
                              </HoverTip>
                              <HoverTip content={t("common.delete")}>
                                <Button
                                  variant="quiet"
                                  size="icon-compact"
                                  className="h-7 w-7 text-fg-2 hover:text-danger-text"
                                  aria-label={t("common.delete")}
                                  onClick={() =>
                                    setDeleteTarget({
                                      sourceApp: null,
                                      providerId: u.id,
                                      name: u.name,
                                    })
                                  }
                                >
                                  <Trash2 className="h-3.5 w-3.5" />
                                </Button>
                              </HoverTip>
                            </div>
                          </div>
                        </div>

                        {isEditingAssign &&
                          renderInlineAssignEditor(key, null, u.id)}
                      </div>
                    );
                  })}

                  {filteredReadyAppProviders.map(
                    (item: AggregationAppProviderItem) => {
                      const key = `${item.appType}:${item.provider.id}`;
                      const host =
                        extractHost(item.baseUrl) || item.baseUrl || "";
                      const assignedApps = item.syncedApps.filter((a) =>
                        visibleSet.has(a),
                      ) as AppId[];
                      const isEditingAssign = assigningKey === key;

                      return (
                        <div key={key}>
                          <div className="flex flex-wrap items-center justify-between gap-3 px-4 py-3 transition-colors hover:bg-subtle/40">
                            <div className="flex min-w-0 flex-1 items-center gap-3">
                              <ProviderIcon
                                icon={item.provider.icon}
                                name={item.provider.name}
                                color={item.provider.iconColor}
                                size={28}
                              />
                              <div className="min-w-0 flex-1">
                                <div className="flex flex-wrap items-center gap-1.5">
                                  <span className="truncate text-body font-semibold text-fg-1">
                                    {item.provider.name}
                                  </span>
                                  <Badge
                                    variant="outline"
                                    className="h-5 px-1.5 text-[11px] text-fg-2"
                                  >
                                    {FORMAT_LABELS[item.apiFormat] ??
                                      item.apiFormat}
                                  </Badge>
                                  <span className="rounded bg-subtle px-1.5 py-0.5 text-[11px] text-fg-3">
                                    {isLocalUrl(item.baseUrl)
                                      ? t("aggregation.localNoKey", {
                                          defaultValue: "本地免 Key",
                                        })
                                      : t("aggregation.readyBadge", {
                                          defaultValue: "已就绪",
                                        })}
                                  </span>
                                </div>
                                <div className="mt-0.5 flex items-center gap-1.5 truncate text-caption text-fg-3">
                                  {host && (
                                    <span
                                      title={item.baseUrl}
                                      className="truncate"
                                    >
                                      {host}
                                    </span>
                                  )}
                                  {item.models.length > 0 && (
                                    <>
                                      {host && <span>·</span>}
                                      <span
                                        title={item.models.join(", ")}
                                        className="truncate font-mono text-[11px]"
                                      >
                                        {item.models[0]}
                                        {item.models.length > 1
                                          ? ` +${item.models.length - 1}`
                                          : ""}
                                      </span>
                                    </>
                                  )}
                                </div>
                                <div className="mt-1.5 flex flex-wrap items-center gap-1">
                                  {assignedApps.length > 0 ? (
                                    assignedApps.map((appId) => (
                                      <button
                                        key={appId}
                                        type="button"
                                        onClick={() => onOpenApp(appId)}
                                        className="inline-flex items-center gap-1 rounded-control border border-stack-border/60 bg-stack-soft/70 px-1.5 py-0.5 text-[11px] font-medium text-stack-text hover:bg-stack-soft"
                                      >
                                        <AppGlyph
                                          app={appId}
                                          size={11}
                                          badgeClassName="bg-transparent"
                                        />
                                        {APP_DISPLAY_NAME[appId]}
                                      </button>
                                    ))
                                  ) : (
                                    <span className="rounded bg-subtle px-1.5 py-0.5 text-[11px] text-fg-3">
                                      {t("aggregation.noAppsAssigned", {
                                        defaultValue: "未分配应用",
                                      })}
                                    </span>
                                  )}
                                </div>
                              </div>
                            </div>

                            <div className="flex shrink-0 flex-wrap items-center gap-1.5">
                              <Button
                                type="button"
                                variant={isEditingAssign ? "solid" : "neutral"}
                                size="compact"
                                onClick={() =>
                                  openAssignPanel(key, assignedApps)
                                }
                              >
                                <SlidersHorizontal className="h-3.5 w-3.5" />
                                {t("aggregation.assignApps", {
                                  defaultValue: "分配应用",
                                })}
                              </Button>

                              <div className="flex items-center gap-0.5">
                                <HoverTip content={t("common.edit")}>
                                  <Button
                                    variant="quiet"
                                    size="icon-compact"
                                    className="h-7 w-7"
                                    aria-label={t("common.edit")}
                                    onClick={() => {
                                      setEditingUniversal(null);
                                      setEditingAppProvider(item);
                                      setInitialPreset(null);
                                      setModalOpen(true);
                                    }}
                                  >
                                    <Edit2 className="h-3.5 w-3.5" />
                                  </Button>
                                </HoverTip>
                                <HoverTip content={t("common.delete")}>
                                  <Button
                                    variant="quiet"
                                    size="icon-compact"
                                    className="h-7 w-7 text-fg-2 hover:text-danger-text"
                                    aria-label={t("common.delete")}
                                    onClick={() =>
                                      setDeleteTarget({
                                        sourceApp: item.appType,
                                        providerId: item.provider.id,
                                        name: item.provider.name,
                                      })
                                    }
                                  >
                                    <Trash2 className="h-3.5 w-3.5" />
                                  </Button>
                                </HoverTip>
                              </div>
                            </div>
                          </div>

                          {isEditingAssign &&
                            renderInlineAssignEditor(
                              key,
                              item.appType,
                              item.provider.id,
                            )}
                        </div>
                      );
                    },
                  )}
                </div>
              )}

              {/* 4. 遗留的待补全凭据外部供应商（官方账号不再出现在此处） */}
              {filteredIncompleteAppProviders.length > 0 && (
                <div className="rounded-panel border border-border bg-surface">
                  <button
                    type="button"
                    onClick={() => setSecondaryFolded((v) => !v)}
                    className="flex w-full items-center justify-between px-4 py-2.5 text-caption font-medium text-fg-2 hover:text-fg-1"
                  >
                    <span>
                      {t("aggregation.incompleteSectionTitle", {
                        count: filteredIncompleteAppProviders.length,
                        defaultValue: `待补全凭据的外部供应商 (${filteredIncompleteAppProviders.length})`,
                      })}
                    </span>
                    {secondaryFolded ? (
                      <ChevronRight className="h-4 w-4" />
                    ) : (
                      <ChevronDown className="h-4 w-4" />
                    )}
                  </button>

                  {!secondaryFolded && (
                    <div className="divide-y divide-border border-t border-border">
                      {filteredIncompleteAppProviders.map((item) => {
                        const key = `${item.appType}:${item.provider.id}`;
                        const host =
                          extractHost(item.baseUrl) || item.baseUrl || "";
                        return (
                          <div
                            key={key}
                            className="flex flex-wrap items-center justify-between gap-3 px-4 py-2.5 opacity-90"
                          >
                            <div className="flex min-w-0 flex-1 items-center gap-3">
                              <ProviderIcon
                                icon={item.provider.icon}
                                name={item.provider.name}
                                color={item.provider.iconColor}
                                size={24}
                              />
                              <div className="min-w-0 flex-1">
                                <div className="flex items-center gap-1.5">
                                  <span className="truncate text-body font-medium text-fg-1">
                                    {item.provider.name}
                                  </span>
                                  <Badge
                                    variant="outline"
                                    className="inline-flex h-5 items-center gap-1 px-1.5 text-[11px]"
                                  >
                                    <AppGlyph
                                      app={item.appType}
                                      size={11}
                                      badgeClassName="bg-transparent"
                                    />
                                    {APP_DISPLAY_NAME[item.appType] ??
                                      item.appType}
                                  </Badge>
                                </div>
                                {host && (
                                  <div className="truncate text-[11px] text-fg-3">
                                    {host}
                                  </div>
                                )}
                              </div>
                            </div>

                            <div className="flex shrink-0 items-center gap-2">
                              <span className="rounded bg-subtle px-2 py-0.5 text-[11px] text-fg-3">
                                {t("aggregation.missingCredentials", {
                                  defaultValue: "缺少端点或 Key",
                                })}
                              </span>
                              <Button
                                variant="neutral"
                                size="compact"
                                onClick={() => {
                                  setEditingUniversal(null);
                                  setEditingAppProvider(item);
                                  setInitialPreset(null);
                                  setModalOpen(true);
                                }}
                              >
                                <Edit2 className="h-3.5 w-3.5" />
                                {t("aggregation.completeCredentials", {
                                  defaultValue: "补全并配置",
                                })}
                              </Button>
                              <HoverTip content={t("common.delete")}>
                                <Button
                                  variant="quiet"
                                  size="icon-compact"
                                  className="h-7 w-7 text-fg-2 hover:text-danger-text"
                                  aria-label={t("common.delete")}
                                  onClick={() =>
                                    setDeleteTarget({
                                      sourceApp: item.appType,
                                      providerId: item.provider.id,
                                      name: item.provider.name,
                                    })
                                  }
                                >
                                  <Trash2 className="h-3.5 w-3.5" />
                                </Button>
                              </HoverTip>
                            </div>
                          </div>
                        );
                      })}
                    </div>
                  )}
                </div>
              )}
            </div>
          )}
        </div>
      </div>

      <AggregationProviderModal
        isOpen={modalOpen}
        onClose={() => {
          setModalOpen(false);
          setEditingUniversal(null);
          setEditingAppProvider(null);
          setInitialPreset(null);
        }}
        onSaveAndSync={handleSaveAndSyncUniversal}
        onPromoteAppProvider={handlePromoteAppProvider}
        editingProvider={editingUniversal}
        editingAppProvider={editingAppProvider}
        initialPreset={initialPreset}
      />

      {visibleSet.has("codex") && (
        <StackSettingsSheet
          open={codexStackSettingsOpen}
          onOpenChange={setCodexStackSettingsOpen}
          models={codexStack?.subagentModels ?? []}
        />
      )}

      <ConfirmDialog
        isOpen={Boolean(deleteTarget)}
        title={t("aggregation.deleteConfirmTitle", {
          defaultValue: "删除外部供应商",
        })}
        message={t("aggregation.deleteConfirmDescription", {
          name: deleteTarget?.name ?? "",
          defaultValue: `确定删除「${deleteTarget?.name ?? ""}」并移除其分配到各应用的配置吗？`,
        })}
        onConfirm={() => void handleConfirmDelete()}
        onCancel={() => setDeleteTarget(null)}
      />
    </>
  );
}

function FilterPill({
  active,
  label,
  onClick,
}: {
  active: boolean;
  label: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "inline-flex h-7 items-center rounded-full border px-3 text-caption transition-colors",
        active
          ? "border-action/40 bg-action-soft font-medium text-action-text"
          : "border-border bg-surface text-fg-2 hover:text-fg-1",
      )}
    >
      {label}
    </button>
  );
}
