import { useCallback, useEffect, useState } from "react";
import { useQueryClient, type QueryClient } from "@tanstack/react-query";
import { readLocalCache, writeLocalCache } from "@/lib/localCache";
import {
  pluginsApi,
  type PluginActionOutcome,
  type PluginApp,
  type PluginEntry,
} from "@/lib/api/plugins";

/**
 * 插件页的数据：已安装、市场目录、市场三份查询，外加带乐观更新的写操作。
 *
 * 每次查询都要起一次 CLI（Claude Code 约 1s），所以：
 * - 已安装列表小，存一份本地缓存，打开页面先显示上次的结果，后台再刷新；
 * - 目录大（Claude Code 官方目录几千项）且只随市场变，只放内存，装、卸都不重读；
 * - 启停、卸载先改界面，失败再改回；装好后直接把条目加进已安装；
 * - 同一应用的操作全部结束后才在后台对一次账，中途不让刷新覆盖掉乐观状态。
 */

export const pluginKeys = {
  all: ["plugins"] as const,
  installed: (app: PluginApp) => ["plugins", app, "installed"] as const,
  catalog: (app: PluginApp) => ["plugins", app, "catalog"] as const,
  marketplaces: (app: PluginApp) => ["plugins", app, "marketplaces"] as const,
};

const installedCacheKey = (app: PluginApp) => `plugins:${app}:installed`;

/** 每个应用进行中的写操作数，和操作结束后要重读的查询。 */
const pending = new Map<PluginApp, number>();
const reconcileTargets = new Map<PluginApp, Set<ReconcileTarget>>();

export const hasPendingPluginWrites = (app: PluginApp) =>
  (pending.get(app) ?? 0) > 0;

/** 本次运行里已经在后台预读过已安装列表的应用。 */
const prefetched = new Set<PluginApp>();

/** 只给测试用：用例之间清掉模块级状态。 */
export function resetPluginWriteState() {
  pending.clear();
  reconcileTargets.clear();
  prefetched.clear();
}

/** 有写操作在跑时不自动刷新：刷新读到的是操作前的状态，会把乐观结果盖回去。 */
const idleOnly = (app: PluginApp) => () => !hasPendingPluginWrites(app);

export const installedPluginsQuery = (app: PluginApp) => ({
  queryKey: pluginKeys.installed(app),
  queryFn: async () => {
    const result = await pluginsApi.listInstalled(app);
    if (!hasPendingPluginWrites(app)) {
      writeLocalCache(installedCacheKey(app), result);
    }
    return result;
  },
  initialData: () => readLocalCache<PluginEntry[]>(installedCacheKey(app)),
  // 本地缓存是旧数据：时间记成 0，挂载时照常在后台刷新
  initialDataUpdatedAt: 0,
  staleTime: 15_000,
  retry: false,
  refetchOnMount: idleOnly(app),
  // 用户可能在终端里用 CLI 改过：切回窗口时重读（便宜的那条）
  refetchOnWindowFocus: idleOnly(app),
});

/**
 * 在后台读好另一个应用的已安装列表，切过去不用等。每个应用一次运行只预读一次：应用多了以后
 * 每次打开页面都给每个应用起一个 CLI 太重，之后切过去时本地缓存先显示、再在后台刷新。
 */
export function prefetchInstalledOnce(
  queryClient: QueryClient,
  app: PluginApp,
) {
  if (prefetched.has(app)) return;
  prefetched.add(app);
  void queryClient.prefetchQuery(installedPluginsQuery(app));
}

export const pluginCatalogQuery = (app: PluginApp) => ({
  queryKey: pluginKeys.catalog(app),
  queryFn: () => pluginsApi.listCatalog(app),
  staleTime: 5 * 60_000,
  retry: false,
  refetchOnWindowFocus: false,
});

export const pluginMarketplacesQuery = (app: PluginApp) => ({
  queryKey: pluginKeys.marketplaces(app),
  queryFn: () => pluginsApi.listMarketplaces(app),
  staleTime: 60_000,
  retry: false,
  refetchOnWindowFocus: false,
});

type ReconcileTarget = "installed" | "catalog" | "marketplaces";
type InstalledPatch = (entries: PluginEntry[]) => PluginEntry[];

const sameEntry = (a: PluginEntry, b: PluginEntry) =>
  a.id === b.id && (a.scope ?? null) === (b.scope ?? null);

