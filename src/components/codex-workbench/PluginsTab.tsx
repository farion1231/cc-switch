import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import {
  useCodexPluginMarketplaceStatus,
  useCodexPluginCaches,
  useCodexEffectiveHome,
  useInitializeCodexPluginMarketplace,
  useRefreshCodexPluginCache,
} from "@/lib/query/codexWorkbench";

export function PluginsTab() {
  const { t } = useTranslation();
  const homeQuery = useCodexEffectiveHome();
  const statusQuery = useCodexPluginMarketplaceStatus(true);
  const cachesQuery = useCodexPluginCaches(true);
  const initMut = useInitializeCodexPluginMarketplace();
  const refreshMut = useRefreshCodexPluginCache();

  const status = statusQuery.data;
  const caches = cachesQuery.data ?? [];

  return (
    <div className="space-y-6 p-1">
      <section className="rounded-lg border p-4 space-y-3">
        <div className="flex items-center justify-between gap-2">
          <h3 className="text-sm font-medium">
            {t("codexWorkbench.plugins.marketplace", {
              defaultValue: "插件市场",
            })}
          </h3>
          <div className="flex items-center gap-2">
            <Badge variant={status?.initialized ? "default" : "secondary"}>
              {status?.initialized
                ? t("codexWorkbench.plugins.ready", { defaultValue: "已就绪" })
                : t("codexWorkbench.plugins.notReady", {
                    defaultValue: "未初始化",
                  })}
            </Badge>
            <Button
              size="sm"
              disabled={initMut.isPending}
              onClick={() => initMut.mutate()}
            >
              {initMut.isPending
                ? t("codexWorkbench.plugins.initializing", {
                    defaultValue: "处理中…",
                  })
                : t("codexWorkbench.plugins.initOrRepair", {
                    defaultValue: "初始化/修复",
                  })}
            </Button>
          </div>
        </div>
        <p className="text-xs text-muted-foreground break-all">
          {t("codexWorkbench.plugins.home", { defaultValue: "CODEX_HOME" })}:{" "}
          {homeQuery.data ?? "…"}
        </p>
        {status?.marketplaceRoot && (
          <p className="text-xs text-muted-foreground break-all">
            {t("codexWorkbench.plugins.root", {
              defaultValue: "市场目录",
            })}
            : {status.marketplaceRoot}
          </p>
        )}
        {initMut.isError && (
          <p className="text-xs text-destructive">
            {(initMut.error as Error)?.message ?? String(initMut.error)}
          </p>
        )}
      </section>

      <section className="rounded-lg border p-4 space-y-3">
        <h3 className="text-sm font-medium">
          {t("codexWorkbench.plugins.cache", {
            defaultValue: "插件缓存",
          })}
        </h3>
        {cachesQuery.isLoading ? (
          <p className="text-sm text-muted-foreground">…</p>
        ) : caches.length === 0 ? (
          <p className="text-sm text-muted-foreground">
            {t("codexWorkbench.plugins.empty", {
              defaultValue: "暂无插件。请先初始化市场。",
            })}
          </p>
        ) : (
          <ul className="space-y-2">
            {caches.map((p) => (
              <li
                key={p.id}
                className="flex items-center justify-between gap-3 rounded-md border px-3 py-2"
              >
                <div className="min-w-0">
                  <div className="text-sm font-medium truncate">{p.id}</div>
                  <div className="text-xs text-muted-foreground">
                    src={p.sourceVersion ?? "—"} / cache=
                    {p.currentVersion ?? "—"}
                  </div>
                  <div className="text-xs text-muted-foreground">
                    {p.refreshReason}
                  </div>
                </div>
                <Button
                  size="sm"
                  variant="outline"
                  disabled={!p.canRefresh || refreshMut.isPending}
                  title={p.refreshReason}
                  onClick={() => refreshMut.mutate(p.id)}
                >
                  {t("codexWorkbench.plugins.refresh", {
                    defaultValue: "刷新缓存",
                  })}
                </Button>
              </li>
            ))}
          </ul>
        )}
        {refreshMut.isError && (
          <p className="text-xs text-destructive">
            {(refreshMut.error as Error)?.message ?? String(refreshMut.error)}
          </p>
        )}
      </section>
    </div>
  );
}

export default PluginsTab;
