import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { keepPreviousData, useQueryClient } from "@tanstack/react-query";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  Activity,
  BarChart3,
  CheckCircle2,
  Database,
  Palette,
  PanelsTopLeft,
  RefreshCw,
  Server,
  X,
} from "lucide-react";
import { APP_ICON_MAP } from "@/config/appConfig";
import { Button } from "@/components/ui/button";
import { useUsageEventBridge } from "@/hooks/useUsageEventBridge";
import { usageKeys, useTrayUsageOverview } from "@/lib/query/usage";
import { cn } from "@/lib/utils";
import type {
  DailyStats,
  UsageRangeSelection,
  UsageSummary,
} from "@/types/usage";
import {
  fmtUsd,
  formatTokensShort,
  getLocaleFromLanguage,
  getResolvedLang,
  parseFiniteNumber,
} from "./format";

type TrayRangePreset = "today" | "7d" | "30d";
type TrayVisualTheme = "native" | "card";

const TRAY_THEME_STORAGE_KEY = "cc-switch:tray-usage-theme";

const RANGE_OPTIONS: Array<{
  value: TrayRangePreset;
  labelKey: string;
  fallback: string;
}> = [
  { value: "today", labelKey: "usage.trayPanel.day", fallback: "Day" },
  { value: "7d", labelKey: "usage.trayPanel.week", fallback: "Week" },
  { value: "30d", labelKey: "usage.trayPanel.month", fallback: "Month" },
];

const TREND_LIMIT_BY_RANGE: Record<TrayRangePreset, number> = {
  today: 7,
  "7d": 7,
  "30d": 14,
};

const TRAY_THEME_OPTIONS: Array<{
  value: TrayVisualTheme;
  labelKey: string;
  fallback: string;
  icon: ReactNode;
}> = [
  {
    value: "native",
    labelKey: "usage.trayPanel.themeNative",
    fallback: "Native",
    icon: <PanelsTopLeft className="h-3.5 w-3.5" />,
  },
  {
    value: "card",
    labelKey: "usage.trayPanel.themeCard",
    fallback: "Card",
    icon: <Palette className="h-3.5 w-3.5" />,
  },
];

const emptySummary: UsageSummary = {
  totalRequests: 0,
  totalCost: "0",
  totalInputTokens: 0,
  totalOutputTokens: 0,
  totalCacheCreationTokens: 0,
  totalCacheReadTokens: 0,
  successRate: 0,
  realTotalTokens: 0,
  cacheHitRate: 0,
};

function aggregateSummaries(items: UsageSummary[]): UsageSummary {
  if (items.length === 0) return emptySummary;

  let totalRequests = 0;
  let successCount = 0;
  let totalCost = 0;
  let input = 0;
  let output = 0;
  let cacheCreation = 0;
  let cacheRead = 0;

  for (const item of items) {
    totalRequests += item.totalRequests;
    successCount += Math.round((item.totalRequests * item.successRate) / 100);
    totalCost += parseFiniteNumber(item.totalCost) ?? 0;
    input += item.totalInputTokens;
    output += item.totalOutputTokens;
    cacheCreation += item.totalCacheCreationTokens;
    cacheRead += item.totalCacheReadTokens;
  }

  const cacheableInput = input + cacheCreation + cacheRead;
  return {
    totalRequests,
    totalCost: totalCost.toFixed(6),
    totalInputTokens: input,
    totalOutputTokens: output,
    totalCacheCreationTokens: cacheCreation,
    totalCacheReadTokens: cacheRead,
    successRate: totalRequests > 0 ? (successCount / totalRequests) * 100 : 0,
    realTotalTokens: input + output + cacheCreation + cacheRead,
    cacheHitRate: cacheableInput > 0 ? cacheRead / cacheableInput : 0,
  };
}

function rangeSelection(preset: TrayRangePreset): UsageRangeSelection {
  return { preset };
}

function formatUsdAuto(value: unknown) {
  const cost = parseFiniteNumber(value);
  if (cost == null) return "--";
  return fmtUsd(cost, Math.abs(cost) >= 1 ? 2 : 4);
}

function trendTotalTokens(day: DailyStats): number {
  return (
    day.totalInputTokens +
    day.totalOutputTokens +
    day.totalCacheCreationTokens +
    day.totalCacheReadTokens
  );
}