/** 乐观补丁：都按条目 id 作用在「当前」数据上，几个操作交错时互不覆盖。 */
export const pluginPatches = {
  setEnabled:
    (target: PluginEntry, enabled: boolean): InstalledPatch =>
    (entries) =>
      entries.map((e) => (sameEntry(e, target) ? { ...e, enabled } : e)),
  remove:
    (target: PluginEntry): InstalledPatch =>
    (entries) =>
      entries.filter((e) => !sameEntry(e, target)),
  /** 加回（卸载失败时）或加入（装好后）；已经在就不重复加。 */
  add:
    (target: PluginEntry, index?: number): InstalledPatch =>
    (entries) => {
      if (entries.some((e) => sameEntry(e, target))) return entries;
      const next = [...entries];
      next.splice(index ?? next.length, 0, target);
      return next;
    },
};

/** 目录条目装好后在已安装列表里的 id（OpenClaw 的目录给的是包名）。 */
export const installedIdOf = (entry: PluginEntry) =>
  entry.installedId || entry.id;

/** 能否卸载、更新；旧缓存里的条目没有 removable，按 manageable 算。 */
export const isRemovable = (entry: PluginEntry) =>
  entry.removable ?? entry.manageable;

/** 装好后先放进已安装的那一项；版本、路径等后台对账时补齐。 */
export function installedFromCatalog(
  app: PluginApp,
  entry: PluginEntry,
): PluginEntry {
  const id = installedIdOf(entry);
  return {
    ...entry,
    id,
    name: id === entry.id ? entry.name : id,
    // 没有市场概念的应用（OpenClaw）已装条目不显示来源
    marketplace: app === "openclaw" ? "" : entry.marketplace,
    installedId: null,
    installed: true,
    enabled: true,
    manageable: true,
    removable: true,
    scope: app === "claude" ? "user" : null,
  };
}

interface PluginWrite {
  /** 行上转圈用的 key：插件 id，或 `market:*` 这类 */
  key: string;
  action: string;
  task: () => Promise<PluginActionOutcome>;
  /** 发出请求前就改界面；失败时用 revert 改回 */
  optimistic?: { apply: InstalledPatch; revert: InstalledPatch };
  /** 成功后立即改界面（不等对账） */
  onSuccess?: InstalledPatch;
  /** 操作结束后要在后台重读的查询 */
  reconcile: ReconcileTarget[];
}

function patchInstalled(
  queryClient: QueryClient,
  app: PluginApp,
  patch: InstalledPatch,
) {
  queryClient.setQueryData<PluginEntry[]>(pluginKeys.installed(app), (old) =>
    old ? patch(old) : old,
  );
}

function finishWrite(
  queryClient: QueryClient,
  app: PluginApp,
  targets: ReconcileTarget[],
) {
  const set = reconcileTargets.get(app) ?? new Set<ReconcileTarget>();
  targets.forEach((t) => set.add(t));
  reconcileTargets.set(app, set);
  const left = (pending.get(app) ?? 1) - 1;
  pending.set(app, left);
  if (left > 0) return;
  // 最后一个操作结束：攒下的查询一起对一次账
  reconcileTargets.delete(app);
  for (const target of set) {
    void queryClient.invalidateQueries({
      queryKey: pluginKeys[target](app),
      exact: true,
    });
  }
}

export interface PluginWriteResult {
  ok: boolean;
  outcome?: PluginActionOutcome;
  error?: unknown;
}

/** 插件页的写操作。`busy` 是进行中操作的 key → 操作名，供行内显示转圈。 */
export function usePluginWrites(app: PluginApp) {
  const queryClient = useQueryClient();
  const [busy, setBusy] = useState<Record<string, string>>({});

  // 换应用时转圈状态不跟过去（那边的操作照常在后台跑完、对账）
  useEffect(() => setBusy({}), [app]);

  const run = useCallback(
    async (write: PluginWrite): Promise<PluginWriteResult> => {
      pending.set(app, (pending.get(app) ?? 0) + 1);
      setBusy((prev) => ({ ...prev, [write.key]: write.action }));
      try {
        if (write.optimistic) {
          // 正在跑的刷新读的是操作前的状态，晚到会把乐观结果盖掉
          await queryClient.cancelQueries({
            queryKey: pluginKeys.installed(app),
            exact: true,
          });
          patchInstalled(queryClient, app, write.optimistic.apply);
        }
        try {
          const outcome = await write.task();
          if (write.onSuccess)
            patchInstalled(queryClient, app, write.onSuccess);
          return { ok: true, outcome };
        } catch (error) {
          if (write.optimistic) {
            patchInstalled(queryClient, app, write.optimistic.revert);
          }
          return { ok: false, error };
        }
      } finally {
        setBusy((prev) => {
          const next = { ...prev };
          delete next[write.key];
          return next;
        });
        finishWrite(queryClient, app, write.reconcile);
      }
    },
    [app, queryClient],
  );

  return { busy, run };
}
