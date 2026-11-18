import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  Copy,
  Edit2,
  ExternalLink,
  Layers,
  Loader2,
  Plus,
  RefreshCw,
  Search,
  Settings2,
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
import { universalProvidersApi, type AppId } from "@/lib/api";
import { useProxyStack } from "@/lib/query/proxy";
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
import { cn } from "@/lib/utils";
import { AggregationProviderModal } from "./AggregationProviderModal";
import {
  COMPATIBLE_APP_IDS,
  countVisibleEnabledUniversalApps,
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
  const [sourceFilter, setSourceFilter] = useState<string>("all");
  const [togglingMode, setTogglingMode] = useState(false);
  const [syncingKey, setSyncingKey] = useState<string | null>(null);
  const [appModeBusy, setAppModeBusy] = useState<string | null>(null);
  const [modalOpen, setModalOpen] = useState(false);
  const [editingUniversal, setEditingUniversal] =
    useState<UniversalProvider | null>(null);
  const [initialPreset, setInitialPreset] =
    useState<AggregationPresetEntry | null>(null);
  const [secondaryFolded, setSecondaryFolded] = useState(true);
  const [codexStackSettingsOpen, setCodexStackSettingsOpen] = useState(false);
  const [deleteUniversalTarget, setDeleteUniversalTarget] =
    useState<UniversalProvider | null>(null);

  const invalidateAll = async () => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: ["aggregation"] }),
      queryClient.invalidateQueries({ queryKey: ["settings"] }),
      queryClient.invalidateQueries({ queryKey: ["app-mode"] }),
      queryClient.invalidateQueries({ queryKey: ["proxy-stack"] }),
      queryClient.invalidateQueries({ queryKey: ["providers"] }),
      queryClient.invalidateQueries({ queryKey: ["universal-providers"] }),
      queryClient.invalidateQueries({ queryKey: ["pi", "currentState"] }),
      queryClient.invalidateQueries({ queryKey: ["omp", "currentState"] }),
      queryClient.invalidateQueries({ queryKey: ["opencodeLiveProviderIds"] }),
      queryClient.invalidateQueries({ queryKey: ["openclaw", "liveProviderIds"] }),
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
          ? t("aggregation.toast.enabled", { defaultValue: "已开启全局聚合模式" })
          : t("aggregation.toast.disabled", { defaultValue: "已关闭全局聚合模式" }),
      );
    } catch (error) {
      toast.error(
        t("aggregation.toast.toggleFailed", { defaultValue: "切换聚合模式失败" }),
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
        t("aggregation.toast.syncedApps", {
          count: result.syncedApps.length,
          defaultValue: `已保存并同步兼容到 ${result.syncedApps.length} 个应用`,
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

  const handleSyncToAllApps = async (
    sourceApp: string | null,
    providerId: string,
    key: string,
  ) => {
    setSyncingKey(key);
    try {
      const result = await aggregationApi.syncProviderToAllApps(
        sourceApp,
        providerId,
      );
      await invalidateAll();
      toast.success(
        t("aggregation.toast.syncedApps", {
          count: result.syncedApps.length,
          defaultValue: `已兼容同步到 ${result.syncedApps.length} 个应用`,
        }),
      );
      if (result.warnings.length > 0) {
        toast.warning(result.warnings.slice(0, 3).join("；"));
      }
    } catch (error) {
      toast.error(
        t("aggregation.toast.syncFailed", { defaultValue: "同步兼容失败" }),
        { description: extractErrorMessage(error) || undefined },
      );
    } finally {
      setSyncingKey(null);
    }
  };

  const handleUnsyncProvider = async (
    sourceApp: string | null,
    providerId: string,
    key: string,
  ) => {
    setSyncingKey(key);
    try {
      const result = await aggregationApi.unsyncProvider(
        sourceApp,
        providerId,
      );
      await invalidateAll();
      toast.success(
        result.syncedApps.length > 0
          ? t("aggregation.toast.unsyncedApps", {
              count: result.syncedApps.length,
              defaultValue: `已取消兼容（已移除 ${result.syncedApps.length} 个应用配置）`,
            })
          : t("aggregation.toast.unsynced", {
              defaultValue: "已取消跨应用兼容",
            }),
      );
      if (result.warnings.length > 0) {
        toast.warning(result.warnings.slice(0, 3).join("；"));
      }
    } catch (error) {
      toast.error(
        t("aggregation.toast.unsyncFailed", {
          defaultValue: "取消兼容失败",
        }),
        { description: extractErrorMessage(error) || undefined },
      );
    } finally {
      setSyncingKey(null);
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

  const handleDeleteUniversal = async () => {
    if (!deleteUniversalTarget) return;
    try {
      await universalProvidersApi.delete(deleteUniversalTarget.id);
      await invalidateAll();
      toast.success(
        t("universalProvider.deleted", { defaultValue: "通用供应商已删除" }),
      );
    } catch (error) {
      toast.error(
        t("universalProvider.deleteError", {
          defaultValue: "删除通用供应商失败",
        }),
        { description: extractErrorMessage(error) || undefined },
      );
    } finally {
      setDeleteUniversalTarget(null);
    }
  };

  const handleToggleAppMode = async (appState: AggregationAppState) => {
    const entering = appState.supportsStackCatalog
      ? appState.mode !== "stack"
      : appState.mode === "direct";
    setAppModeBusy(appState.appType);
    try {
      await aggregationApi.setAppMode(
        appState.appType,
        entering,
        appState.routeProviderId ?? null,
      );
      await invalidateAll();
      toast.success(
        entering
          ? t("aggregation.toast.appAttached", {
              app: APP_DISPLAY_NAME[appState.appType],
              defaultValue: `${APP_DISPLAY_NAME[appState.appType]} 已接入聚合`,
            })
          : t("aggregation.toast.appDetached", {
              app: APP_DISPLAY_NAME[appState.appType],
              defaultValue: `${APP_DISPLAY_NAME[appState.appType]} 已回到直连`,
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
  const appProviders = useMemo(
    () =>
      (overview?.appProviders ?? [])
        .filter((item) => visibleSet.has(item.appType))
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
    item.canSync ?? (!item.isOfficial && Boolean(item.baseUrl));

  const readyAppProviders = useMemo(
    () => appProviders.filter(isItemReady),
    [appProviders],
  );
  const secondaryAppProviders = useMemo(
    () => appProviders.filter((item) => !isItemReady(item)),
    [appProviders],
  );

  const activeFilter =
    sourceFilter !== "all" &&
    sourceFilter !== "universal" &&
    !visibleSet.has(sourceFilter)
      ? "all"
      : sourceFilter;

  const appCounts = useMemo(() => {
    const map = new Map<string, number>();
    for (const item of readyAppProviders) {
      map.set(item.appType, (map.get(item.appType) ?? 0) + 1);
    }
    return map;
  }, [readyAppProviders]);

  const appsWithProviders = useMemo(
    () => Array.from(appCounts.keys()),
    [appCounts],
  );

  const needle = searchTerm.trim().toLowerCase();

  const filteredUniversal = useMemo(() => {
    if (activeFilter !== "all" && activeFilter !== "universal") return [];
    return universalProviders.filter((u) => {
      if (!needle) return true;
      return (
        u.name.toLowerCase().includes(needle) ||
        u.baseUrl.toLowerCase().includes(needle) ||
        (u.notes ?? "").toLowerCase().includes(needle)
      );
    });
  }, [universalProviders, activeFilter, needle]);

  const matchesAppItem = (item: AggregationAppProviderItem) => {
    if (activeFilter === "universal") return false;
    if (activeFilter !== "all" && item.appType !== activeFilter) return false;
    if (!needle) return true;
    return (
      item.provider.name.toLowerCase().includes(needle) ||
      (item.baseUrl ?? "").toLowerCase().includes(needle) ||
      APP_DISPLAY_NAME[item.appType].toLowerCase().includes(needle) ||
      item.models.some((m) => m.toLowerCase().includes(needle))
    );
  };

  const filteredReadyAppProviders = useMemo(
    () => readyAppProviders.filter(matchesAppItem),
    [readyAppProviders, activeFilter, needle],
  );

  const filteredSecondaryAppProviders = useMemo(
    () => secondaryAppProviders.filter(matchesAppItem),
    [secondaryAppProviders, activeFilter, needle],
  );

  // 计算通用供应商正在被哪些可见应用路由使用（参考 Magpie row uses）
  const activeAppsByUniversalId = useMemo(() => {
    const map = new Map<string, AppId[]>();
    for (const appState of appStates) {
      const routeId = appState.routeProviderId;
      if (!routeId) continue;
      for (const u of universalProviders) {
        if (
          routeId === `universal-${u.id}` ||
          routeId === `universal-${appState.appType}-${u.id}`
        ) {
          const list = map.get(u.id) ?? [];
          list.push(appState.appType);
          map.set(u.id, list);
        }
      }
    }
    return map;
  }, [appStates, universalProviders]);

  const readyTotalCount = universalProviders.length + readyAppProviders.length;

  const openModalWithPreset = (preset: AggregationPresetEntry | null) => {
    setEditingUniversal(null);
    setInitialPreset(preset);
    setModalOpen(true);
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
                  "开启聚合模式后，所有添加的供应商将通过协议转换与配置分发兼容全部应用，并集中在此页面展示和管理。",
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
                defaultValue: "按名称、地址或模型筛选",
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
                defaultValue: "添加供应商",
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
          {/* 1. 顶部控制卡：全局开关 + 代理状态 + 已显示应用状态 */}
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
                      "开启后，所有添加的供应商将自动适配并兼容全部已显示的应用，并集中在本页面管理。",
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
                    defaultValue: "各应用兼容与接入状态",
                  })}
                </div>
                <div className="flex flex-wrap gap-1.5">
                  {appStates.map((appState) => {
                    const isProxyAttached = appState.supportsStackCatalog
                      ? appState.mode === "stack"
                      : appState.supportsLocalProxy
                        ? appState.mode !== "direct"
                        : Boolean(
                            overview?.enabled &&
                              (appState.attached ||
                                appState.syncedProviderCount > 0),
                          );
                    const busy = appModeBusy === appState.appType;
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
                        {appState.supportsLocalProxy ? (
                          <button
                            type="button"
                            disabled={busy}
                            onClick={() => void handleToggleAppMode(appState)}
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
                        ) : isProxyAttached ? (
                          <span className="rounded bg-stack px-1.5 py-0.5 text-[11px] font-medium text-stack-on">
                            {t("aggregation.appAttached", {
                              defaultValue: "已接入",
                            })}
                          </span>
                        ) : (
                          <span className="rounded bg-subtle px-1.5 py-0.5 text-[11px] text-fg-3">
                            {t("aggregation.syncedCount", {
                              count: appState.syncedProviderCount,
                              defaultValue: `已同步 ${appState.syncedProviderCount}`,
                            })}
                          </span>
                        )}
                      </div>
                    );
                  })}
                </div>
              </div>
            )}
          </div>

          {/* 2. 筛选栏与搜索框 */}
          <div className="flex flex-wrap items-center justify-between gap-2">
            <div className="flex flex-wrap items-center gap-1.5">
              <FilterPill
                active={activeFilter === "all"}
                label={t("aggregation.filterAll", {
                  count: readyTotalCount,
                  defaultValue: `可用供应商 (${readyTotalCount})`,
                })}
                onClick={() => setSourceFilter("all")}
              />
              <FilterPill
                active={activeFilter === "universal"}
                label={t("aggregation.filterUniversal", {
                  count: universalProviders.length,
                  defaultValue: `跨应用添加 (${universalProviders.length})`,
                })}
                onClick={() => setSourceFilter("universal")}
              />
              {appsWithProviders.map((appKey) => (
                <FilterPill
                  key={appKey}
                  active={activeFilter === appKey}
                  label={`${APP_DISPLAY_NAME[appKey as AppId] ?? appKey} (${appCounts.get(appKey) ?? 0})`}
                  onClick={() => setSourceFilter(appKey)}
                />
              ))}
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

          {/* 3. Magpie 风格紧凑行式供应商列表 */}
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
                      defaultValue: "暂无可跨应用聚合的供应商",
                    })}
                  </p>
                  <p className="mt-1 text-caption text-fg-3">
                    {t("aggregation.emptyHint", {
                      defaultValue:
                        "选择一个常用预设并填入 API Key，即可让全部已显示的应用共用。",
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
                    const enabledCount = countVisibleEnabledUniversalApps(
                      u.apps,
                      visibleSet,
                    );
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
                    const usingApps = activeAppsByUniversalId.get(u.id) ?? [];

                    return (
                      <div
                        key={key}
                        className="flex flex-wrap items-center justify-between gap-3 px-4 py-3 transition-colors hover:bg-subtle/40"
                      >
                        {/* 左侧：图标 + 名称 + 简明副标题 */}
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
                                className="h-5 border-stack-border bg-stack-soft px-1.5 text-[11px] text-stack-text"
                              >
                                {t("aggregation.universalBadge", {
                                  defaultValue: "通用",
                                })}
                              </Badge>
                              <Badge
                                variant="outline"
                                className="h-5 px-1.5 text-[11px] text-fg-2"
                              >
                                {FORMAT_LABELS[u.apiFormat || "openai_chat"] ??
                                  u.apiFormat ??
                                  "OpenAI"}
                              </Badge>
                            </div>
                            <div className="mt-0.5 flex items-center gap-1.5 truncate text-caption text-fg-3">
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
                          </div>
                        </div>

                        {/* 右侧：正在使用的应用图标 + Key 胶囊 + 兼容状态 + 操作 */}
                        <div className="flex shrink-0 flex-wrap items-center gap-2">
                          {usingApps.length > 0 && (
                            <div className="flex items-center gap-1">
                              {usingApps.map((appId) => (
                                <HoverTip
                                  key={appId}
                                  content={`${APP_DISPLAY_NAME[appId]} 正在使用`}
                                >
                                  <button
                                    type="button"
                                    onClick={() => onOpenApp(appId)}
                                    className="inline-flex h-6 w-6 items-center justify-center rounded border border-border bg-app hover:border-stack-border"
                                  >
                                    <AppGlyph
                                      app={appId}
                                      size={13}
                                      badgeClassName="bg-transparent"
                                    />
                                  </button>
                                </HoverTip>
                              ))}
                            </div>
                          )}

                          <span className="rounded bg-subtle px-2 py-0.5 font-mono text-[11px] text-fg-2">
                            {isLocalUrl(u.baseUrl) && !masked
                              ? t("aggregation.localNoKey", {
                                  defaultValue: "本地免 Key",
                                })
                              : masked || "Key"}
                          </span>

                          {enabledCount > 0 ? (
                            <>
                              <span className="inline-flex items-center gap-1 rounded-full border border-stack-border/60 bg-stack-soft px-2 py-0.5 text-[11px] font-medium text-stack-text">
                                <CheckCircle2 className="h-3 w-3" />
                                {t("aggregation.compatibleCount", {
                                  count: enabledCount,
                                  defaultValue: `已兼容 ${enabledCount} 个应用`,
                                })}
                              </span>
                              <Button
                                variant="quiet"
                                size="compact"
                                disabled={syncingKey === key}
                                onClick={() =>
                                  void handleUnsyncProvider(null, u.id, key)
                                }
                              >
                                {syncingKey === key ? (
                                  <Loader2 className="h-3.5 w-3.5 animate-spin" />
                                ) : (
                                  <X className="h-3.5 w-3.5" />
                                )}
                                {t("aggregation.unsyncApps", {
                                  defaultValue: "取消兼容",
                                })}
                              </Button>
                            </>
                          ) : (
                            <>
                              <span className="rounded bg-subtle px-2 py-0.5 text-[11px] text-fg-3">
                                {t("aggregation.notCompatible", {
                                  defaultValue: "未兼容",
                                })}
                              </span>
                              <Button
                                variant="neutral"
                                size="compact"
                                disabled={syncingKey === key}
                                onClick={() =>
                                  void handleSyncToAllApps(null, u.id, key)
                                }
                              >
                                {syncingKey === key ? (
                                  <Loader2 className="h-3.5 w-3.5 animate-spin" />
                                ) : (
                                  <RefreshCw className="h-3.5 w-3.5" />
                                )}
                                {t("aggregation.syncAllApps", {
                                  defaultValue: "兼容全部应用",
                                })}
                              </Button>
                            </>
                          )}

                          <div className="flex items-center gap-0.5">
                            <HoverTip
                              content={t("aggregation.syncAllApps", {
                                defaultValue: "兼容全部应用",
                              })}
                            >
                              <Button
                                variant="quiet"
                                size="icon-compact"
                                className="h-7 w-7"
                                disabled={syncingKey === key}
                                aria-label={t("aggregation.syncAllApps", {
                                  defaultValue: "兼容全部应用",
                                })}
                                onClick={() =>
                                  void handleSyncToAllApps(null, u.id, key)
                                }
                              >
                                {syncingKey === key ? (
                                  <Loader2 className="h-3.5 w-3.5 animate-spin" />
                                ) : (
                                  <RefreshCw className="h-3.5 w-3.5" />
                                )}
                              </Button>
                            </HoverTip>
                            <HoverTip content={t("common.edit")}>
                              <Button
                                variant="quiet"
                                size="icon-compact"
                                className="h-7 w-7"
                                onClick={() => {
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
                                onClick={() => void handleDuplicateUniversal(u)}
                              >
                                <Copy className="h-3.5 w-3.5" />
                              </Button>
                            </HoverTip>
                            <HoverTip content={t("common.delete")}>
                              <Button
                                variant="quiet"
                                size="icon-compact"
                                className="h-7 w-7 text-fg-2 hover:text-danger-text"
                                onClick={() => setDeleteUniversalTarget(u)}
                              >
                                <Trash2 className="h-3.5 w-3.5" />
                              </Button>
                            </HoverTip>
                          </div>
                        </div>
                      </div>
                    );
                  })}

                  {filteredReadyAppProviders.map(
                    (item: AggregationAppProviderItem) => {
                      const key = `${item.appType}:${item.provider.id}`;
                      const host =
                        extractHost(item.baseUrl) || item.baseUrl || "";
                      const hasOtherSynced = item.syncedApps.some(
                        (a) => a !== item.appType,
                      );
                      const allVisibleSynced =
                        visibleAppIds.length > 0 &&
                        item.syncedApps.length >= visibleAppIds.length;
                      return (
                        <div
                          key={key}
                          className="flex flex-wrap items-center justify-between gap-3 px-4 py-3 transition-colors hover:bg-subtle/40"
                        >
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
                                <Badge
                                  variant="outline"
                                  className="h-5 px-1.5 text-[11px] text-fg-2"
                                >
                                  {FORMAT_LABELS[item.apiFormat] ??
                                    item.apiFormat}
                                </Badge>
                              </div>
                              <div className="mt-0.5 flex items-center gap-1.5 truncate text-caption text-fg-3">
                                {host && (
                                  <span title={item.baseUrl} className="truncate">
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
                            </div>
                          </div>

                          <div className="flex shrink-0 flex-wrap items-center gap-2">
                            {item.isCurrentInApp && (
                              <HoverTip
                                content={`${APP_DISPLAY_NAME[item.appType]} 正在使用`}
                              >
                                <button
                                  type="button"
                                  onClick={() => onOpenApp(item.appType)}
                                  className="inline-flex h-6 w-6 items-center justify-center rounded border border-direct-border bg-direct-soft"
                                >
                                  <AppGlyph
                                    app={item.appType}
                                    size={13}
                                    badgeClassName="bg-transparent"
                                  />
                                </button>
                              </HoverTip>
                            )}

                            <span className="rounded bg-subtle px-2 py-0.5 text-[11px] text-fg-2">
                              {isLocalUrl(item.baseUrl)
                                ? t("aggregation.localNoKey", {
                                    defaultValue: "本地免 Key",
                                  })
                                : t("aggregation.readyBadge", {
                                    defaultValue: "已就绪",
                                  })}
                            </span>

                            {hasOtherSynced ? (
                              <>
                                <span className="inline-flex items-center gap-1 rounded-full border border-stack-border/60 bg-stack-soft px-2 py-0.5 text-[11px] font-medium text-stack-text">
                                  <CheckCircle2 className="h-3 w-3" />
                                  {t("aggregation.compatibleCount", {
                                    count: item.syncedApps.length,
                                    defaultValue: `已兼容 ${item.syncedApps.length} 个应用`,
                                  })}
                                </span>
                                {!allVisibleSynced && (
                                  <Button
                                    variant="neutral"
                                    size="compact"
                                    disabled={syncingKey === key}
                                    onClick={() =>
                                      void handleSyncToAllApps(
                                        item.appType,
                                        item.provider.id,
                                        key,
                                      )
                                    }
                                  >
                                    {syncingKey === key ? (
                                      <Loader2 className="h-3.5 w-3.5 animate-spin" />
                                    ) : (
                                      <RefreshCw className="h-3.5 w-3.5" />
                                    )}
                                    {t("aggregation.syncAllApps", {
                                      defaultValue: "兼容全部应用",
                                    })}
                                  </Button>
                                )}
                                <Button
                                  variant="quiet"
                                  size="compact"
                                  disabled={syncingKey === key}
                                  onClick={() =>
                                    void handleUnsyncProvider(
                                      item.appType,
                                      item.provider.id,
                                      key,
                                    )
                                  }
                                >
                                  {syncingKey === key ? (
                                    <Loader2 className="h-3.5 w-3.5 animate-spin" />
                                  ) : (
                                    <X className="h-3.5 w-3.5" />
                                  )}
                                  {t("aggregation.unsyncApps", {
                                    defaultValue: "取消兼容",
                                  })}
                                </Button>
                              </>
                            ) : (
                              <Button
                                variant="neutral"
                                size="compact"
                                disabled={syncingKey === key}
                                onClick={() =>
                                  void handleSyncToAllApps(
                                    item.appType,
                                    item.provider.id,
                                    key,
                                  )
                                }
                              >
                                {syncingKey === key ? (
                                  <Loader2 className="h-3.5 w-3.5 animate-spin" />
                                ) : (
                                  <RefreshCw className="h-3.5 w-3.5" />
                                )}
                                {t("aggregation.syncAllApps", {
                                  defaultValue: "兼容全部应用",
                                })}
                              </Button>
                            )}

                            <HoverTip
                              content={t("aggregation.openApp", {
                                app:
                                  APP_DISPLAY_NAME[item.appType] ??
                                  item.appType,
                                defaultValue: `打开 ${APP_DISPLAY_NAME[item.appType] ?? item.appType}`,
                              })}
                            >
                              <Button
                                variant="quiet"
                                size="icon-compact"
                                className="h-7 w-7"
                                onClick={() => onOpenApp(item.appType)}
                              >
                                <ExternalLink className="h-3.5 w-3.5" />
                              </Button>
                            </HoverTip>
                          </div>
                        </div>
                      );
                    },
                  )}
                </div>
              )}

              {/* 4. 默认折叠次要项（官方账号 / 缺少独立 Key 的条目，对照 Magpie foldOff） */}
              {filteredSecondaryAppProviders.length > 0 && (
                <div className="rounded-panel border border-border bg-surface">
                  <button
                    type="button"
                    onClick={() => setSecondaryFolded((v) => !v)}
                    className="flex w-full items-center justify-between px-4 py-2.5 text-caption font-medium text-fg-2 hover:text-fg-1"
                  >
                    <span>
                      {t("aggregation.secondarySectionTitle", {
                        count: filteredSecondaryAppProviders.length,
                        defaultValue: `仅原生应用可用 · 官方与待补全凭据 (${filteredSecondaryAppProviders.length})`,
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
                      {filteredSecondaryAppProviders.map((item) => {
                        const key = `${item.appType}:${item.provider.id}`;
                        const host =
                          extractHost(item.baseUrl) || item.baseUrl || "";
                        return (
                          <div
                            key={key}
                            className="flex flex-wrap items-center justify-between gap-3 px-4 py-2.5 opacity-85"
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
                                {item.isOfficial
                                  ? t("providerCard.chip.official")
                                  : t("aggregation.missingCredentials", {
                                      defaultValue: "缺少端点或 Key",
                                    })}
                              </span>
                              <Button
                                variant="quiet"
                                size="compact"
                                onClick={() => onOpenApp(item.appType)}
                              >
                                <ExternalLink className="h-3.5 w-3.5" />
                                {t("aggregation.openApp", {
                                  app:
                                    APP_DISPLAY_NAME[item.appType] ??
                                    item.appType,
                                  defaultValue: `打开 ${APP_DISPLAY_NAME[item.appType] ?? item.appType}`,
                                })}
                              </Button>
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
          setInitialPreset(null);
        }}
        onSaveAndSync={handleSaveAndSyncUniversal}
        editingProvider={editingUniversal}
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
        isOpen={Boolean(deleteUniversalTarget)}
        title={t("universalProvider.deleteConfirmTitle", {
          defaultValue: "删除通用供应商",
        })}
        message={t("universalProvider.deleteConfirmDescription", {
          name: deleteUniversalTarget?.name ?? "",
          defaultValue: `确定删除「${deleteUniversalTarget?.name ?? ""}」以及所有同步到各应用的兼容配置吗？`,
        })}
        onConfirm={() => void handleDeleteUniversal()}
        onCancel={() => setDeleteUniversalTarget(null)}
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
