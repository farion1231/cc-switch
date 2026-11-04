import {
  useDeferredValue,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
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
import { DialogTitle, DialogDescription } from "@/components/ui/dialog";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { AppPageHeader } from "@/components/shell/AppPageHeader";
import { AppGlyph, APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";
import {
  FIELD_CLASS,
  LABEL_CLASS,
  V7ConfirmDialog,
  V7Dialog,
} from "@/components/mcp/formBits";
import {
  installedFromCatalog,
  installedPluginsQuery,
  pluginCatalogQuery,
  pluginMarketplacesQuery,
  pluginPatches,
  usePluginWrites,
} from "@/hooks/usePlugins";
import {
  PLUGIN_APPS,
  PLUGIN_CLI_NOT_INSTALLED,
  pluginsApi,
  type PluginActionOutcome,
  type PluginApp,
  type PluginEntry,
  type PluginMarketplace,
} from "@/lib/api/plugins";

type PluginsView = "installed" | "discover" | "marketplaces";

/** 发现页一次最多画这么多行（Claude Code 的官方目录有几千个插件），其余靠搜索缩小。 */
export const DISCOVER_RENDER_LIMIT = 200;

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
  const byId = new Map(catalog.map((entry) => [entry.id, entry]));
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
  return catalog.filter((entry) => !ids.has(entry.id));
}

function pluginLabel(entry: PluginEntry) {
  return entry.displayName || entry.name;
}

export interface PluginsPageProps {
  /** 分段控件里的应用（只列「应用」页里设为显示的）；为空时两个都列 */
  apps?: PluginApp[];
  initialApp?: PluginApp;
}

type WriteMessages = {
  success: (outcome: PluginActionOutcome) => string;
  failure: string;
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
  const [removeMarketTarget, setRemoveMarketTarget] =
    useState<PluginMarketplace | null>(null);
  const [addOpen, setAddOpen] = useState(false);
  const [source, setSource] = useState("");

  const appName = APP_DISPLAY_NAME[app];

  // 已安装和目录同时起：已安装那条便宜，先回来先显示
  const installedQuery = useQuery(installedPluginsQuery(app));
  const catalogQuery = useQuery(pluginCatalogQuery(app));
  const marketplaces = useQuery({
    ...pluginMarketplacesQuery(app),
    enabled: view === "marketplaces" || addOpen,
  });

  // 其它应用的已安装列表先在后台读好，切过去不用等
  useEffect(() => {
    for (const other of appChoicesKey.split(",") as PluginApp[]) {
      if (other !== app) {
        void queryClient.prefetchQuery(installedPluginsQuery(other));
      }
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
  const available = useMemo(
    () => filterPlugins(availableAll, deferredSearch),
    [availableAll, deferredSearch],
  );

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
        description: t("pluginsPage.restartNote", { app: appName }),
        closeButton: true,
      });
    } else {
      toast.error(messages.failure, {
        description: extractErrorMessage(result.error) || String(result.error),
        closeButton: true,
      });
    }
    return result.ok;
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
        <>
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
            {t("pluginsPage.refreshMarketplaces")}
          </Button>
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
        </>
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
        { value: "marketplaces", label: t("pluginsPage.viewMarketplaces") },
      ]}
      trailing={
        appChoices.length > 1 ? (
          <SegmentedControl<PluginApp>
            size="sm"
            aria-label={t("pluginsPage.appAria")}
            value={app}
            onValueChange={(next) => {
              setApp(next);
              setSearch("");
            }}
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
          body={t("pluginsPage.emptyDiscoverBody")}
        />
      );
    }

    const shown =
      view === "discover" ? rows.slice(0, DISCOVER_RENDER_LIMIT) : rows;
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
            {t("pluginsPage.restartNote", { app: appName })}
          </span>
        </div>
        {rows.length === 0 ? (
          <CenteredState
            title={t("pluginsPage.noMatches", {
              query: deferredSearch.trim(),
            })}
          />
        ) : (
          <div
            data-testid={`plugins-${view}`}
            className="min-h-0 overflow-auto scroll-stable rounded-panel border border-border bg-surface"
          >
            {shown.map((entry, index) => (
              <PluginRow
                key={`${entry.id}:${entry.scope ?? ""}`}
                entry={entry}
                isLast={index === shown.length - 1}
                busyAction={busy[entry.id]}
                onInstall={() => void install(entry)}
                onToggle={(enabled) => void toggle(entry, enabled)}
                onUpdate={() => void update(entry)}
                onUninstall={() => setUninstallTarget(entry)}
              />
            ))}
            {shown.length < rows.length && (
              <p className="m-0 border-t border-border px-4 py-2.5 text-caption text-fg-2">
                {t("pluginsPage.truncated", {
                  shown: shown.length,
                  count: rows.length,
                })}
              </p>
            )}
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
    const markets = marketplaces.data ?? [];
    if (markets.length === 0) {
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
      <div
        data-testid="plugins-marketplaces"
        className="mt-3 min-h-0 overflow-auto scroll-stable rounded-panel border border-border bg-surface"
      >
        {markets.map((market, index) => {
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
                  {removing && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
                  {t("pluginsPage.removeMarketplace")}
                </Button>
              )}
            </div>
          );
        })}
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
        body={t("pluginsPage.confirmUninstallBody", { app: appName })}
        confirmLabel={t("pluginsPage.uninstall")}
        onCancel={() => setUninstallTarget(null)}
        onConfirm={() => {
          const target = uninstallTarget;
          setUninstallTarget(null);
          if (target) void uninstall(target);
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
  onInstall,
  onToggle,
  onUpdate,
  onUninstall,
}: {
  entry: PluginEntry;
  isLast: boolean;
  busyAction?: string;
  onInstall: () => void;
  onToggle: (enabled: boolean) => void;
  onUpdate: () => void;
  onUninstall: () => void;
}) {
  const { t } = useTranslation();
  const label = pluginLabel(entry);
  const busy = Boolean(busyAction);
  const readOnlyScope = entry.installed && !entry.manageable;

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
          <span className="shrink-0 truncate font-mono text-caption text-fg-3">
            @{entry.marketplace}
          </span>
          {entry.version && (
            <span className="shrink-0 font-mono text-caption tabular-nums text-fg-3">
              {entry.version}
            </span>
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
              <DropdownMenuItem onSelect={onUpdate}>
                {t("pluginsPage.update")}
              </DropdownMenuItem>
              <DropdownMenuItem
                className="text-danger-text focus:text-danger-text"
                onSelect={onUninstall}
              >
                {t("pluginsPage.uninstall")}
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
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