function trendInputLikeTokens(day: DailyStats): number {
  return (
    day.totalInputTokens +
    day.totalCacheCreationTokens +
    day.totalCacheReadTokens
  );
}

function isKnownAppId(appType: string): appType is keyof typeof APP_ICON_MAP {
  return appType in APP_ICON_MAP;
}

function formatPercent(value: number, scale: "unit" | "percent" = "percent") {
  const pct = scale === "unit" ? value * 100 : value;
  if (!Number.isFinite(pct)) return "--";
  return `${Math.round(pct)}%`;
}

function parseTrendDate(date: string) {
  const raw = date.trim();
  const candidates = [
    raw,
    raw.includes("T") ? raw : `${raw}T00:00:00`,
    raw.replace(" ", "T"),
  ];

  for (const candidate of candidates) {
    const parsed = new Date(candidate);
    if (!Number.isNaN(parsed.getTime())) return parsed;
  }

  return null;
}

function trendLabel(date: string, lang: string, preset: TrayRangePreset) {
  const parsed = parseTrendDate(date);
  if (!parsed) return date;

  if (preset === "today" && date.includes(":")) {
    return `${String(parsed.getHours()).padStart(2, "0")}:00`;
  }

  return new Intl.DateTimeFormat(getLocaleFromLanguage(lang), {
    month: "numeric",
    day: "numeric",
  }).format(parsed);
}

function isTrayVisualTheme(value: string | null): value is TrayVisualTheme {
  return value === "native" || value === "card";
}

function getInitialTrayTheme(): TrayVisualTheme {
  if (typeof window === "undefined") return "native";

  try {
    const stored = window.localStorage.getItem(TRAY_THEME_STORAGE_KEY);
    return isTrayVisualTheme(stored) ? stored : "native";
  } catch {
    return "native";
  }
}

