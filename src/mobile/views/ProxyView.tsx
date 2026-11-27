import { AlertCircle, Loader2, Lock, Play, Square } from "lucide-react";
import type { ProxyStatusApiResponse } from "../api";
import { formatAppName, formatCompactNumber, formatUptime } from "../format";
import { tr, type MobileLang } from "../i18n";

interface ProxyViewProps {
  lang: MobileLang;
  data: ProxyStatusApiResponse | null;
  loading: boolean;
  mutating: boolean;
  error: string | null;
  onRetry: () => void;
  onStart: () => void;
  onStop: () => void;
}

export function ProxyView({
  lang,
  data,
  loading,
  mutating,
  error,
  onRetry,
  onStart,
  onStop,
}: ProxyViewProps) {
  const status = data?.status;
  const takeoverEntries: Array<[string, boolean]> = data
    ? [
        ["claude", data.takeover.claude],
        ["codex", data.takeover.codex],
        ["gemini", data.takeover.gemini],
        ["grokbuild", data.takeover.grokbuild],
        ["claude-desktop", data.takeover["claude-desktop"]],
      ]
    : [];

  return (
    <div className="space-y-4">
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
        <div className="h-48 animate-pulse rounded-2xl bg-zinc-200/60 dark:bg-zinc-800/60" />
      ) : status ? (
        <>
          <section className="rounded-2xl border border-zinc-200/80 bg-white p-4 dark:border-zinc-800 dark:bg-[#232328]">
            <div className="flex items-center justify-between gap-3">
              <div>
                <div className="flex items-center gap-2">
                  <span
                    className={`h-2.5 w-2.5 rounded-full ${
                      status.running ? "bg-emerald-500" : "bg-zinc-400"
                    }`}
                  />
                  <span className="text-base font-semibold text-zinc-900 dark:text-zinc-100">
                    {status.running
                      ? tr(lang, "proxyRunning")
                      : tr(lang, "proxyStopped")}
                  </span>
                </div>
                {status.address && (
                  <p className="mt-1 font-mono text-xs text-zinc-500 dark:text-zinc-400">
                    {status.address}:{status.port}
                  </p>
                )}
              </div>

              {data.allowControl ? (
                <button
                  type="button"
                  disabled={mutating}
                  onClick={status.running ? onStop : onStart}
                  className={`flex min-h-[44px] items-center gap-2 rounded-xl px-4 text-xs font-semibold text-white transition active:scale-95 disabled:opacity-50 ${
                    status.running ? "bg-red-600" : "bg-[#f97316]"
                  }`}
                >
                  {mutating ? (
                    <Loader2 className="h-4 w-4 animate-spin" />
                  ) : status.running ? (
                    <Square className="h-3.5 w-3.5 fill-current" />
                  ) : (
                    <Play className="h-3.5 w-3.5 fill-current" />
                  )}
                  {status.running
                    ? tr(lang, "stopProxy")
                    : tr(lang, "startProxy")}
                </button>
              ) : (
                <span className="inline-flex items-center gap-1 rounded-full bg-zinc-100 px-2.5 py-1 text-[11px] font-medium text-zinc-500 dark:bg-zinc-800 dark:text-zinc-400">
                  <Lock className="h-3 w-3" />
                  {tr(lang, "readOnlyBadge")}
                </span>
              )}
            </div>

            <div className="mt-4 grid grid-cols-3 gap-2">
              <StatMini
                label={tr(lang, "uptime")}
                value={
                  status.running ? formatUptime(status.uptimeSeconds) : "—"
                }
              />
              <StatMini
                label={tr(lang, "kpiRequests")}
                value={formatCompactNumber(status.totalRequests)}
              />
              <StatMini
                label={tr(lang, "failovers")}
                value={String(status.failoverCount)}
              />
            </div>
          </section>

          {/* Routed Apps */}
          <section className="rounded-2xl border border-zinc-200/80 bg-white p-4 dark:border-zinc-800 dark:bg-[#232328]">
            <h2 className="mb-3 text-xs font-semibold uppercase tracking-wider text-zinc-500 dark:text-zinc-400">
              {tr(lang, "takeoverApps")}
            </h2>
            <div className="flex flex-wrap gap-2">
              {takeoverEntries.map(([app, active]) => (
                <span
                  key={app}
                  className={`inline-flex items-center gap-1.5 rounded-full px-3 py-1.5 text-xs font-medium ${
                    active
                      ? "bg-teal-500/15 text-teal-700 dark:text-teal-300"
                      : "bg-zinc-100 text-zinc-500 dark:bg-zinc-800 dark:text-zinc-400"
                  }`}
                >
                  <span
                    className={`h-1.5 w-1.5 rounded-full ${
                      active ? "bg-teal-500" : "bg-zinc-400"
                    }`}
                  />
                  {formatAppName(app)}
                </span>
              ))}
            </div>
          </section>

          {/* Active Targets */}
          <section className="rounded-2xl border border-zinc-200/80 bg-white p-4 dark:border-zinc-800 dark:bg-[#232328]">
            <h2 className="mb-3 text-xs font-semibold uppercase tracking-wider text-zinc-500 dark:text-zinc-400">
              {tr(lang, "activeTargets")}
            </h2>
            {status.activeTargets.length === 0 ? (
              <p className="text-xs text-zinc-400">
                {tr(lang, "noActiveTargets")}
              </p>
            ) : (
              <div className="divide-y divide-zinc-100 dark:divide-zinc-800">
                {status.activeTargets.map((target) => (
                  <div
                    key={`${target.appType}-${target.providerId}`}
                    className="flex items-center justify-between py-2.5 first:pt-0 last:pb-0"
                  >
                    <span className="text-xs text-zinc-500 dark:text-zinc-400">
                      {formatAppName(target.appType.toLowerCase())}
                    </span>
                    <span className="text-sm font-semibold text-zinc-900 dark:text-zinc-100">
                      {target.providerName}
                    </span>
                  </div>
                ))}
              </div>
            )}
          </section>
        </>
      ) : null}
    </div>
  );
}

function StatMini({ label, value }: { label: string; value: string }) {
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
