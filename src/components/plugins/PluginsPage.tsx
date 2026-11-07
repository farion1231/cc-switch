import {
  useDeferredValue,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Check,
  ChevronDown,
  Download,
  Loader2,
  MoreHorizontal,
  Plus,
  Puzzle,
  RefreshCw,
} from "lucide-react";
import { toast } from "@/lib/toast";
import { cn } from "@/lib/utils";
import { extractErrorMessage } from "@/utils/errorUtils";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { HelpTip } from "@/components/ui/help-tip";
import { HoverTip } from "@/components/ui/hover-tip";
import { PageTabs } from "@/components/ui/page-tabs";
import { SearchField } from "@/components/ui/search-field";
import { SegmentedControl } from "@/components/ui/segmented-control";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { DialogTitle, DialogDescription } from "@/components/ui/dialog";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { AppPageHeader } from "@/components/shell/AppPageHeader";
import {
  TablePagination,
  useClientPagination,
} from "@/components/usage/TablePagination";
import { AppGlyph, APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";
import {
  FIELD_CLASS,
  LABEL_CLASS,
  V7ConfirmDialog,
  V7Dialog,
} from "@/components/mcp/formBits";
import {
  installedFromCatalog,
  installedIdOf,
  installedPluginsQuery,
  isRemovable,
  pluginCatalogQuery,
  pluginMarketplacesQuery,
  pluginPatches,
  prefetchInstalledOnce,
  usePluginWrites,
} from "@/hooks/usePlugins";
import {
  PLUGIN_APP_FEATURES,
  PLUGIN_APPS,
  PLUGIN_CLI_NOT_INSTALLED,
  PLUGIN_NEEDS_TERMINAL_CONFIRM,
  pluginsApi,
  type PluginActionOutcome,
  type PluginApp,
  type PluginEntry,
  type PluginMarketplace,
} from "@/lib/api/plugins";

type PluginsView = "installed" | "discover" | "marketplaces";

/**
 * 发现页、市场页每页条数的选项。目录整份读进来（Claude Code 的官方目录有几千个插件），
 * 但只挂当前这一页的行，翻页、搜索时才换。
 */
export const PLUGIN_PAGE_SIZES = [20, 50, 100] as const;
export const DEFAULT_PLUGIN_PAGE_SIZE = 50;

const NO_MARKETPLACES: PluginMarketplace[] = [];

/** 名字、显示名、市场、描述、分类里任一包含关键词（不分大小写）。 */
export function filterPlugins(
  entries: PluginEntry[],
  query: string,
): PluginEntry[] {
  const q = query.trim().toLowerCase();
  if (!q) return entries;
  return entries.filter((entry) =>
    [
      entry.id,
      entry.displayName,
      entry.description,
      entry.category,
      entry.marketplace,
    ].some((field) => field?.toLowerCase().includes(q)),
  );
}

/** 已安装的条目缺描述、显示名（插件清单里没写）时，用目录里同一 id 的补上。 */
export function withCatalogDetails(
  installed: PluginEntry[],
  catalog: PluginEntry[] | undefined,
): PluginEntry[] {
  if (!catalog?.length) return installed;
  const byId = new Map(catalog.map((entry) => [installedIdOf(entry), entry]));
  return installed.map((entry) => {
    const item = byId.get(entry.id);
    if (!item || (entry.description && entry.displayName)) return entry;
    return {
      ...entry,
      description: entry.description || item.description,
      displayName: entry.displayName || item.displayName,
      category: entry.category || item.category,
    };
  });
}

/** 发现 = 目录里还没装的。装、卸只改已安装列表，这里跟着变，不用重读目录。 */
export function availableFromCatalog(
  catalog: PluginEntry[],
  installed: PluginEntry[],
): PluginEntry[] {
  const ids = new Set(installed.map((entry) => entry.id));
  return catalog.filter((entry) => !ids.has(installedIdOf(entry)));
}

/** 应用多于这个数时，分段控件放不下，改用下拉选择。 */
const SEGMENTED_APP_LIMIT = 3;

function pluginLabel(entry: PluginEntry) {
  return entry.displayName || entry.name;
}

export interface PluginsPageProps {
  /** 可选的应用（只列「应用」页里设为显示的）；为空时全部列出 */
  apps?: PluginApp[];
  initialApp?: PluginApp;
}

type WriteMessages = {
  success: (outcome: PluginActionOutcome) => string;
  failure: string;
  /** 后端返回已知错误码时换成的说明；没给或不认识时显示原始报错。 */
  describeError?: (error: string) => string | undefined;
};

export function PluginsPage({ apps, initialApp }: PluginsPageProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const appChoices = apps && apps.length > 0 ? apps : PLUGIN_APPS;
  const appChoicesKey = appChoices.join(",");
  const [app, setApp] = useState<PluginApp>(
    initialApp && appChoices.includes(initialApp) ? initialApp : appChoices[0],
  );
  const [view, setView] = useState<PluginsView>("installed");
  const [search, setSearch] = useState("");
  // 目录有几千项：输入框先更新，过滤稍后跟上，打字不卡
  const deferredSearch = useDeferredValue(search);
  const [uninstallTarget, setUninstallTarget] = useState<PluginEntry | null>(
    null,
  );
  const [installTarget, setInstallTarget] = useState<PluginEntry | null>(null);
  const [removeMarketTarget, setRemoveMarketTarget] =
    useState<PluginMarketplace | null>(null);
  const [addOpen, setAddOpen] = useState(false);
  const [source, setSource] = useState("");
  const [pageSize, setPageSize] = useState<number>(DEFAULT_PLUGIN_PAGE_SIZE);
  const listScrollRef = useRef<HTMLDivElement>(null);

  const appName = APP_DISPLAY_NAME[app];
  const features = PLUGIN_APP_FEATURES[app];
  const restartNote = t(
    features.restartNote === "gateway"
      ? "pluginsPage.restartNoteGateway"
      : "pluginsPage.restartNote",
    { app: appName },
  );

  const switchApp = (next: PluginApp) => {
    setApp(next);
    setSearch("");
    // 新应用没有市场页时回到已安装
    if (!PLUGIN_APP_FEATURES[next].marketplaces && view === "marketplaces") {
      setView("installed");
    }
  };

  // 已安装和目录同时起：已安装那条便宜，先回来先显示
  const installedQuery = useQuery(installedPluginsQuery(app));
  const catalogQuery = useQuery(pluginCatalogQuery(app));
  const marketplaces = useQuery({
    ...pluginMarketplacesQuery(app),
    enabled: features.marketplaces && (view === "marketplaces" || addOpen),
  });

  // 其它应用的已安装列表先在后台读好，切过去不用等
  useEffect(() => {
    for (const other of appChoicesKey.split(",") as PluginApp[]) {
      if (other !== app) prefetchInstalledOnce(queryClient, other);
    }
  }, [app, appChoicesKey, queryClient]);

  const installedAll = useMemo(
    () => withCatalogDetails(installedQuery.data ?? [], catalogQuery.data),
    [installedQuery.data, catalogQuery.data],
  );
  const availableAll = useMemo(
    () =>
      catalogQuery.data
        ? availableFromCatalog(catalogQuery.data, installedQuery.data ?? [])
        : [],
    [catalogQuery.data, installedQuery.data],
  );
  const installed = useMemo(
    () => filterPlugins(installedAll, deferredSearch),
    [installedAll, deferredSearch],
  );
  // 几千项的目录只在发现页里搜；停在别的页签时不算
  const available = useMemo(
    () =>
      view === "discover" ? filterPlugins(availableAll, deferredSearch) : [],
    [view, availableAll, deferredSearch],
  );

  // 换应用、换关键词、改每页条数回到第一页；装、卸只改列表，停在当前页（变少时收回到最后一页）
  const discoverPager = useClientPagination(
    available,
    JSON.stringify([app, deferredSearch.trim(), pageSize]),
    pageSize,
  );
  const marketPager = useClientPagination(
    marketplaces.data ?? NO_MARKETPLACES,
    JSON.stringify([app, pageSize]),
    pageSize,
  );

  // 翻页后从这一页的第一行看起
  useEffect(() => {
    if (listScrollRef.current) listScrollRef.current.scrollTop = 0;
  }, [discoverPager.page, marketPager.page]);

  const { busy, run: runWrite } = usePluginWrites(app);

  /** 跑一次写操作，成功失败各弹一条。乐观更新和后台对账在 usePluginWrites 里。 */
  const run = async (
    write: Parameters<typeof runWrite>[0],
    messages: WriteMessages,
  ) => {
    if (busy[write.key]) return false;
    const result = await runWrite(write);
    if (result.ok && result.outcome) {
      toast.success(messages.success(result.outcome), {
        description: restartNote,
        closeButton: true,
      });
    } else {
      const error = extractErrorMessage(result.error) || String(result.error);
      toast.error(messages.failure, {
        description: messages.describeError?.(error) ?? error,
        closeButton: true,
      });
    }
    return result.ok;
  };

  /** CLI 要求确认信任的应用先弹确认框，其余直接装。 */
  const requestInstall = (entry: PluginEntry) => {
    if (features.confirmInstall) setInstallTarget(entry);
    else void install(entry);
  };

  const install = (entry: PluginEntry) =>
    run(
      {
        key: entry.id,
        action: "install",
        task: () => pluginsApi.install(app, entry.id),
        // 安装要联网、可能失败，不先挪；装好立即放进已安装，版本等对账时补齐
        onSuccess: pluginPatches.add(installedFromCatalog(app, entry)),
        reconcile: ["installed"],
      },
      {
        success: () =>
          t("pluginsPage.toast.installed", { name: pluginLabel(entry) }),
        failure: t("pluginsPage.toast.installFailed", {
          name: pluginLabel(entry),
        }),
        describeError: (error) =>
          error === PLUGIN_NEEDS_TERMINAL_CONFIRM
            ? t("pluginsPage.toast.installNeedsTerminal", {
                command: `claude plugin install ${entry.id}`,
              })
            : undefined,
      },
    );

  const toggle = (entry: PluginEntry, enabled: boolean) =>
    run(
      {
        key: entry.id,
        action: "toggle",
        task: () => pluginsApi.setEnabled(app, entry.id, enabled),
        optimistic: {
          apply: pluginPatches.setEnabled(entry, enabled),
          revert: pluginPatches.setEnabled(entry, entry.enabled),
        },
        reconcile: ["installed"],
      },
      {
        success: () =>
          t(
            enabled
              ? "pluginsPage.toast.enabled"
              : "pluginsPage.toast.disabled",
            { name: pluginLabel(entry) },
          ),
        failure: t("pluginsPage.toast.toggleFailed", {
          name: pluginLabel(entry),
        }),
      },
    );

  const update = (entry: PluginEntry) =>
    run(
      {
        key: entry.id,
        action: "update",
        task: () => pluginsApi.update(app, entry.id),
        reconcile: ["installed"],
      },
      {
        success: (outcome) =>
          t(
            outcome.changed
              ? "pluginsPage.toast.updated"
              : "pluginsPage.toast.upToDate",
            { name: pluginLabel(entry) },
          ),
        failure: t("pluginsPage.toast.updateFailed", {
          name: pluginLabel(entry),
        }),
      },
    );

  const uninstall = (entry: PluginEntry) => {
    const index = (installedQuery.data ?? []).findIndex(
      (e) => e.id === entry.id && (e.scope ?? null) === (entry.scope ?? null),
    );
    return run(
      {
        key: entry.id,
        action: "uninstall",
        task: () => pluginsApi.uninstall(app, entry.id),
        optimistic: {
          apply: pluginPatches.remove(entry),
          revert: pluginPatches.add(entry, index < 0 ? undefined : index),
        },
        reconcile: ["installed"],
      },
      {
        success: () =>
          t("pluginsPage.toast.uninstalled", { name: pluginLabel(entry) }),
        failure: t("pluginsPage.toast.uninstallFailed", {
          name: pluginLabel(entry),
        }),
      },
    );
  };

  // 市场变了目录才会变；移除市场时 Claude Code 可能连带卸掉它的插件
  const refreshMarketplaces = () =>
    run(
      {
        key: "market:*",
        action: "refresh",
        task: () => pluginsApi.refreshMarketplaces(app),
        reconcile: ["catalog", "marketplaces", "installed"],
      },
      {
        success: () => t("pluginsPage.toast.refreshed"),
        failure: t("pluginsPage.toast.refreshFailed"),
      },
    );

  const addMarketplace = async () => {
    const trimmed = source.trim();
    if (!trimmed) return;
    const ok = await run(
      {
        key: "market:+",
        action: "add",
        task: () => pluginsApi.addMarketplace(app, trimmed),
        reconcile: ["catalog", "marketplaces"],
      },
      {
        success: () => t("pluginsPage.toast.marketAdded", { source: trimmed }),
        failure: t("pluginsPage.toast.marketAddFailed"),
      },
    );
    // 失败时保留对话框和输入，改了再试
    if (ok) {
      setAddOpen(false);
      setSource("");
    }
  };

  const removeMarketplace = (market: PluginMarketplace) =>
    run(
      {
        key: `market:${market.name}`,
        action: "remove",
        task: () => pluginsApi.removeMarketplace(app, market.name),
        reconcile: ["catalog", "marketplaces", "installed"],
      },
      {
        success: () =>
          t("pluginsPage.toast.marketRemoved", { name: market.name }),
        failure: t("pluginsPage.toast.marketRemoveFailed", {
          name: market.name,
        }),
      },
    );

  const anyBusy = Object.keys(busy).length > 0;
  const loadError = installedQuery.isError
    ? extractErrorMessage(installedQuery.error) || String(installedQuery.error)
    : null;
  const cliMissing = loadError === PLUGIN_CLI_NOT_INSTALLED;
  const loaded = installedQuery.data !== undefined && loadError === null;
  const catalogReady = catalogQuery.data !== undefined;

  const header = (
    <AppPageHeader
      icon={<Puzzle className="h-5 w-5" strokeWidth={1.5} />}
      title={t("pluginsPage.title")}
      titleExtra={
        <HelpTip title={t("pluginsPage.helpTitle")}>
          {t("pluginsPage.help")}
        </HelpTip>
      }
      actions={
        features.refresh || features.marketplaces ? (
          <>
            {features.refresh && (
              <Button
                type="button"
                variant="quiet"
                size="regular"
                disabled={!loaded || anyBusy}
                onClick={() => void refreshMarketplaces()}
              >
                <RefreshCw
                  className={cn("h-4 w-4", busy["market:*"] && "animate-spin")}
                  strokeWidth={2}
                />
                {t(
                  features.marketplaces
                    ? "pluginsPage.refreshMarketplaces"
                    : "pluginsPage.refreshCatalog",
                )}
              </Button>
            )}
            {features.marketplaces && (
              <Button
                type="button"
                variant="solid"
                size="regular"
                disabled={!loaded}
                onClick={() => setAddOpen(true)}
              >
                <Plus className="h-4 w-4" strokeWidth={2} />
                {t("pluginsPage.addMarketplace")}
              </Button>
            )}
          </>
        ) : undefined
      }
    />
  );

  const tabs = (
    <PageTabs<PluginsView>
      aria-label={t("pluginsPage.viewAria")}
      idPrefix="plugins-view"
      className="h-11"
      value={view}
      onValueChange={setView}
      items={[
        {
          value: "installed",
          label: loaded
            ? t("pluginsPage.viewInstalledCount", {
                count: installedAll.length,
              })
            : t("pluginsPage.viewInstalled"),
        },
        {
          value: "discover",
          label:
            loaded && catalogReady
              ? t("pluginsPage.viewDiscoverCount", {
                  count: availableAll.length,
                })
              : t("pluginsPage.viewDiscover"),
        },
        ...(features.marketplaces
          ? [
              {
                value: "marketplaces" as const,
                label: t("pluginsPage.viewMarketplaces"),
              },
            ]
          : []),
      ]}
      trailing={
        appChoices.length > SEGMENTED_APP_LIMIT ? (
          <AppPicker apps={appChoices} value={app} onChange={switchApp} />
        ) : appChoices.length > 1 ? (
          <SegmentedControl<PluginApp>
            size="sm"
            aria-label={t("pluginsPage.appAria")}
            value={app}
            onValueChange={switchApp}
            items={appChoices.map((choice) => ({
              value: choice,
              label: (
                <span className="flex items-center gap-1.5">
                  <AppGlyph app={choice} size={14} />
                  {APP_DISPLAY_NAME[choice]}
                </span>
              ),
            }))}
          />
        ) : undefined
      }
    />
  );

  /** 翻页条放在列表面板底部；只有一页且条数不多时不画。 */
  const renderPager = (
    pager: typeof discoverPager | typeof marketPager,
  ): ReactNode => (
    <div className="shrink-0">
      <TablePagination
        page={pager.page}
        totalPages={pager.totalPages}
        total={pager.total}
        onPageChange={pager.setPage}
        pageSize={pageSize}
        pageSizeOptions={PLUGIN_PAGE_SIZES}
        onPageSizeChange={setPageSize}
      />
    </div>
  );

  const renderBody = () => {
    if (installedQuery.isPending) {
      return (
        <div className="flex items-center justify-center gap-2 py-12 text-body text-fg-2">
          <Loader2 className="h-4 w-4 animate-spin" />
          {t("pluginsPage.loading", { app: appName })}
        </div>
      );
    }
    if (cliMissing) {
      return (
        <CenteredState
          title={t("pluginsPage.cliMissingTitle", { app: appName })}
          body={t("pluginsPage.cliMissingBody", { app: appName })}
        />
      );
    }
    if (loadError !== null) {
      return (
        <CenteredState
          title={t("pluginsPage.loadFailed", { app: appName })}
          detail={loadError}
          action={
            <Button
              type="button"
              variant="neutral"
              size="regular"
              onClick={() => void installedQuery.refetch()}
            >
              {t("common.retry")}
            </Button>
          }
        />
      );
    }
    if (view === "marketplaces") return renderMarketplaces();
    if (view === "discover" && !catalogReady) {
      return catalogQuery.isError ? (
        <CenteredState
          title={t("pluginsPage.catalogLoadFailed")}
          detail={
            extractErrorMessage(catalogQuery.error) ||
            String(catalogQuery.error)
          }
          action={
            <Button
              type="button"
              variant="neutral"
              size="regular"
              onClick={() => void catalogQuery.refetch()}
            >
              {t("common.retry")}
            </Button>
          }
        />
      ) : (
        <div className="flex items-center justify-center gap-2 py-12 text-body text-fg-2">
          <Loader2 className="h-4 w-4 animate-spin" />
          {t("pluginsPage.loadingCatalog", { app: appName })}
        </div>
      );
    }

    const rows = view === "installed" ? installed : available;
    const total =
      view === "installed" ? installedAll.length : availableAll.length;
    // 显示的是本地缓存或上一次的结果，后台正在重读
    const refreshing =
      view === "installed"
        ? installedQuery.isFetching
        : catalogQuery.isFetching;

    if (total === 0) {
      return view === "installed" ? (
        <CenteredState
          title={t("pluginsPage.emptyInstalledTitle")}
          body={t("pluginsPage.emptyInstalledBody", { app: appName })}
          action={
            <Button
              type="button"
              variant="neutral"
              size="regular"
              onClick={() => setView("discover")}
            >
              {t("pluginsPage.browse")}
            </Button>
          }
        />
      ) : (
        <CenteredState
          title={t("pluginsPage.emptyDiscoverTitle")}
          body={t(
            features.marketplaces
              ? "pluginsPage.emptyDiscoverBody"
              : "pluginsPage.emptyCatalogBody",
          )}
        />
      );
    }

    const shown = view === "discover" ? discoverPager.pageRows : rows;
    return (
      <>
        <div className="flex h-14 shrink-0 items-center gap-3">
          <SearchField
            containerClassName="w-[320px] min-w-[180px] shrink"
            value={search}
            onValueChange={setSearch}
            placeholder={t("pluginsPage.searchPlaceholder")}
            aria-label={t("pluginsPage.searchAria")}
            clearLabel={t("common.clear")}
          />
          <span className="shrink-0 whitespace-nowrap text-caption tabular-nums text-fg-2">
            {deferredSearch.trim()
              ? t("pluginsPage.countFiltered", {
                  shown: rows.length,
                  count: total,
                })
              : t("pluginsPage.count", { count: total })}
          </span>
          {refreshing && (
            <HoverTip content={t("pluginsPage.refreshing")}>
              <span
                tabIndex={0}
                role="status"
                aria-label={t("pluginsPage.refreshing")}
                className="flex shrink-0 items-center text-fg-3"
              >
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
              </span>
            </HoverTip>
          )}
          <span className="ms-auto min-w-0 truncate text-caption text-fg-2">
            {restartNote}
          </span>
        </div>
        {rows.length === 0 ? (
          <CenteredState
            title={t("pluginsPage.noMatches", {
              query: deferredSearch.trim(),
            })}
          />
        ) : (
          <div className="flex min-h-0 flex-col overflow-hidden rounded-panel border border-border bg-surface">
            <div
              ref={listScrollRef}
              data-testid={`plugins-${view}`}
              className="min-h-0 overflow-auto scroll-stable"
            >
              {shown.map((entry, index) => (
                <PluginRow
                  key={`${entry.id}:${entry.scope ?? ""}`}
                  entry={entry}
                  isLast={index === shown.length - 1}
                  busyAction={busy[entry.id]}
                  canUpdate={features.update}
                  onInstall={() => requestInstall(entry)}
                  onToggle={(enabled) => void toggle(entry, enabled)}
                  onUpdate={() => void update(entry)}
                  onUninstall={() => setUninstallTarget(entry)}
                />
              ))}
            </div>
            {view === "discover" && renderPager(discoverPager)}
          </div>
        )}
      </>
    );
  };

  const renderMarketplaces = () => {
    if (marketplaces.isLoading) {
      return (
        <div className="flex items-center justify-center gap-2 py-12 text-body text-fg-2">
          <Loader2 className="h-4 w-4 animate-spin" />
          {t("pluginsPage.loading", { app: appName })}
        </div>
      );
    }
    if (marketplaces.isError) {
      return (
        <CenteredState
          title={t("pluginsPage.marketsLoadFailed")}
          detail={
            extractErrorMessage(marketplaces.error) ||
            String(marketplaces.error)
          }
          action={
            <Button
              type="button"
              variant="neutral"
              size="regular"
              onClick={() => void marketplaces.refetch()}
            >
              {t("common.retry")}
            </Button>
          }
        />
      );
    }
    if (marketPager.total === 0) {
      return (
        <CenteredState
          title={t("pluginsPage.emptyMarketsTitle")}
          body={t("pluginsPage.emptyMarketsBody", { app: appName })}
          action={
            <Button
              type="button"
              variant="neutral"
              size="regular"
              onClick={() => setAddOpen(true)}
            >
              <Plus className="h-3.5 w-3.5" strokeWidth={2} />
              {t("pluginsPage.addMarketplace")}
            </Button>
          }
        />
      );
    }
    return (
      <div className="mt-3 flex min-h-0 flex-col overflow-hidden rounded-panel border border-border bg-surface">
        <div
          ref={listScrollRef}
          data-testid="plugins-marketplaces"
          className="min-h-0 overflow-auto scroll-stable"
        >
          {marketPager.pageRows.map((market, index, markets) => {
            const removing = Boolean(busy[`market:${market.name}`]);
            return (
              <div
                key={market.name}
                className={cn(
                  "flex items-center gap-3 px-4 py-2.5",
                  index < markets.length - 1 && "border-b border-border",
                )}
              >
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-2">
                    <span className="truncate text-body font-medium text-fg-1">
                      {market.name}
                    </span>
                    {!market.removable && (
                      <span className="shrink-0 rounded-[4px] bg-subtle px-1.5 text-caption text-fg-2">
                        {t("pluginsPage.builtIn")}
                      </span>
                    )}
                  </div>
                  {(market.source || market.location) && (
                    <div className="truncate font-mono text-caption text-fg-3">
                      {market.source || market.location}
                    </div>
                  )}
                </div>
                {market.removable && (
                  <Button
                    type="button"
                    variant="neutral"
                    size="compact"
                    disabled={removing || anyBusy}
                    onClick={() => setRemoveMarketTarget(market)}
                  >
                    {removing && (
                      <Loader2 className="h-3.5 w-3.5 animate-spin" />
                    )}
                    {t("pluginsPage.removeMarketplace")}
                  </Button>
                )}
              </div>
            );
          })}
        </div>
        {renderPager(marketPager)}
      </div>
    );
  };

  return (
    <>
      {header}
      <div id="main-content" className="flex min-h-0 flex-1 flex-col">
        <div className="shrink-0 px-6 pt-1">{tabs}</div>
        <div className="flex min-h-0 flex-1 flex-col px-6 pb-5">
          {renderBody()}
        </div>
      </div>

      <V7ConfirmDialog
        open={uninstallTarget !== null}
        title={t("pluginsPage.confirmUninstallTitle", {
          name: uninstallTarget ? pluginLabel(uninstallTarget) : "",
        })}
        body={t(
          app === "claude"
            ? "pluginsPage.confirmUninstallClaudeBody"
            : "pluginsPage.confirmUninstallBody",
          { app: appName },
        )}
        confirmLabel={t("pluginsPage.uninstall")}
        onCancel={() => setUninstallTarget(null)}
        onConfirm={() => {
          const target = uninstallTarget;
          setUninstallTarget(null);
          if (target) void uninstall(target);
        }}
      />

      <V7ConfirmDialog
        open={installTarget !== null}
        title={t("pluginsPage.confirmInstallTitle", {
          name: installTarget ? pluginLabel(installTarget) : "",
        })}
        body={t("pluginsPage.confirmInstallBody", { app: appName })}
        details={
          installTarget && (
            <code className="rounded-[8px] bg-subtle px-3 py-2 font-mono text-caption text-fg-2 [overflow-wrap:anywhere]">
              {installTarget.id}
            </code>
          )
        }
        confirmLabel={t("pluginsPage.install")}
        danger={false}
        onCancel={() => setInstallTarget(null)}
        onConfirm={() => {
          const target = installTarget;
          setInstallTarget(null);
          if (target) void install(target);
        }}
      />

      <V7ConfirmDialog
        open={removeMarketTarget !== null}
        title={t("pluginsPage.confirmRemoveMarketTitle", {
          name: removeMarketTarget?.name ?? "",
        })}
        body={t("pluginsPage.confirmRemoveMarketBody", { app: appName })}
        confirmLabel={t("pluginsPage.removeMarketplace")}
        onCancel={() => setRemoveMarketTarget(null)}
        onConfirm={() => {
          const target = removeMarketTarget;
          setRemoveMarketTarget(null);
          if (target) void removeMarketplace(target);
        }}
      />

      <V7Dialog
        open={addOpen}
        onOpenChange={(next) => {
          if (!next && !busy["market:+"]) {
            setAddOpen(false);
            setSource("");
          }
        }}
      >
        <form
          className="flex flex-col gap-4"
          onSubmit={(event) => {
            event.preventDefault();
            void addMarketplace();
          }}
        >
          <div className="flex flex-col gap-1.5">
            <DialogTitle className="text-section">
              {t("pluginsPage.addMarketplaceTitle", { app: appName })}
            </DialogTitle>
            <DialogDescription className="text-body text-fg-2">
              {t("pluginsPage.addMarketplaceBody")}
            </DialogDescription>
          </div>
          <label className="flex flex-col gap-1.5">
            <span className={LABEL_CLASS}>{t("pluginsPage.sourceLabel")}</span>
            <input
              autoFocus
              className={cn(FIELD_CLASS, "font-mono text-caption")}
              value={source}
              placeholder="owner/repo"
              spellCheck={false}
              autoCapitalize="off"
              autoCorrect="off"
              disabled={Boolean(busy["market:+"])}
              onChange={(event) => setSource(event.target.value)}
            />
          </label>
          <div className="flex justify-end gap-2 pt-1">
            <Button
              type="button"
              variant="neutral"
              size="regular"
              disabled={Boolean(busy["market:+"])}
              onClick={() => {
                setAddOpen(false);
                setSource("");
              }}
            >
              {t("common.cancel")}
            </Button>
            <Button
              type="submit"
              variant="solid"
              size="regular"
              disabled={!source.trim() || Boolean(busy["market:+"])}
              aria-busy={Boolean(busy["market:+"])}
            >
              {busy["market:+"] && (
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
              )}
              {t("common.add")}
            </Button>
          </div>
        </form>
      </V7Dialog>
    </>
  );
}

/** 应用多时的选择器：按钮显示当前应用，点开是列表（同提示词页）。 */
function AppPicker({
  apps,
  value,
  onChange,
}: {
  apps: PluginApp[];
  value: PluginApp;
  onChange: (app: PluginApp) => void;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button
          type="button"
          variant="neutral"
          size="compact"
          aria-label={t("pluginsPage.appAria")}
          className="shrink-0 gap-1.5 pe-1.5 ps-2"
        >
          <AppGlyph app={value} size={14} />
          <span>{APP_DISPLAY_NAME[value]}</span>
          <ChevronDown
            aria-hidden="true"
            className="h-3.5 w-3.5 shrink-0 text-fg-2"
          />
        </Button>
      </PopoverTrigger>
      <PopoverContent
        align="end"
        aria-label={t("pluginsPage.appAria")}
        className="flex w-[240px] flex-col rounded-panel border-border bg-surface p-1 shadow-v7-md"
      >
        {apps.map((id) => {
          const selected = id === value;
          return (
            <button
              key={id}
              type="button"
              aria-pressed={selected}
              onClick={() => {
                setOpen(false);
                if (!selected) onChange(id);
              }}
              className={cn(
                "flex min-h-8 w-full items-center gap-2 rounded-control pe-2 ps-2.5 text-left text-body text-fg-1 transition-colors hover:bg-subtle",
                selected ? "font-medium" : "font-normal",
              )}
            >
              <AppGlyph app={id} size={16} badgeClassName="bg-surface" />
              <span className="min-w-0 flex-1 truncate">
                {APP_DISPLAY_NAME[id]}
              </span>
              <Check
                aria-hidden="true"
                strokeWidth={1.5}
                className={cn(
                  "h-3.5 w-3.5 shrink-0",
                  selected ? "visible" : "invisible",
                )}
              />
            </button>
          );
        })}
      </PopoverContent>
    </Popover>
  );
}

function CenteredState({
  title,
  body,
  detail,
  action,
}: {
  title: string;
  body?: string;
  detail?: string;
  action?: ReactNode;
}) {
  return (
    <div className="flex flex-1 flex-col items-center justify-center gap-2.5 px-10 pb-12 pt-8 text-center">
      <h2 className="m-0 text-section">{title}</h2>
      {detail && (
        <code className="max-w-[520px] whitespace-pre-wrap rounded-[8px] bg-subtle px-3 py-2 text-left font-mono text-caption text-fg-2 [overflow-wrap:anywhere]">
          {detail}
        </code>
      )}
      {body && <p className="m-0 max-w-[480px] text-body text-fg-2">{body}</p>}
      {action && <div className="mt-1">{action}</div>}
    </div>
  );
}

function PluginRow({
  entry,
  isLast,
  busyAction,
  canUpdate,
  onInstall,
  onToggle,
  onUpdate,
  onUninstall,
}: {
  entry: PluginEntry;
  isLast: boolean;
  busyAction?: string;
  /** 这个应用的 CLI 有更新命令 */
  canUpdate: boolean;
  onInstall: () => void;
  onToggle: (enabled: boolean) => void;
  onUpdate: () => void;
  onUninstall: () => void;
}) {
  const { t } = useTranslation();
  const label = pluginLabel(entry);
  const busy = Boolean(busyAction);
  const readOnlyScope = entry.installed && !entry.manageable;
  // 随应用自带、只能开关的插件（OpenClaw 的 bundled）
  const builtIn = entry.installed && entry.manageable && !isRemovable(entry);
  const showMenu = entry.installed && !builtIn;

  return (
    <div
      className={cn(
        "flex items-center gap-3 px-4 py-2.5",
        !isLast && "border-b border-border",
      )}
    >
      <div className="min-w-0 flex-1">
        <div className="flex min-w-0 items-center gap-2">
          <span className="truncate text-body font-medium text-fg-1">
            {label}
          </span>
          {entry.marketplace && (
            <span className="shrink-0 truncate font-mono text-caption text-fg-3">
              @{entry.marketplace}
            </span>
          )}
          {entry.version && (
            <span className="shrink-0 font-mono text-caption tabular-nums text-fg-3">
              {entry.version}
            </span>
          )}
          {builtIn && (
            <HoverTip content={t("pluginsPage.builtInPlugin")}>
              <span
                tabIndex={0}
                className="shrink-0 rounded-[4px] bg-subtle px-1.5 text-caption text-fg-2"
              >
                {t("pluginsPage.builtIn")}
              </span>
            </HoverTip>
          )}
          {entry.scope && entry.scope !== "user" && (
            <HoverTip
              content={t("pluginsPage.scopeReadOnly", { scope: entry.scope })}
            >
              <span
                tabIndex={0}
                className="shrink-0 rounded-[4px] bg-subtle px-1.5 text-caption text-fg-2"
              >
                {t("pluginsPage.scope", { scope: entry.scope })}
              </span>
            </HoverTip>
          )}
        </div>
        {entry.description && (
          <p
            className="m-0 truncate text-caption text-fg-2"
            title={entry.description}
          >
            {entry.description}
          </p>
        )}
      </div>

      {busy && (
        <Loader2
          className="h-4 w-4 shrink-0 animate-spin text-fg-2"
          aria-label={t("pluginsPage.working")}
        />
      )}

      {entry.installed ? (
        <>
          <Switch
            size="sm"
            checked={entry.enabled}
            disabled={busy || readOnlyScope}
            aria-label={t("pluginsPage.enableAria", { name: label })}
            onCheckedChange={onToggle}
          />
          {showMenu ? (
            <DropdownMenu>
              <HoverTip content={t("common.more")}>
                <DropdownMenuTrigger asChild>
                  <Button
                    type="button"
                    variant="quiet"
                    size="icon-compact"
                    disabled={busy || readOnlyScope}
                    aria-label={t("pluginsPage.moreActions", { name: label })}
                  >
                    <MoreHorizontal className="h-4 w-4" strokeWidth={1.5} />
                  </Button>
                </DropdownMenuTrigger>
              </HoverTip>
              <DropdownMenuContent align="end" className="min-w-[180px]">
                {canUpdate && (
                  <DropdownMenuItem onSelect={onUpdate}>
                    {t("pluginsPage.update")}
                  </DropdownMenuItem>
                )}
                <DropdownMenuItem
                  className="text-danger-text focus:text-danger-text"
                  onSelect={onUninstall}
                >
                  {t("pluginsPage.uninstall")}
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          ) : (
            // 占住菜单按钮的位置，开关和其它行对齐
            <span aria-hidden="true" className="size-7 shrink-0" />
          )}
        </>
      ) : entry.installable ? (
        <Button
          type="button"
          variant="neutral"
          size="compact"
          disabled={busy}
          onClick={onInstall}
        >
          <Download className="h-3.5 w-3.5" />
          {busyAction === "install"
            ? t("pluginsPage.installing")
            : t("pluginsPage.install")}
        </Button>
      ) : (
        <span className="shrink-0 text-caption text-fg-3">
          {t("pluginsPage.notInstallable")}
        </span>
      )}
    </div>
  );
}

export default PluginsPage;