export function TrayUsagePanel() {
  const { t, i18n } = useTranslation();
  const queryClient = useQueryClient();
  const lang = getResolvedLang(i18n);
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const commitRangeTimerRef = useRef<ReturnType<typeof setTimeout> | null>(
    null,
  );
  const [selectedRangePreset, setSelectedRangePreset] =
    useState<TrayRangePreset>("today");
  const [rangePreset, setRangePreset] = useState<TrayRangePreset>("today");
  const [visualTheme, setVisualTheme] =
    useState<TrayVisualTheme>(getInitialTrayTheme);
  const range = useMemo(() => rangeSelection(rangePreset), [rangePreset]);
  const isCardTheme = visualTheme === "card";

  useUsageEventBridge();

  const resetScroll = () => {
    scrollRef.current?.scrollTo({ top: 0 });
  };

  useEffect(() => {
    resetScroll();
  }, [selectedRangePreset]);

  useEffect(() => {
    try {
      window.localStorage.setItem(TRAY_THEME_STORAGE_KEY, visualTheme);
    } catch {
      // localStorage may be unavailable in restricted webviews.
    }
  }, [visualTheme]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void getCurrentWindow()
      .onFocusChanged(({ payload: focused }) => {
        if (focused) requestAnimationFrame(resetScroll);
      })
      .then((dispose) => {
        unlisten = dispose;
      });

    return () => {
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    return () => {
      if (commitRangeTimerRef.current) {
        clearTimeout(commitRangeTimerRef.current);
      }
    };
  }, []);

  const queryOptions = {
    placeholderData: keepPreviousData,
    refetchInterval: 30000,
    refetchIntervalInBackground: true,
  };
  const { data: overview, isLoading } = useTrayUsageOverview(
    range,
    {},
    queryOptions,
  );

  const apps = overview?.summaryByApp ?? [];
  const providers = overview?.providers;
  const models = overview?.models;
  const trends = overview?.trends;
  const summary = useMemo(
    () => aggregateSummaries(apps.map((app) => app.summary)),
    [apps],
  );
  const totalCost = parseFiniteNumber(summary.totalCost);
  const totalBreakdown =
    summary.totalInputTokens +
    summary.totalOutputTokens +
    summary.totalCacheCreationTokens +
    summary.totalCacheReadTokens;
  const hasUsage = summary.totalRequests > 0 || summary.realTotalTokens > 0;
  const topApps = apps
    .slice()
    .sort((a, b) => b.summary.realTotalTokens - a.summary.realTotalTokens)
    .slice(0, 4);
  const topModels = (models ?? [])
    .slice()
    .sort((a, b) => b.totalTokens - a.totalTokens)
    .slice(0, 5);

  const tokenSegments = [
    {
      label: t("usage.freshInput", "Fresh Input"),
      value: summary.totalInputTokens,
      className: "bg-sky-500",
    },
    {
      label: t("usage.outputTokens", "Output"),
      value: summary.totalOutputTokens,
      className: "bg-violet-500",
    },
    {
      label: t("usage.cacheCreationTokens", "Cache Creation"),
      value: summary.totalCacheCreationTokens,
      className: "bg-amber-500",
    },
    {
      label: t("usage.cacheReadTokens", "Cache Hit"),
      value: summary.totalCacheReadTokens,
      className: "bg-emerald-500",
    },
  ];

  const closePanel = () => {
    void getCurrentWindow().hide();
  };

  const selectRangePreset = (preset: TrayRangePreset) => {
    if (preset !== selectedRangePreset) {
      if (commitRangeTimerRef.current) {
        clearTimeout(commitRangeTimerRef.current);
      }
      setSelectedRangePreset(preset);
      commitRangeTimerRef.current = setTimeout(() => {
        setRangePreset(preset);
        commitRangeTimerRef.current = null;
      }, 0);
    } else if (preset !== rangePreset) {
      setRangePreset(preset);
    }
  };

  const refresh = () => {
    void queryClient.invalidateQueries({ queryKey: usageKeys.all });
  };

  return (
    <div className="h-screen overflow-hidden bg-transparent p-2 text-foreground">
      <section
        className={cn(
          "flex h-full flex-col overflow-hidden shadow-2xl",
          isCardTheme
            ? "rounded-[16px] border-2 border-slate-800 bg-[#fff9ed] dark:border-slate-200 dark:bg-slate-950"
            : "rounded-[14px] border border-border/70 bg-background",
        )}
        style={
          isCardTheme
            ? {
                backgroundImage:
                  "radial-gradient(circle at 10px 10px, rgba(15, 23, 42, 0.08) 1px, transparent 1.5px)",
                backgroundSize: "16px 16px",
              }
            : undefined
        }
      >
        <header
          data-tauri-drag-region
          className={cn(
            "flex shrink-0 items-center justify-between px-3.5 py-3",
            isCardTheme
              ? "border-b-2 border-slate-800 bg-[#fffaf2]/90 dark:border-slate-200 dark:bg-slate-950/90"
              : "border-b border-border/70",
          )}
        >
          <div className="flex min-w-0 items-center gap-2">
            <span className="flex h-7 w-7 shrink-0 items-center justify-center rounded-md border border-emerald-500/25 bg-emerald-500/10 text-emerald-600 dark:text-emerald-400">
              <BarChart3 className="h-4 w-4" />
            </span>
            <div className="min-w-0">
              <div className="truncate text-sm font-semibold">CC Switch</div>
              <div className="truncate text-[10px] text-muted-foreground">
                {t("usage.trayPanel.title", "Usage")}
              </div>
            </div>
          </div>

          <div className="flex items-center gap-1.5" data-tauri-no-drag>
            <div
              className={cn(
                "flex h-7 rounded-md p-0.5",
                isCardTheme
                  ? "border-2 border-slate-800 bg-white shadow-[2px_2px_0_#cbd5e1] dark:border-slate-200 dark:bg-slate-900"
                  : "border border-border/70 bg-muted/40",
              )}
            >
              {RANGE_OPTIONS.map((option) => (
                <button
                  key={option.value}
                  type="button"
                  aria-pressed={selectedRangePreset === option.value}
                  onPointerDown={(event) => {
                    if (event.pointerType === "mouse" && event.button !== 0) {
                      return;
                    }
                    selectRangePreset(option.value);
                  }}
                  onClick={() => selectRangePreset(option.value)}
                  className={cn(
                    "min-w-9 rounded px-2 text-[11px] font-medium transition-colors",
                    selectedRangePreset === option.value
                      ? isCardTheme
                        ? "bg-amber-200 text-slate-950"
                        : "bg-background text-foreground shadow-sm"
                      : isCardTheme
                        ? "text-slate-500 hover:bg-amber-100 hover:text-slate-950 dark:text-slate-400 dark:hover:bg-slate-800 dark:hover:text-slate-100"
                        : "text-muted-foreground hover:text-foreground",
                  )}
                >
                  {t(option.labelKey, option.fallback)}
                </button>
              ))}
            </div>
            <ThemeSelector
              value={visualTheme}
              onChange={setVisualTheme}
              t={t}
              isCardTheme={isCardTheme}
            />
            <Button
              type="button"
              variant="ghost"
              size="icon"
              className="h-7 w-7 rounded-md"
              title={t("common.refresh")}
              aria-label={t("common.refresh")}
              onClick={refresh}
            >
              <RefreshCw className="h-3.5 w-3.5" />
            </Button>
            <Button
              type="button"
              variant="ghost"
              size="icon"
              className="h-7 w-7 rounded-md"
              title={t("common.close")}
              aria-label={t("common.close")}
              onClick={closePanel}
            >
              <X className="h-3.5 w-3.5" />
            </Button>
          </div>
        </header>

        <div
          ref={scrollRef}
          className="min-h-0 flex-1 overflow-y-auto px-4 py-3.5"
        >
          <div className="space-y-3.5">
            <section>
              <div className="flex items-end justify-between gap-3">
                <div className="min-w-0">
                  <div className="text-[10px] font-medium uppercase text-muted-foreground">
                    {t("usage.realTotal", "Tokens Processed")}
                  </div>
                  <div
                    className="mt-1 truncate text-[32px] font-semibold tabular-nums leading-none"
                    title={summary.realTotalTokens.toLocaleString()}
                  >
                    {isLoading
                      ? "--"
                      : formatTokensShort(summary.realTotalTokens, lang, 2)}
                  </div>
                </div>
                <div className="shrink-0 text-right">
                  <div className="text-[10px] font-medium text-muted-foreground">
                    {t("usage.cost", "Cost")}
                  </div>
                  <div className="mt-1 text-lg font-semibold tabular-nums text-emerald-600 dark:text-emerald-400">
                    {isLoading ? "--" : formatUsdAuto(totalCost)}
                  </div>
                </div>
              </div>

              <div className="mt-3">
                <TokenSplitBar
                  segments={tokenSegments}
                  total={totalBreakdown}
                  theme={visualTheme}
                />
                <div className="mt-2 grid grid-cols-2 gap-x-4 gap-y-1.5">
                  {tokenSegments.map((segment) => (
                    <TokenLegend
                      key={segment.label}
                      label={segment.label}
                      value={formatTokensShort(segment.value, lang)}
                      className={segment.className}
                      theme={visualTheme}
                    />
                  ))}
                </div>
              </div>
            </section>

            <div className="grid grid-cols-2 gap-2">
              <MetricTile
                icon={<Activity className="h-3.5 w-3.5" />}
                label={t("usage.totalRequests")}
                value={
                  isLoading ? "--" : summary.totalRequests.toLocaleString()
                }
                accent="text-sky-600 dark:text-sky-400"
                theme={visualTheme}
              />
              <MetricTile
                icon={<Database className="h-3.5 w-3.5" />}
                label={t("usage.cacheHitRate")}
                value={
                  isLoading ? "--" : formatPercent(summary.cacheHitRate, "unit")
                }
                accent="text-emerald-600 dark:text-emerald-400"
                theme={visualTheme}
              />
              <MetricTile
                icon={<Server className="h-3.5 w-3.5" />}
                label={t("usage.trayPanel.sources", "Sources")}
                value={isLoading ? "--" : String(providers?.length ?? 0)}
                accent="text-amber-600 dark:text-amber-400"
                theme={visualTheme}
              />
              <MetricTile
                icon={<CheckCircle2 className="h-3.5 w-3.5" />}
                label={t("usage.trayPanel.success", "Success")}
                value={isLoading ? "--" : formatPercent(summary.successRate)}
                accent="text-violet-600 dark:text-violet-400"
                theme={visualTheme}
              />
            </div>

            <PanelSection title={t("usage.trends")} theme={visualTheme}>
              <TrendBars
                items={(trends ?? []).slice(-TREND_LIMIT_BY_RANGE[rangePreset])}
                emptyLabel={t("usage.noData")}
                lang={lang}
                rangePreset={rangePreset}
                theme={visualTheme}
              />
            </PanelSection>

            <PanelSection
              title={t("usage.trayPanel.apps", "Apps")}
              meta={hasUsage ? String(apps.length) : undefined}
              theme={visualTheme}
            >
              {topApps.length === 0 ? (
                <EmptyRow theme={visualTheme}>{t("usage.noData")}</EmptyRow>
              ) : (
                <div className="space-y-2">
                  {topApps.map((app) => {
                    const appConfig = isKnownAppId(app.appType)
                      ? APP_ICON_MAP[app.appType]
                      : null;
                    return (
                      <RankRow
                        key={app.appType}
                        icon={appConfig?.icon}
                        label={
                          appConfig?.label ??
                          t(`usage.appFilter.${app.appType}`, app.appType)
                        }
                        value={formatTokensShort(
                          app.summary.realTotalTokens,
                          lang,
                        )}
                        barValue={app.summary.realTotalTokens}
                        maxValue={summary.realTotalTokens}
                        accent="bg-sky-500"
                        theme={visualTheme}
                      />
                    );
                  })}
                </div>
              )}
            </PanelSection>

            <PanelSection
              title={t("usage.trayPanel.models", "Models")}
              meta={
                topModels.length > 0 ? String(models?.length ?? 0) : undefined
              }
              theme={visualTheme}
            >
              {topModels.length === 0 ? (
                <EmptyRow theme={visualTheme}>{t("usage.noData")}</EmptyRow>
              ) : (
                <div className="space-y-2">
                  {topModels.map((model) => (
                    <RankRow
                      key={model.model}
                      label={model.model}
                      value={formatTokensShort(model.totalTokens, lang)}
                      secondary={formatUsdAuto(model.totalCost)}
                      barValue={model.totalTokens}
                      maxValue={topModels[0]?.totalTokens ?? 0}
                      accent="bg-violet-500"
                      theme={visualTheme}
                    />
                  ))}
                </div>
              )}
            </PanelSection>
          </div>
        </div>
      </section>
    </div>
  );
}

