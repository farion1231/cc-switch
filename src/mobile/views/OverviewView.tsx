import {
  Activity,
  AlertCircle,
  CheckCircle2,
  Coins,
  Flame,
  Gauge,
  Zap,
} from "lucide-react";
import type { OverviewResponse } from "../api";
import {
  computeRowTps,
  formatAppName,
  formatCompactNumber,
  formatPercent,
  formatTps,
  formatUsd,
} from "../format";
import { tr, type MobileLang } from "../i18n";

interface OverviewViewProps {
  lang: MobileLang;
  days: 1 | 7 | 30;
  onChangeDays: (days: 1 | 7 | 30) => void;
  data: OverviewResponse | null;
  loading: boolean;
  error: string | null;
  onRetry: () => void;
}

export function OverviewView({
  lang,
  days,
  onChangeDays,
  data,
  loading,
  error,
  onRetry,
}: OverviewViewProps) {
  const periods: Array<{ value: 1 | 7 | 30; label: string }> = [
    { value: 1, label: tr(lang, "period24h") },
    { value: 7, label: tr(lang, "period7d") },
    { value: 30, label: tr(lang, "period30d") },
  ];

  const maxCost = Math.max(
    ...(data?.dailyTrends.map((d) => Number.parseFloat(d.totalCost) || 0) ?? [
      0,
    ]),
    0.0001,
  );

  return (
    <div className="space-y-4">
      {/* Period Switcher */}
      <div className="grid grid-cols-3 rounded-xl bg-zinc-200/70 p-1 dark:bg-zinc-800/80">
        {periods.map((p) => (
          <button
            key={p.value}
            type="button"
            onClick={() => onChangeDays(p.value)}
            className={`min-h-[38px] rounded-lg text-xs font-semibold transition ${
              days === p.value
                ? "bg-white text-zinc-900 shadow-sm dark:bg-zinc-900 dark:text-zinc-100"
                : "text-zinc-600 dark:text-zinc-400"
            }`}
          >
            {p.label}
          </button>
        ))}
      </div>

      {error && (
        <div className="flex items-center justify-between gap-2 rounded-2xl border border-red-200 bg-red-50 px-4 py-3 text-xs text-red-700 dark:border-red-900/50 dark:bg-red-950/30 dark:text-red-300">
          <span className="flex items-center gap-2">
            <AlertCircle className="h-4 w-4 shrink-0" />
            {error}
          </span>
          <button
            type="button"
            onClick={onRetry}
            className="shrink-0 rounded-lg bg-red-600 px-3 py-1.5 font-semibold text-white"
          >
            {tr(lang, "retry")}
          </button>
        </div>
      )}

      {loading && !data ? (
        <div className="grid grid-cols-2 gap-3">
          {Array.from({ length: 6 }).map((_, i) => (
            <div
              key={i}
              className="h-24 animate-pulse rounded-2xl bg-zinc-200/60 dark:bg-zinc-800/60"
            />
          ))}
        </div>
      ) : data ? (
        <>
          {/* 2-column KPI Grid */}
          <div className="grid grid-cols-2 gap-3">
            <KpiCard
              icon={<Coins className="h-4 w-4 text-[#f97316]" />}
              label={tr(lang, "kpiCost")}
              value={formatUsd(data.summary.totalCost)}
              accent
            />
            <KpiCard
              icon={<Flame className="h-4 w-4 text-amber-500" />}
              label={tr(lang, "kpiTokens")}
              value={formatCompactNumber(data.summary.realTotalTokens)}
            />
            <KpiCard
              icon={<Activity className="h-4 w-4 text-sky-500" />}
              label={tr(lang, "kpiRequests")}
              value={formatCompactNumber(data.summary.totalRequests)}
            />
            <KpiCard
              icon={<Zap className="h-4 w-4 text-emerald-500" />}
              label={tr(lang, "kpiCacheHit")}
              value={formatPercent(data.summary.cacheHitRate)}
            />
            <KpiCard
              icon={<Gauge className="h-4 w-4 text-violet-500" />}
              label={tr(lang, "kpiSpeed")}
              value={formatTps(data.aggregateTps, data.aggregateTpsEstimated)}
            />
            <KpiCard
              icon={<CheckCircle2 className="h-4 w-4 text-teal-500" />}
              label={tr(lang, "kpiSuccess")}
              value={`${data.summary.successRate.toFixed(1)}%`}
            />
          </div>

          {/* Daily Trend Bars */}
          <section className="rounded-2xl border border-zinc-200/80 bg-white p-4 dark:border-zinc-800 dark:bg-[#232328]">
            <h2 className="mb-3 text-xs font-semibold uppercase tracking-wider text-zinc-500 dark:text-zinc-400">
              {tr(lang, "dailyTrend")}
            </h2>
            {data.dailyTrends.length === 0 ? (
              <p className="py-6 text-center text-xs text-zinc-400">
                {tr(lang, "noData")}
              </p>
            ) : (
              <div className="flex h-28 items-end gap-1.5">
                {data.dailyTrends.slice(-14).map((day) => {
                  const cost = Number.parseFloat(day.totalCost) || 0;
                  const pct = Math.max(6, Math.round((cost / maxCost) * 100));
                  return (
                    <div
                      key={day.date}
                      className="flex flex-1 flex-col items-center gap-1"
                      title={`${day.date}: ${formatUsd(cost)} (${day.requestCount})`}
                    >
                      <div className="flex h-20 w-full items-end">
                        <div
                          className="w-full rounded-t-md bg-[#f97316]/85 transition-all"
                          style={{ height: `${pct}%` }}
                        />
                      </div>
                      <span className="text-[9px] tabular-nums text-zinc-400">
                        {day.date.slice(5)}
                      </span>
                    </div>
                  );
                })}
              </div>
            )}
          </section>

          {/* Active Providers Strip */}
          {data.activeApps.length > 0 && (
            <section className="rounded-2xl border border-zinc-200/80 bg-white p-4 dark:border-zinc-800 dark:bg-[#232328]">
              <h2 className="mb-3 text-xs font-semibold uppercase tracking-wider text-zinc-500 dark:text-zinc-400">
                {tr(lang, "activeProviders")}
              </h2>
              <div className="divide-y divide-zinc-100 dark:divide-zinc-800">
                {data.activeApps.map((item) => (
                  <div
                    key={item.app}
                    className="flex items-center justify-between gap-3 py-2.5 first:pt-0 last:pb-0"
                  >
                    <span className="text-xs font-medium text-zinc-500 dark:text-zinc-400">
                      {formatAppName(item.app)}
                    </span>
                    <span className="truncate text-sm font-semibold text-zinc-900 dark:text-zinc-100">
                      {item.currentProviderName ?? "—"}
                    </span>
                  </div>
                ))}
              </div>
            </section>
          )}

          {/* Top Models */}
          {data.modelStats.length > 0 && (
            <section className="rounded-2xl border border-zinc-200/80 bg-white p-4 dark:border-zinc-800 dark:bg-[#232328]">
              <h2 className="mb-3 text-xs font-semibold uppercase tracking-wider text-zinc-500 dark:text-zinc-400">
                {tr(lang, "topModels")}
              </h2>
              <div className="space-y-2.5">
                {data.modelStats.slice(0, 5).map((m) => {
                  const speed = computeRowTps(m);
                  return (
                    <div
                      key={m.model}
                      className="flex items-center justify-between gap-2"
                    >
                      <div className="min-w-0">
                        <div className="truncate text-sm font-medium text-zinc-900 dark:text-zinc-100">
                          {m.model}
                        </div>
                        <div className="mt-0.5 flex items-center gap-2 text-[11px] tabular-nums text-zinc-500 dark:text-zinc-400">
                          <span>{formatCompactNumber(m.requestCount)} req</span>
                          <span>·</span>
                          <span>{formatCompactNumber(m.totalTokens)} tok</span>
                          {speed.tps && (
                            <>
                              <span>·</span>
                              <span>
                                {formatTps(speed.tps, speed.estimated)}
                              </span>
                            </>
                          )}
                        </div>
                      </div>
                      <span className="shrink-0 text-sm font-semibold tabular-nums text-zinc-900 dark:text-zinc-100">
                        {formatUsd(m.totalCost)}
                      </span>
                    </div>
                  );
                })}
              </div>
            </section>
          )}
        </>
      ) : null}
    </div>
  );
}

function KpiCard({
  icon,
  label,
  value,
  accent,
}: {
  icon: React.ReactNode;
  label: string;
  value: string;
  accent?: boolean;
}) {
  return (
    <div
      className={`rounded-2xl border p-3.5 ${
        accent
          ? "border-orange-500/30 bg-gradient-to-br from-orange-500/10 to-white dark:from-orange-500/15 dark:to-[#232328]"
          : "border-zinc-200/80 bg-white dark:border-zinc-800 dark:bg-[#232328]"
      }`}
    >
      <div className="flex items-center gap-1.5 text-xs font-medium text-zinc-500 dark:text-zinc-400">
        {icon}
        <span>{label}</span>
      </div>
      <div className="mt-2 truncate text-xl font-bold tracking-tight tabular-nums text-zinc-900 dark:text-zinc-100">
        {value}
      </div>
    </div>
  );
}
