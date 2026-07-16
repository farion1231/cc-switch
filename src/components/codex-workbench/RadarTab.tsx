import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { useCodexRadarQuery, useRefreshCodexRadar } from "@/lib/query/codexWorkbench";

export function RadarTab() {
  const { t } = useTranslation();
  const radarQ = useCodexRadarQuery();
  const refreshMut = useRefreshCodexRadar();

  const result = radarQ.data;
  const snap = result?.snapshot;
  const models = snap?.models ?? [];
  const comparisons = snap?.comparisons ?? [];

  return (
    <div className="space-y-4 p-1">
      <div className="flex flex-wrap items-center gap-2">
        <h3 className="text-sm font-medium">
          {t("codexWorkbench.radar.title", { defaultValue: "降智雷达" })}
        </h3>
        {result?.from_cache && (
          <Badge variant="secondary">
            {t("codexWorkbench.radar.cache", { defaultValue: "缓存" })}
          </Badge>
        )}
        {result?.stale && (
          <Badge variant="destructive">
            {t("codexWorkbench.radar.stale", { defaultValue: "已过期" })}
          </Badge>
        )}
        <div className="ml-auto flex gap-2">
          <Button
            size="sm"
            variant="outline"
            disabled={refreshMut.isPending || radarQ.isFetching}
            onClick={() => refreshMut.mutate()}
          >
            {t("codexWorkbench.radar.refresh", { defaultValue: "刷新" })}
          </Button>
          {snap?.sourceUrl && (
            <Button
              size="sm"
              variant="ghost"
              onClick={() => {
                // open external via window if available
                try {
                  window.open(snap.sourceUrl, "_blank", "noopener,noreferrer");
                } catch {
                  /* ignore */
                }
              }}
            >
              {t("codexWorkbench.radar.source", { defaultValue: "来源" })}
            </Button>
          )}
        </div>
      </div>

      {snap?.fetchedAt != null && (
        <p className="text-xs text-muted-foreground">
          {t("codexWorkbench.radar.fetchedAt", {
            defaultValue: "抓取时间",
          })}
          : {new Date(snap.fetchedAt * 1000).toLocaleString()}
        </p>
      )}

      {(result?.error || radarQ.isError) && (
        <p className="text-xs text-destructive">
          {result?.error ??
            (radarQ.error as Error)?.message ??
            String(radarQ.error)}
        </p>
      )}

      {radarQ.isLoading && !result && (
        <p className="text-sm text-muted-foreground">
          {t("codexWorkbench.radar.loading", { defaultValue: "加载中…" })}
        </p>
      )}

      {!radarQ.isLoading && models.length === 0 && (
        <p className="text-sm text-muted-foreground">
          {t("codexWorkbench.radar.empty", {
            defaultValue: "暂无雷达数据，请点击刷新",
          })}
        </p>
      )}

      {models.length > 0 && (
        <div className="grid gap-2 sm:grid-cols-2 lg:grid-cols-3">
          {models.map((m) => (
            <div
              key={m.model}
              className="rounded-md border p-3 space-y-1"
            >
              <div className="text-sm font-medium truncate" title={m.label || m.model}>
                {m.label || m.model}
              </div>
              <div className="text-2xl font-semibold tabular-nums">
                {Number.isFinite(m.score) ? m.score.toFixed(1) : "—"}
              </div>
              <div className="text-xs text-muted-foreground truncate">
                {m.model}
              </div>
            </div>
          ))}
        </div>
      )}

      {comparisons.length > 0 && (
        <section className="space-y-2">
          <h4 className="text-xs font-medium text-muted-foreground">
            {t("codexWorkbench.radar.comparisons", {
              defaultValue: "相邻对比",
            })}
          </h4>
          <ul className="space-y-1 text-sm">
            {comparisons.map((c, i) => (
              <li
                key={`${c.leftModel}-${c.rightModel}-${i}`}
                className="flex justify-between gap-2 border-b border-border/50 py-1"
              >
                <span className="truncate">
                  {c.leftModel} vs {c.rightModel}
                </span>
                <span className="tabular-nums text-muted-foreground">
                  Δ {c.delta >= 0 ? "+" : ""}
                  {c.delta.toFixed(2)}
                </span>
              </li>
            ))}
          </ul>
        </section>
      )}
    </div>
  );
}

export default RadarTab;