function ThemeSelector({
  value,
  onChange,
  t,
  isCardTheme,
}: {
  value: TrayVisualTheme;
  onChange: (theme: TrayVisualTheme) => void;
  t: (key: string, fallback: string) => string;
  isCardTheme: boolean;
}) {
  return (
    <div
      className={cn(
        "flex h-7 rounded-md p-0.5",
        isCardTheme
          ? "border-2 border-slate-800 bg-white shadow-[2px_2px_0_#cbd5e1] dark:border-slate-200 dark:bg-slate-900"
          : "border border-border/70 bg-muted/40",
      )}
    >
      {TRAY_THEME_OPTIONS.map((option) => {
        const label = t(option.labelKey, option.fallback);
        return (
          <button
            key={option.value}
            type="button"
            title={label}
            aria-label={label}
            aria-pressed={value === option.value}
            onClick={() => onChange(option.value)}
            className={cn(
              "grid h-6 w-7 place-items-center rounded transition-colors",
              value === option.value
                ? isCardTheme
                  ? "bg-amber-200 text-slate-950"
                  : "bg-background text-foreground shadow-sm"
                : isCardTheme
                  ? "text-slate-500 hover:bg-amber-100 hover:text-slate-950 dark:text-slate-400 dark:hover:bg-slate-800 dark:hover:text-slate-100"
                  : "text-muted-foreground hover:text-foreground",
            )}
          >
            {option.icon}
          </button>
        );
      })}
    </div>
  );
}

