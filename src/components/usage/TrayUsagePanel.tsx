import { useMemo, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useQueryClient } from "@tanstack/react-query";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { RefreshCw, X, Activity, Coins, Layers3 } from "lucide-react";
import { APP_ICON_MAP } from "@/config/appConfig";
import { Button } from "@/components/ui/button";
import { useUsageEventBridge } from "@/hooks/useUsageEventBridge";
import {
  usageKeys,
  useModelStats,
  useProviderStats,
  useUsageSummaryByApp,
  useUsageTrends,
} from "@/lib/query/usage";
import { cn } from "@/lib/utils";
import type {
  DailyStats,
  UsageRangeSelection,
  UsageSummary,
} from "@/types/usage";
import {
  fmtUsd,
  formatTokensShort,
  getResolvedLang,
  parseFiniteNumber,
} from "./format";

type TrayRangePreset = "today" | "7d" | "30d";

const RANGE_OPTIONS: Array<{
  value: TrayRangePreset;
  labelKey: string;
  fallback: string;
}> = [
  { value: "today", labelKey: "usage.trayPanel.day", fallback: "Day" },
  { value: "7d", labelKey: "usage.trayPanel.week", fallback: "Week" },
  { value: "30d", labelKey: "usage.trayPanel.month", fallback: "Month" },
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

function isKnownAppId(appType: string): appType is keyof typeof APP_ICON_MAP {
  return appType in APP_ICON_MAP;
}

export function TrayUsagePanel() {
  const { t, i18n } = useTranslation();
  const queryClient = useQueryClient();
  const lang = getResolvedLang(i18n);
  const [rangePreset, setRangePreset] = useState<TrayRangePreset>("today");
  const range = useMemo(() => rangeSelection(rangePreset), [rangePreset]);

  useUsageEventBridge();

  const queryOptions = {
    refetchInterval: 30000,
    refetchIntervalInBackground: true,
  };
  const { data: summaryByApp, isLoading } = useUsageSummaryByApp(
    range,
    {},
    queryOptions,
  );
  const { data: providers } = useProviderStats(range, {}, queryOptions);
  const { data: models } = useModelStats(range, {}, queryOptions);
  const { data: trends } = useUsageTrends(range, {}, queryOptions);

  const apps = summaryByApp ?? [];
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
  const trendMax = Math.max(1, ...(trends ?? []).map(trendTotalTokens));

  const tokenSegments = [
    {
      label: t("usage.freshInput", "Fresh Input"),
      value: summary.totalInputTokens,
      className: "bg-sky-500",
    },
    {
      label: t("usage.output", "Output"),
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

  const refresh = () => {
    void queryClient.invalidateQueries({ queryKey: usageKeys.all });
  };

  return (
    <div className="h-screen overflow-hidden bg-transparent p-2 text-foreground">
      <section className="flex h-full flex-col overflow-hidden rounded-lg border border-border/70 bg-background/95 shadow-2xl backdrop-blur-xl">
        <header
          data-tauri-drag-region
          className="flex items-center justify-between border-b border-border/70 px-3 py-2"
        >
          <div className="min-w-0">
            <div className="text-[11px] font-medium uppercase text-muted-foreground">
              CC Switch
            </div>
            <h1 className="truncate text-sm font-semibold">
              {t("usage.trayPanel.title", "Usage")}
            </h1>
          </div>

          <div className="flex items-center gap-1.5" data-tauri-no-drag>
            <div className="flex rounded-md border border-border/70 bg-muted/40 p-0.5">
              {RANGE_OPTIONS.map((option) => (
                <button
                  key={option.value}
                  type="button"
                  onClick={() => setRangePreset(option.value)}
                  className={cn(
                    "h-7 rounded px-2 text-[11px] font-medium transition-colors",
                    rangePreset === option.value
                      ? "bg-background text-foreground shadow-sm"
                      : "text-muted-foreground hover:text-foreground",
                  )}
                >
                  {t(option.labelKey, option.fallback)}
                </button>
              ))}
            </div>
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

        <div className="min-h-0 flex-1 overflow-y-auto px-3 py-3">
          <div className="space-y-3">
            <div className="rounded-lg border border-border/70 bg-card/80 p-3">
              <div className="flex items-start justify-between gap-3">
                <div className="min-w-0">
                  <div className="text-[11px] font-medium uppercase text-muted-foreground">
                    {t("usage.realTotal", "Tokens Processed")}
                  </div>
                  <div
                    className="mt-1 truncate text-3xl font-semibold tabular-nums leading-none"
                    title={summary.realTotalTokens.toLocaleString()}
                  >
                    {isLoading
                      ? "--"
                      : formatTokensShort(summary.realTotalTokens, lang, 2)}
                  </div>
                </div>
                <div className="shrink-0 rounded-md border border-emerald-500/20 bg-emerald-500/10 px-2 py-1 text-right">
                  <div className="text-[10px] font-medium uppercase text-emerald-600 dark:text-emerald-400">
                    {t("usage.totalCost")}
                  </div>
                  <div className="text-sm font-semibold tabular-nums text-emerald-700 dark:text-emerald-300">
                    {isLoading ? "--" : formatUsdAuto(totalCost)}
                  </div>
                </div>
              </div>

              <div className="mt-4 grid grid-cols-3 gap-2">
                <Metric
                  icon={<Activity className="h-3.5 w-3.5" />}
                  label={t("usage.totalRequests")}
                  value={
                    isLoading ? "--" : summary.totalRequests.toLocaleString()
                  }
                  accent="text-sky-500"
                />
                <Metric
                  icon={<Coins className="h-3.5 w-3.5" />}
                  label={t("usage.cacheHitRate")}
                  value={
                    isLoading
                      ? "--"
                      : `${Math.round(summary.cacheHitRate * 100)}%`
                  }
                  accent="text-emerald-500"
                />
                <Metric
                  icon={<Layers3 className="h-3.5 w-3.5" />}
                  label={t("usage.providerStats")}
                  value={isLoading ? "--" : String(providers?.length ?? 0)}
                  accent="text-amber-500"
                />
              </div>

              <div className="mt-4">
                <div className="flex h-2 overflow-hidden rounded bg-muted/70">
                  {tokenSegments.map((segment) => {
                    const width =
                      totalBreakdown > 0
                        ? Math.max(3, (segment.value / totalBreakdown) * 100)
                        : 0;
                    return (
                      <div
                        key={segment.label}
                        className={segment.className}
                        style={{ width: `${width}%` }}
                        title={`${segment.label}: ${segment.value.toLocaleString()}`}
                      />
                    );
                  })}
                </div>
                <div className="mt-2 grid grid-cols-2 gap-x-3 gap-y-1">
                  {tokenSegments.map((segment) => (
                    <div
                      key={segment.label}
                      className="flex min-w-0 items-center gap-1.5 text-[11px] text-muted-foreground"
                    >
                      <span
                        className={cn(
                          "h-2 w-2 shrink-0 rounded",
                          segment.className,
                        )}
                      />
                      <span className="truncate">{segment.label}</span>
                      <span className="ml-auto shrink-0 tabular-nums">
                        {formatTokensShort(segment.value, lang)}
                      </span>
                    </div>
                  ))}
                </div>
              </div>
            </div>

            {(trends?.length ?? 0) > 0 && (
              <div className="rounded-lg border border-border/70 bg-card/60 p-3">
                <SectionTitle>{t("usage.trends")}</SectionTitle>
                <div className="mt-3 flex h-16 items-end gap-1">
                  {(trends ?? []).slice(-14).map((day) => {
                    const value = trendTotalTokens(day);
                    const height = Math.max(6, (value / trendMax) * 64);
                    return (
                      <div
                        key={day.date}
                        className="flex min-w-0 flex-1 items-end"
                        title={`${day.date}: ${value.toLocaleString()}`}
                      >
                        <div
                          className="w-full rounded-t bg-blue-500/75"
                          style={{ height }}
                        />
                      </div>
                    );
                  })}
                </div>
              </div>
            )}

            <div className="grid gap-3">
              <RankSection
                title={t("usage.trayPanel.apps", "Apps")}
                empty={!hasUsage}
                emptyLabel={t("usage.noData")}
              >
                {apps
                  .slice()
                  .sort(
                    (a, b) =>
                      b.summary.realTotalTokens - a.summary.realTotalTokens,
                  )
                  .slice(0, 4)
                  .map((app) => {
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
                        accent="bg-blue-500"
                      />
                    );
                  })}
              </RankSection>

              <RankSection
                title={t("usage.modelStats")}
                empty={(models?.length ?? 0) === 0}
                emptyLabel={t("usage.noData")}
              >
                {(models ?? []).slice(0, 5).map((model) => (
                  <RankRow
                    key={model.model}
                    label={model.model}
                    value={formatTokensShort(model.totalTokens, lang)}
                    secondary={formatUsdAuto(model.totalCost)}
                    barValue={model.totalTokens}
                    maxValue={models?.[0]?.totalTokens ?? 0}
                    accent="bg-violet-500"
                  />
                ))}
              </RankSection>
            </div>
          </div>
        </div>
      </section>
    </div>
  );
}

function Metric({
  icon,
  label,
  value,
  accent,
}: {
  icon: ReactNode;
  label: string;
  value: string;
  accent: string;
}) {
  return (
    <div className="min-w-0 rounded-md border border-border/60 bg-background/50 p-2">
      <div className={cn("mb-1 flex items-center gap-1", accent)}>
        {icon}
        <span className="truncate text-[10px] font-medium uppercase text-muted-foreground">
          {label}
        </span>
      </div>
      <div className="truncate text-sm font-semibold tabular-nums">{value}</div>
    </div>
  );
}

function SectionTitle({ children }: { children: ReactNode }) {
  return (
    <div className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
      {children}
    </div>
  );
}

function RankSection({
  title,
  empty,
  emptyLabel,
  children,
}: {
  title: string;
  empty: boolean;
  emptyLabel: string;
  children: ReactNode;
}) {
  return (
    <div className="rounded-lg border border-border/70 bg-card/60 p-3">
      <SectionTitle>{title}</SectionTitle>
      <div className="mt-2 space-y-2">
        {empty ? (
          <div className="rounded-md bg-muted/35 px-3 py-4 text-center text-xs text-muted-foreground">
            {emptyLabel}
          </div>
        ) : (
          children
        )}
      </div>
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
}: {
  icon?: ReactNode;
  label: string;
  value: string;
  secondary?: string;
  barValue: number;
  maxValue: number;
  accent: string;
}) {
  const width = maxValue > 0 ? Math.max(4, (barValue / maxValue) * 100) : 0;

  return (
    <div className="min-w-0">
      <div className="mb-1 flex min-w-0 items-center gap-2">
        <span className="flex h-5 w-5 shrink-0 items-center justify-center rounded bg-muted/60">
          {icon}
        </span>
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
      <div className="h-1.5 overflow-hidden rounded bg-muted/60">
        <div
          className={cn("h-full rounded", accent)}
          style={{ width: `${width}%` }}
        />
      </div>
    </div>
  );
}
