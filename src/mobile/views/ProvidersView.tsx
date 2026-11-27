import { useMemo, useState } from "react";
import {
  AlertCircle,
  Check,
  CheckCircle2,
  Loader2,
  Lock,
  ShieldAlert,
} from "lucide-react";
import type { ProvidersListResponse } from "../api";
import { formatAppName } from "../format";
import { tr, type MobileLang } from "../i18n";

interface ProvidersViewProps {
  lang: MobileLang;
  data: ProvidersListResponse | null;
  loading: boolean;
  error: string | null;
  switchingKey: string | null;
  onRetry: () => void;
  onSwitch: (app: string, providerId: string) => void;
}

export function ProvidersView({
  lang,
  data,
  loading,
  error,
  switchingKey,
  onRetry,
  onSwitch,
}: ProvidersViewProps) {
  const [selectedApp, setSelectedApp] = useState<string>("all");

  const visibleApps = useMemo(() => {
    if (!data) return [];
    if (selectedApp === "all") return data.apps;
    return data.apps.filter((a) => a.app === selectedApp);
  }, [data, selectedApp]);

  return (
    <div className="space-y-4">
      {data && !data.allowControl && (
        <div className="flex items-start gap-2.5 rounded-2xl border border-amber-200/80 bg-amber-50/90 px-3.5 py-3 text-xs leading-relaxed text-amber-800 dark:border-amber-900/50 dark:bg-amber-950/30 dark:text-amber-300">
          <Lock className="mt-0.5 h-4 w-4 shrink-0" />
          <span>{tr(lang, "readOnlyNotice")}</span>
        </div>
      )}

      {/* App Filter Pills */}
      {data && data.apps.length > 1 && (
        <div className="-mx-4 flex gap-1.5 overflow-x-auto px-4 pb-1">
          <FilterPill
            active={selectedApp === "all"}
            label={tr(lang, "all")}
            onClick={() => setSelectedApp("all")}
          />
          {data.apps.map((a) => (
            <FilterPill
              key={a.app}
              active={selectedApp === a.app}
              label={formatAppName(a.app)}
              onClick={() => setSelectedApp(a.app)}
            />
          ))}
        </div>
      )}

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
        <div className="space-y-3">
          {Array.from({ length: 4 }).map((_, i) => (
            <div
              key={i}
              className="h-20 animate-pulse rounded-2xl bg-zinc-200/60 dark:bg-zinc-800/60"
            />
          ))}
        </div>
      ) : visibleApps.length === 0 ? (
        <div className="rounded-2xl border border-zinc-200/80 bg-white px-4 py-10 text-center text-sm text-zinc-500 dark:border-zinc-800 dark:bg-[#232328]">
          {tr(lang, "noProviders")}
        </div>
      ) : (
        visibleApps.map((group) => (
          <section key={group.app} className="space-y-2">
            <h2 className="px-1 text-xs font-semibold uppercase tracking-wider text-zinc-500 dark:text-zinc-400">
              {formatAppName(group.app)}
            </h2>
            <div className="space-y-2">
              {group.providers.map((p) => {
                const key = `${group.app}:${p.id}`;
                const isSwitching = switchingKey === key;
                const badgeColor =
                  p.iconColor && /^#[0-9a-fA-F]{3,8}$/.test(p.iconColor)
                    ? p.iconColor
                    : "#f97316";
                return (
                  <div
                    key={p.id}
                    className={`flex items-center justify-between gap-3 rounded-2xl border p-3.5 transition ${
                      p.isCurrent
                        ? "border-[#f97316]/60 bg-orange-500/[0.04] dark:bg-orange-500/[0.07]"
                        : "border-zinc-200/80 bg-white dark:border-zinc-800 dark:bg-[#232328]"
                    }`}
                  >
                    <div className="flex min-w-0 items-center gap-3">
                      <div
                        className="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl text-sm font-bold text-white"
                        style={{ backgroundColor: badgeColor }}
                      >
                        {(p.name.trim()[0] ?? "?").toUpperCase()}
                      </div>
                      <div className="min-w-0">
                        <div className="flex items-center gap-1.5">
                          <span className="truncate text-sm font-semibold text-zinc-900 dark:text-zinc-100">
                            {p.name}
                          </span>
                          {p.isCurrent && (
                            <span className="inline-flex shrink-0 items-center gap-0.5 rounded-full bg-[#f97316] px-2 py-0.5 text-[10px] font-semibold text-white">
                              <Check className="h-3 w-3" />
                              {tr(lang, "active")}
                            </span>
                          )}
                        </div>
                        <div className="mt-1 flex flex-wrap items-center gap-1.5 text-[11px] text-zinc-500 dark:text-zinc-400">
                          {p.category && (
                            <span className="rounded-md bg-zinc-100 px-1.5 py-0.5 dark:bg-zinc-800">
                              {p.category}
                            </span>
                          )}
                          {p.inFailoverQueue && (
                            <span className="rounded-md bg-sky-500/10 px-1.5 py-0.5 text-sky-600 dark:text-sky-400">
                              {tr(lang, "failoverQueue")}
                            </span>
                          )}
                          {p.health && (
                            <span
                              className={`inline-flex items-center gap-1 ${
                                p.health.isHealthy
                                  ? "text-emerald-600 dark:text-emerald-400"
                                  : "text-red-500"
                              }`}
                            >
                              {p.health.isHealthy ? (
                                <CheckCircle2 className="h-3 w-3" />
                              ) : (
                                <ShieldAlert className="h-3 w-3" />
                              )}
                              {p.health.isHealthy
                                ? tr(lang, "healthy")
                                : `${tr(lang, "degraded")} (${p.health.consecutiveFailures})`}
                            </span>
                          )}
                        </div>
                      </div>
                    </div>

                    {!p.isCurrent && data?.allowControl && (
                      <button
                        type="button"
                        disabled={switchingKey !== null}
                        onClick={() => onSwitch(group.app, p.id)}
                        className="flex min-h-[40px] shrink-0 items-center gap-1.5 rounded-xl bg-zinc-900 px-3.5 text-xs font-semibold text-white transition active:scale-95 disabled:opacity-50 dark:bg-zinc-100 dark:text-zinc-900"
                      >
                        {isSwitching && (
                          <Loader2 className="h-3.5 w-3.5 animate-spin" />
                        )}
                        {isSwitching
                          ? tr(lang, "switching")
                          : tr(lang, "switchBtn")}
                      </button>
                    )}
                  </div>
                );
              })}
            </div>
          </section>
        ))
      )}
    </div>
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
      className={`min-h-[36px] shrink-0 rounded-full px-3.5 text-xs font-medium transition ${
        active
          ? "bg-zinc-900 text-white dark:bg-zinc-100 dark:text-zinc-900"
          : "bg-white text-zinc-600 ring-1 ring-zinc-200 dark:bg-[#232328] dark:text-zinc-300 dark:ring-zinc-800"
      }`}
    >
      {label}
    </button>
  );
}