function TokenSplitBar({
  segments,
  total,
  theme,
}: {
  segments: Array<{ label: string; value: number; className: string }>;
  total: number;
  theme: TrayVisualTheme;
}) {
  const isCardTheme = theme === "card";

  return (
    <div
      className={cn(
        "flex overflow-hidden rounded bg-muted/70",
        isCardTheme
          ? "h-3 border border-slate-800 dark:border-slate-200"
          : "h-2",
      )}
    >
      {segments.map((segment) => {
        if (segment.value <= 0 || total <= 0) return null;
        return (
          <div
            key={segment.label}
            className={segment.className}
            style={{
              flexGrow: segment.value,
              flexBasis: 0,
              minWidth: segment.value / total < 0.015 ? 3 : undefined,
            }}
            title={`${segment.label}: ${segment.value.toLocaleString()}`}
          />
        );
      })}
    </div>
  );
}

function TokenLegend({
  label,
  value,
  className,
  theme,
}: {
  label: string;
  value: string;
  className: string;
  theme: TrayVisualTheme;
}) {
  const isCardTheme = theme === "card";

  return (
    <div
      className={cn(
        "flex min-w-0 items-center gap-1.5 text-[11px]",
        isCardTheme
          ? "font-medium text-slate-600 dark:text-slate-300"
          : "text-muted-foreground",
      )}
    >
      <span className={cn("h-2 w-2 shrink-0 rounded-[2px]", className)} />
      <span className="min-w-0 flex-1 truncate">{label}</span>
      <span className="shrink-0 tabular-nums">{value}</span>
    </div>
  );
}

