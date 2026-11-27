import { useState } from "react";
import type { OverviewResponse } from "../api";
import {
  computeRowTps,
  formatAppName,
  formatCompactNumber,
  formatLatency,
  formatPercent,
  formatTps,
  formatUsd,
} from "../format";
import { tr, type MobileLang } from "../i18n";

interface ModelsViewProps {
  lang: MobileLang;
  data: OverviewResponse | null;
  loading: boolean;
}

export function ModelsView({ lang, data, loading }: ModelsViewProps) {
  const [tab, setTab] = useState<"models" | "providers">("models");

  return (
    <div className="space-y-3">
      <div className="grid grid-cols-2 rounded-xl bg-zinc-200/70 p-1 dark:bg-zinc-800/80">
        <button
          type="button"
          onClick={() => setTab("models")}
          className={`min-h-[38px] rounded-lg text-xs font-semibold transition ${
            tab === "models"
              ? "bg-white text-zinc-900 shadow-sm dark:bg-zinc-900 dark:text-zinc-100"
              : "text-zinc-600 dark:text-zinc-400"
          }`}
        >
          {tr(lang, "tabModels")}
        </button>
        <button
          type="button"
          onClick={() => setTab("providers")}
          className={`min-h-[38px] rounded-lg text-xs font-semibold transition ${
            tab === "providers"
              ? "bg-white text-zinc-900 shadow-sm dark:bg-zinc-900 dark:text-zinc-100"
              : "text-zinc-600 dark:text-zinc-400"
          }`}
        >
          {tr(lang, "tabProviders")}
        </button>
      </div>

      {loading && !data ? (
        <div className="space-y-2.5">
          {Array.from({ length: 5 }).map((_, i) => (
            <div
              key={i}
              className="h-24 animate-pulse rounded-2xl bg-zinc-200/60 dark:bg-zinc-800/60"
            />
          ))}
        </div>
      ) : tab === "models" ? (
        (data?.modelStats.length ?? 0) === 0 ? (
          <EmptyCard text={tr(lang, "noData")} />
        ) : (
          <div className="space-y-2.5">
            {data!.modelStats.map((m) => {
              const speed = computeRowTps(m);
              const cacheHit =
                m.cacheableInputTokens > 0
                  ? m.cacheReadTokens / m.cacheableInputTokens
                  : 0;
              return (
                <div
                  key={m.model}
                  className="rounded-2xl border border-zinc-200/80 bg-white p-3.5 dark:border-zinc-800 dark:bg-[#232328]"
                >
                  <div className="flex items-center justify-between gap-2">
                    <span className="truncate text-sm font-semibold text-zinc-900 dark:text-zinc-100">
                      {m.model}
                    </span>
                    <span className="shrink-0 text-sm font-bold tabular-nums text-[#f97316]">
                      {formatUsd(m.totalCost)}
                    </span>
                  </div>
                  <div className="mt-2 grid grid-cols-3 gap-2 text-[11px] tabular-nums text-zinc-500 dark:text-zinc-400">
                    <div>
                      <div>{tr(lang, "kpiRequests")}</div>
                      <div className="font-semibold text-zinc-800 dark:text-zinc-200">
                        {formatCompactNumber(m.requestCount)}
                      </div>
                    </div>
                    <div>
                      <div>{tr(lang, "kpiTokens")}</div>
                      <div className="font-semibold text-zinc-800 dark:text-zinc-200">
                        {formatCompactNumber(m.totalTokens)}
                      </div>
                    </div>
                    <div>
                      <div>{tr(lang, "kpiCacheHit")}</div>
                      <div className="font-semibold text-zinc-800 dark:text-zinc-200">
                        {formatPercent(cacheHit)}
                      </div>
                    </div>
                    <div>
                      <div>{tr(lang, "ttft")}</div>
                      <div className="font-semibold text-zinc-800 dark:text-zinc-200">
                        {formatLatency(
                          m.firstTokenMedianMs ?? m.estFirstTokenMedianMs,
                        )}
                      </div>
                    </div>
                    <div className="col-span-2">
                      <div>{tr(lang, "kpiSpeed")}</div>
                      <div className="font-semibold text-zinc-800 dark:text-zinc-200">
                        {formatTps(speed.tps, speed.estimated)}
                      </div>
                    </div>
                  </div>
                </div>
              );
            })}
          </div>
        )
      ) : (data?.providerStats.length ?? 0) === 0 ? (
        <EmptyCard text={tr(lang, "noData")} />
      ) : (
        <div className="space-y-2.5">
          {data!.providerStats.map((p) => {
            const speed = computeRowTps(p);
            return (
              <div
                key={`${p.appType}:${p.providerId}`}
                className="rounded-2xl border border-zinc-200/80 bg-white p-3.5 dark:border-zinc-800 dark:bg-[#232328]"
              >
                <div className="flex items-center justify-between gap-2">
                  <div className="min-w-0">
                    <div className="truncate text-sm font-semibold text-zinc-900 dark:text-zinc-100">
                      {p.providerName}
                    </div>
                    <div className="text-[11px] text-zinc-500 dark:text-zinc-400">
                      {formatAppName(p.appType)}
                    </div>
                  </div>
                  <span className="shrink-0 text-sm font-bold tabular-nums text-[#f97316]">
                    {formatUsd(p.totalCost)}
                  </span>
                </div>
                <div className="mt-2 flex flex-wrap items-center gap-x-3 gap-y-1 text-[11px] tabular-nums text-zinc-500 dark:text-zinc-400">
                  <span>{formatCompactNumber(p.requestCount)} req</span>
                  <span>{formatCompactNumber(p.totalTokens)} tok</span>
                  <span>{p.successRate.toFixed(1)}% ok</span>
                  <span>{formatTps(speed.tps, speed.estimated)}</span>
                </div>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

function EmptyCard({ text }: { text: string }) {
  return (
    <div className="rounded-2xl border border-zinc-200/80 bg-white px-4 py-10 text-center text-sm text-zinc-500 dark:border-zinc-800 dark:bg-[#232328]">
      {text}
    </div>
  );
}
