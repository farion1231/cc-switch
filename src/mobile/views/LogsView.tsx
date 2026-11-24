import { useState } from "react";
import { AlertCircle, ChevronLeft, ChevronRight, X } from "lucide-react";
import type { SafePaginatedLogs, SafeRequestLog } from "../api";
import {
  formatAppName,
  formatCompactNumber,
  formatLatency,
  formatRelativeTime,
  formatTps,
  formatUsd,
} from "../format";
import { tr, type MobileLang } from "../i18n";

interface LogsViewProps {
  lang: MobileLang;
  data: SafePaginatedLogs | null;
  page: number;
  statusFilter: "all" | "ok" | "err";
  loading: boolean;
  error: string | null;
  onChangePage: (page: number) => void;
  onChangeStatusFilter: (filter: "all" | "ok" | "err") => void;
  onRetry: () => void;
}

export function LogsView({
  lang,
  data,
  page,
  statusFilter,
  loading,
  error,
  onChangePage,
  onChangeStatusFilter,
  onRetry,
}: LogsViewProps) {
  const [selected, setSelected] = useState<SafeRequestLog | null>(null);

  const filtered = data?.data ?? [];

  const totalPages = data
    ? Math.max(1, Math.ceil(data.total / data.pageSize))
    : 1;

  return (
    <div className="space-y-3">
      {/* Status Filter */}
      <div className="grid grid-cols-3 rounded-xl bg-zinc-200/70 p-1 dark:bg-zinc-800/80">
        {(
          [
            ["all", tr(lang, "statusAll")],
            ["ok", tr(lang, "statusOk")],
            ["err", tr(lang, "statusErr")],
          ] as const
        ).map(([val, label]) => (
          <button
            key={val}
            type="button"
            onClick={() => onChangeStatusFilter(val)}
            className={`min-h-[36px] rounded-lg text-xs font-semibold transition ${
              statusFilter === val
                ? "bg-white text-zinc-900 shadow-sm dark:bg-zinc-900 dark:text-zinc-100"
                : "text-zinc-600 dark:text-zinc-400"
            }`}
          >
            {label}
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
        <div className="space-y-2.5">
          {Array.from({ length: 6 }).map((_, i) => (
            <div
              key={i}
              className="h-20 animate-pulse rounded-2xl bg-zinc-200/60 dark:bg-zinc-800/60"
            />
          ))}
        </div>
      ) : filtered.length === 0 ? (
        <div className="rounded-2xl border border-zinc-200/80 bg-white px-4 py-10 text-center text-sm text-zinc-500 dark:border-zinc-800 dark:bg-[#232328]">
          {tr(lang, "noData")}
        </div>
      ) : (
        <div className="space-y-2">
          {filtered.map((log) => {
            const isOk = log.statusCode >= 200 && log.statusCode < 300;
            return (
              <button
                key={log.requestId}
                type="button"
                onClick={() => setSelected(log)}
                className="w-full rounded-2xl border border-zinc-200/80 bg-white p-3.5 text-left transition active:scale-[0.99] dark:border-zinc-800 dark:bg-[#232328]"
              >
                <div className="flex items-center justify-between gap-2">
                  <div className="flex min-w-0 items-center gap-2">
                    <span
                      className={`inline-flex shrink-0 rounded-md px-1.5 py-0.5 text-[11px] font-bold tabular-nums ${
                        isOk
                          ? "bg-emerald-500/15 text-emerald-700 dark:text-emerald-400"
                          : "bg-red-500/15 text-red-600 dark:text-red-400"
                      }`}
                    >
                      {log.statusCode}
                    </span>
                    <span className="truncate text-sm font-semibold text-zinc-900 dark:text-zinc-100">
                      {log.model}
                    </span>
                  </div>
                  <span className="shrink-0 text-xs font-semibold tabular-nums text-zinc-900 dark:text-zinc-100">
                    {formatUsd(log.totalCostUsd)}
                  </span>
                </div>

                <div className="mt-1.5 flex flex-wrap items-center justify-between gap-2 text-[11px] text-zinc-500 dark:text-zinc-400">
                  <span className="truncate">
                    {formatAppName(log.appType)} ·{" "}
                    {log.providerName ?? log.providerId}
                  </span>
                  <span className="shrink-0 tabular-nums">
                    {formatRelativeTime(log.createdAt)}
                  </span>
                </div>

                <div className="mt-2 flex flex-wrap items-center gap-x-3 gap-y-1 text-[11px] tabular-nums text-zinc-500 dark:text-zinc-400">
                  <span>
                    {formatCompactNumber(log.inputTokens)}→
                    {formatCompactNumber(log.outputTokens)} tok
                  </span>
                  <span>TTFT {formatLatency(log.firstTokenMs)}</span>
                  <span>{formatTps(log.tps, log.tpsEstimated)}</span>
                </div>
              </button>
            );
          })}
        </div>
      )}

      {/* Pagination */}
      {data && data.total > data.pageSize && (
        <div className="flex items-center justify-between pt-1">
          <button
            type="button"
            disabled={page <= 1}
            onClick={() => onChangePage(page - 1)}
            className="flex min-h-[40px] items-center gap-1 rounded-xl border border-zinc-200 bg-white px-3.5 text-xs font-medium text-zinc-700 disabled:opacity-40 dark:border-zinc-800 dark:bg-[#232328] dark:text-zinc-200"
          >
            <ChevronLeft className="h-4 w-4" />
            {tr(lang, "prevPage")}
          </button>
          <span className="text-xs font-medium tabular-nums text-zinc-500 dark:text-zinc-400">
            {page} / {totalPages}
          </span>
          <button
            type="button"
            disabled={page >= totalPages}
            onClick={() => onChangePage(page + 1)}
            className="flex min-h-[40px] items-center gap-1 rounded-xl border border-zinc-200 bg-white px-3.5 text-xs font-medium text-zinc-700 disabled:opacity-40 dark:border-zinc-800 dark:bg-[#232328] dark:text-zinc-200"
          >
            {tr(lang, "nextPage")}
            <ChevronRight className="h-4 w-4" />
          </button>
        </div>
      )}

      {/* Log Detail Bottom Sheet */}
      {selected && (
        <div
          className="fixed inset-0 z-50 flex items-end justify-center bg-black/50 backdrop-blur-[2px]"
          onClick={() => setSelected(null)}
          role="dialog"
          aria-modal="true"
          aria-label={tr(lang, "logDetail")}
        >
          <div
            className="max-h-[85dvh] w-full max-w-lg overflow-y-auto rounded-t-3xl border-t border-zinc-200 bg-white px-4 pb-[calc(1.25rem+env(safe-area-inset-bottom))] pt-3 shadow-2xl dark:border-zinc-800 dark:bg-[#232328]"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="mx-auto mb-3 h-1 w-10 rounded-full bg-zinc-300 dark:bg-zinc-700" />
            <div className="mb-3 flex items-center justify-between">
              <div className="min-w-0">
                <h3 className="truncate text-sm font-semibold text-zinc-900 dark:text-zinc-100">
                  {selected.model}
                </h3>
                <p className="text-xs text-zinc-500 dark:text-zinc-400">
                  {formatAppName(selected.appType)} ·{" "}
                  {selected.providerName ?? selected.providerId}
                </p>
              </div>
              <button
                type="button"
                onClick={() => setSelected(null)}
                aria-label={tr(lang, "close")}
                className="flex h-9 w-9 items-center justify-center rounded-full text-zinc-500 hover:bg-zinc-100 dark:hover:bg-zinc-800"
              >
                <X className="h-4 w-4" />
              </button>
            </div>

            <div className="grid grid-cols-2 gap-2.5 text-xs">
              <DetailTile
                label={tr(lang, "inputTokens")}
                value={selected.inputTokens.toLocaleString()}
              />
              <DetailTile
                label={tr(lang, "outputTokens")}
                value={selected.outputTokens.toLocaleString()}
              />
              <DetailTile
                label={tr(lang, "cacheRead")}
                value={selected.cacheReadTokens.toLocaleString()}
              />
              <DetailTile
                label={tr(lang, "cacheWrite")}
                value={selected.cacheCreationTokens.toLocaleString()}
              />
              <DetailTile
                label={tr(lang, "ttft")}
                value={formatLatency(selected.firstTokenMs)}
              />
              <DetailTile
                label={tr(lang, "totalLatency")}
                value={formatLatency(selected.latencyMs)}
              />
              <DetailTile
                label={tr(lang, "speedTps")}
                value={formatTps(selected.tps, selected.tpsEstimated)}
              />
              <DetailTile
                label={tr(lang, "cost")}
                value={formatUsd(selected.totalCostUsd)}
              />
            </div>

            {selected.errorMessage && (
              <div className="mt-3 rounded-xl bg-red-500/10 p-3 text-xs text-red-600 dark:text-red-400">
                <div className="mb-1 font-semibold">
                  {tr(lang, "errorMessage")}
                </div>
                <pre className="whitespace-pre-wrap break-words font-mono text-[11px]">
                  {selected.errorMessage}
                </pre>
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
}

function DetailTile({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-xl bg-zinc-50 p-2.5 dark:bg-zinc-900/70">
      <div className="text-[11px] text-zinc-500 dark:text-zinc-400">
        {label}
      </div>
      <div className="mt-0.5 text-sm font-semibold tabular-nums text-zinc-900 dark:text-zinc-100">
        {value}
      </div>
    </div>
  );
}