function MetricTile({
  icon,
  label,
  value,
  accent,
  theme,
}: {
  icon: ReactNode;
  label: string;
  value: string;
  accent: string;
  theme: TrayVisualTheme;
}) {
  const isCardTheme = theme === "card";

  return (
    <div
      className={cn(
        "min-w-0 rounded-md px-2.5 py-2",
        isCardTheme
          ? "border border-slate-800 bg-white shadow-[2px_2px_0_#e2e8f0] dark:border-slate-200 dark:bg-slate-900 dark:shadow-none"
          : "bg-muted/40",
      )}
    >
      <div className={cn("flex items-center gap-1.5", accent)}>
        {icon}
        <span className="min-w-0 truncate text-[11px] text-muted-foreground">
          {label}
        </span>
      </div>
      <div className="mt-1 truncate text-base font-semibold tabular-nums">
        {value}
      </div>
    </div>
  );
}

function PanelSection({
  title,
  meta,
  children,
  theme,
}: {
  title: string;
  meta?: string;
  children: ReactNode;
  theme: TrayVisualTheme;
}) {
  const isCardTheme = theme === "card";

  return (
    <section
      className={cn(
        "pt-3",
        isCardTheme
          ? "border-t-2 border-slate-800 dark:border-slate-200"
          : "border-t border-border/70",
      )}
    >
      <div className="mb-2 flex items-baseline justify-between gap-2">
        <SectionTitle>{title}</SectionTitle>
        {meta && (
          <span className="shrink-0 text-[10px] tabular-nums text-muted-foreground">
            {meta}
          </span>
        )}
      </div>
      {children}
    </section>
  );
}

function SectionTitle({ children }: { children: ReactNode }) {
  return (
    <div className="text-[11px] font-semibold uppercase text-muted-foreground">
      {children}
    </div>
  );
}

function TrendBars({
  items,
  emptyLabel,
  lang,
  rangePreset,
  theme,
}: {
  items: DailyStats[];
  emptyLabel: string;
  lang: string;
  rangePreset: TrayRangePreset;
  theme: TrayVisualTheme;
}) {
  if (items.length === 0) {
    return <EmptyRow theme={theme}>{emptyLabel}</EmptyRow>;
  }

  const max = Math.max(1, ...items.map(trendTotalTokens));
  const compact = items.length <= 3;
  const isCardTheme = theme === "card";

  return (
    <div>
      <div
        className={cn(
          "relative flex h-[86px] items-end gap-1.5 pb-1",
          isCardTheme
            ? "rounded-md border border-slate-800 bg-white/80 px-2 dark:border-slate-200 dark:bg-slate-900"
            : "border-b border-border/70",
          compact && "justify-center",
        )}
      >
        <div
          className={cn(
            "pointer-events-none absolute inset-x-0 top-5 border-t",
            isCardTheme
              ? "border-slate-200 dark:border-slate-700"
              : "border-border/40",
          )}
        />
        <div
          className={cn(
            "pointer-events-none absolute inset-x-0 top-10 border-t",
            isCardTheme
              ? "border-slate-100 dark:border-slate-800"
              : "border-border/30",
          )}
        />
        {items.map((day) => {
          const total = trendTotalTokens(day);
          const inputLike = trendInputLikeTokens(day);
          const output = day.totalOutputTokens;
          const height = total > 0 ? Math.max(5, (total / max) * 68) : 0;
          const outputPct = total > 0 ? (output / total) * 100 : 0;
          const inputPct = total > 0 ? (inputLike / total) * 100 : 0;

          return (
            <div
              key={day.date}
              className={cn(
                "relative z-10 flex h-full min-w-0 flex-col justify-end",
                compact ? "w-8 flex-none" : "flex-1",
              )}
              title={`${day.date}: ${total.toLocaleString()}`}
            >
              <div
                className={cn(
                  "flex w-full flex-col overflow-hidden rounded-t-[3px] bg-muted/70",
                  isCardTheme &&
                    "border border-slate-800 dark:border-slate-200",
                )}
                style={{ height }}
              >
                {output > 0 && (
                  <div
                    className="bg-violet-400"
                    style={{
                      height: `${Math.max(8, outputPct)}%`,
                    }}
                  />
                )}
                {inputLike > 0 && (
                  <div
                    className="bg-emerald-500"
                    style={{
                      height: `${Math.max(8, inputPct)}%`,
                    }}
                  />
                )}
              </div>
            </div>
          );
        })}
      </div>
      <div
        className={cn(
          "mt-1.5 flex gap-1.5 text-[9px] tabular-nums text-muted-foreground",
          compact && "justify-center",
        )}
      >
        {items.map((day) => (
          <div
            key={day.date}
            className={cn(
              "min-w-0 truncate whitespace-nowrap text-center",
              compact ? "w-8" : "flex-1",
            )}
          >
            {trendLabel(day.date, lang, rangePreset)}
          </div>
        ))}
      </div>
    </div>
  );
}

function EmptyRow({
  children,
  theme,
}: {
  children: ReactNode;
  theme: TrayVisualTheme;
}) {
  const isCardTheme = theme === "card";

  return (
    <div
      className={cn(
        "rounded-md px-3 py-3 text-center text-xs text-muted-foreground",
        isCardTheme
          ? "border border-slate-800 bg-white/80 dark:border-slate-200 dark:bg-slate-900"
          : "bg-muted/35",
      )}
    >
      {children}
    </div>
  );
}

function RankRow({
  icon,
  label,
  value,
  secondary,
  barValue,
  maxValue,
  accent,
  theme,
}: {
  icon?: ReactNode;
  label: string;
  value: string;
  secondary?: string;
  barValue: number;
  maxValue: number;
  accent: string;
  theme: TrayVisualTheme;
}) {
  const width = maxValue > 0 ? Math.max(4, (barValue / maxValue) * 100) : 0;
  const isCardTheme = theme === "card";

  return (
    <div
      className={cn(
        "min-w-0",
        isCardTheme &&
          "rounded-md border border-slate-800 bg-white px-2.5 py-2 shadow-[2px_2px_0_#e2e8f0] dark:border-slate-200 dark:bg-slate-900 dark:shadow-none",
      )}
    >
      <div className="mb-1 flex min-w-0 items-center gap-2">
        {icon && (
          <span
            className={cn(
              "flex h-5 w-5 shrink-0 items-center justify-center rounded-md bg-muted/60",
              isCardTheme && "border border-slate-800 dark:border-slate-200",
            )}
          >
            {icon}
          </span>
        )}
        <span
          className="min-w-0 flex-1 truncate text-xs font-medium"
          title={label}
        >
          {label}
        </span>
        {secondary && (
          <span className="shrink-0 text-[11px] tabular-nums text-muted-foreground">
            {secondary}
          </span>
        )}
        <span className="shrink-0 text-xs font-semibold tabular-nums">
          {value}
        </span>
      </div>
      <div
        className={cn(
          "h-1.5 overflow-hidden rounded bg-muted/60",
          isCardTheme && "border border-slate-800 dark:border-slate-200",
        )}
      >
        <div
          className={cn("h-full rounded", accent)}
          style={{ width: `${width}%` }}
        />
      </div>
    </div>
  );
}
