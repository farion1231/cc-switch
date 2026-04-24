import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Bot,
  Coins,
  Database,
  Download,
  HardDriveDownload,
  Loader2,
  MessageSquareText,
  Send,
  Zap,
} from "lucide-react";
import {
  useModelTrends,
  useModelStats,
  useUsageSummary,
  useUsageTrends,
} from "@/lib/query/usage";
import { resolveUsageRange } from "@/lib/usageRange";
import { cn } from "@/lib/utils";
import {
  fmtInt,
  fmtUsd,
  getLocaleFromLanguage,
  parseFiniteNumber,
} from "./format";
import type { ModelStats, UsageRangeSelection } from "@/types/usage";

interface UsageTrendChartProps {
  range: UsageRangeSelection;
  rangeLabel: string;
  appType?: string;
  refreshIntervalMs: number;
}

type PanelView = "overview" | "models";
type TokenUnit = "k" | "m";

const MODEL_COLORS = [
  { bar: "bg-blue-600", text: "text-blue-600" },
  { bar: "bg-blue-500", text: "text-blue-500" },
  { bar: "bg-sky-500", text: "text-sky-500" },
  { bar: "bg-blue-300", text: "text-blue-300" },
  { bar: "bg-sky-300", text: "text-sky-300" },
  { bar: "bg-blue-200", text: "text-blue-200" },
  { bar: "bg-slate-400", text: "text-slate-400" },
  { bar: "bg-zinc-400", text: "text-zinc-400" },
];

const HEATMAP_DAYS = 7;

function pct(value: number, total: number): string {
  if (total <= 0) return "0.0%";
  return `${((value / total) * 100).toFixed(1)}%`;
}

function formatToken(value: number, unit: TokenUnit, locale: string): string {
  const divisor = unit === "m" ? 1_000_000 : 1_000;
  const suffix = unit.toUpperCase();
  return `${(value / divisor).toLocaleString(locale, {
    maximumFractionDigits: unit === "m" ? 2 : 1,
  })}${suffix}`;
}

function formatCacheValue(
  value: number,
  denominator: number,
  showPercent: boolean,
  unit: TokenUnit,
  locale: string,
) {
  return showPercent
    ? pct(value, denominator)
    : formatToken(value, unit, locale);
}

function normalizeModel(stat: ModelStats) {
  const inputTokens = stat.totalInputTokens ?? stat.totalTokens ?? 0;
  const outputTokens = stat.totalOutputTokens ?? 0;
  const cacheCreationTokens = stat.totalCacheCreationTokens ?? 0;
  const cacheReadTokens = stat.totalCacheReadTokens ?? 0;
  const totalTokens = stat.totalTokens ?? inputTokens + outputTokens;

  return {
    ...stat,
    inputTokens,
    outputTokens,
    cacheCreationTokens,
    cacheReadTokens,
    totalTokens,
    promptTokens: inputTokens + cacheCreationTokens + cacheReadTokens,
  };
}

function dateKey(date: Date): string {
  return `${date.getFullYear()}-${date.getMonth()}-${date.getDate()}`;
}

export function UsageTrendChart({
  range,
  rangeLabel,
  appType,
  refreshIntervalMs,
}: UsageTrendChartProps) {
  const { t, i18n } = useTranslation();
  const [view, setView] = useState<PanelView>("overview");
  const [tokenUnit, setTokenUnit] = useState<TokenUnit>(() => {
    const stored = localStorage.getItem("usage-token-unit");
    return stored === "m" ? "m" : "k";
  });
  const [showCacheCreationPercent, setShowCacheCreationPercent] =
    useState(false);
  const [showCacheReadPercent, setShowCacheReadPercent] = useState(false);

  useEffect(() => {
    localStorage.setItem("usage-token-unit", tokenUnit);
  }, [tokenUnit]);

  const refetchInterval: number | false =
    refreshIntervalMs > 0 ? refreshIntervalMs : false;
  const queryOptions = {
    refetchInterval,
  };
  const yearHeatmapRange = useMemo<UsageRangeSelection>(() => {
    const now = new Date();
    const yearStart = new Date(now.getFullYear(), 0, 1);
    const todayEnd = new Date(
      now.getFullYear(),
      now.getMonth(),
      now.getDate(),
      23,
      59,
      59,
    );

    return {
      preset: "custom",
      customStartDate: Math.floor(yearStart.getTime() / 1000),
      customEndDate: Math.floor(todayEnd.getTime() / 1000),
    };
  }, []);
  const { data: summary, isLoading: isSummaryLoading } = useUsageSummary(
    range,
    appType,
    queryOptions,
  );
  const { data: trends, isLoading: isTrendsLoading } = useUsageTrends(
    range,
    appType,
    queryOptions,
  );
  const { data: modelStatsRaw, isLoading: isModelsLoading } = useModelStats(
    range,
    appType,
    queryOptions,
  );
  const { data: modelTrendsRaw, isLoading: isModelTrendsLoading } =
    useModelTrends(range, appType, queryOptions);
  const { data: heatmapTrends, isLoading: isHeatmapLoading } = useUsageTrends(
    yearHeatmapRange,
    appType,
    queryOptions,
  );

  const { startDate, endDate } = resolveUsageRange(range);
  const language = i18n.resolvedLanguage || i18n.language || "en";
  const locale = getLocaleFromLanguage(language);
  const isLoading =
    isSummaryLoading ||
    isTrendsLoading ||
    isModelsLoading ||
    isModelTrendsLoading;

  const trendData = useMemo(() => {
    const durationSeconds = Math.max(endDate - startDate, 0);
    const isHourly = durationSeconds <= 24 * 60 * 60;

    return (trends ?? []).map((stat) => {
      const pointDate = new Date(stat.date);
      const tokens =
        stat.totalTokens ?? stat.totalInputTokens + stat.totalOutputTokens;

      return {
        rawDate: stat.date,
        label: isHourly
          ? pointDate.toLocaleString(locale, {
              month: "2-digit",
              day: "2-digit",
              hour: "2-digit",
            })
          : pointDate.toLocaleDateString(locale, {
              month: "2-digit",
              day: "2-digit",
            }),
        hour: pointDate.getHours(),
        requestCount: stat.requestCount,
        inputTokens: stat.totalInputTokens,
        outputTokens: stat.totalOutputTokens,
        cacheCreationTokens: stat.totalCacheCreationTokens,
        cacheReadTokens: stat.totalCacheReadTokens,
        tokens,
      };
    });
  }, [endDate, locale, startDate, trends]);

  const heatmapData = useMemo(
    () =>
      (heatmapTrends ?? []).map((stat) => {
        const pointDate = new Date(stat.date);
        const tokens =
          stat.totalTokens ?? stat.totalInputTokens + stat.totalOutputTokens;

        return {
          rawDate: stat.date,
          label: pointDate.toLocaleDateString(locale, {
            month: "2-digit",
            day: "2-digit",
          }),
          requestCount: stat.requestCount,
          tokens,
        };
      }),
    [heatmapTrends, locale],
  );

  const modelStats = useMemo(
    () =>
      (modelStatsRaw ?? [])
        .map(normalizeModel)
        .sort((a, b) => b.totalTokens - a.totalTokens),
    [modelStatsRaw],
  );

  const totals = useMemo(() => {
    const inputTokens = summary?.totalInputTokens ?? 0;
    const outputTokens = summary?.totalOutputTokens ?? 0;
    const cacheCreationTokens = summary?.totalCacheCreationTokens ?? 0;
    const cacheReadTokens = summary?.totalCacheReadTokens ?? 0;
    const totalTokens = inputTokens + outputTokens;
    const promptTokens = inputTokens + cacheCreationTokens + cacheReadTokens;
    const activePeriods = trendData.filter((item) => item.tokens > 0).length;
    const peak = trendData.reduce(
      (best, item) => (item.tokens > best.tokens ? item : best),
      { label: "--", tokens: 0, requestCount: 0 },
    );

    return {
      requests: summary?.totalRequests ?? 0,
      totalCost: parseFiniteNumber(summary?.totalCost) ?? 0,
      inputTokens,
      outputTokens,
      cacheCreationTokens,
      cacheReadTokens,
      totalTokens,
      promptTokens,
      activePeriods,
      peak,
      favoriteModel: modelStats[0]?.model ?? "--",
    };
  }, [modelStats, summary, trendData]);

  if (isLoading) {
    return (
      <div className="flex h-[560px] items-center justify-center rounded-2xl border border-border/50 bg-card/40 backdrop-blur-sm">
        <Loader2 className="h-8 w-8 animate-spin text-muted-foreground/40" />
      </div>
    );
  }

  const maxTrendTokens = Math.max(...trendData.map((item) => item.tokens), 1);

  return (
    <section className="overflow-hidden rounded-2xl border border-border/50 bg-card/40 p-5 shadow-sm backdrop-blur-sm">
      <div className="mb-5 flex items-center justify-between gap-4">
        <div className="inline-flex w-fit items-center gap-1 rounded-lg bg-muted/70 p-1">
          {(["overview", "models"] as PanelView[]).map((item) => (
            <button
              key={item}
              type="button"
              onClick={() => setView(item)}
              className={cn(
                "rounded-md px-4 py-1.5 text-sm font-medium transition-all",
                view === item
                  ? "bg-background text-foreground shadow-sm"
                  : "text-muted-foreground hover:text-foreground",
              )}
            >
              {item === "overview"
                ? t("usage.overview", "总览")
                : t("usage.modelsView", "模型")}
            </button>
          ))}
        </div>

        <div className="ml-auto flex shrink-0 items-center gap-2">
          <div className="rounded-lg bg-muted/50 px-3 py-1.5 text-xs text-muted-foreground">
            {rangeLabel}
          </div>
          <SegmentedControl
            value={tokenUnit}
            options={[
              { value: "k", label: "K" },
              { value: "m", label: "M" },
            ]}
            onChange={(next) => setTokenUnit(next as TokenUnit)}
          />
        </div>
      </div>

      {view === "overview" ? (
        <OverviewPanel
          t={t}
          locale={locale}
          tokenUnit={tokenUnit}
          totals={totals}
          trendData={trendData}
          heatmapData={heatmapData}
          maxTrendTokens={maxTrendTokens}
          isHeatmapLoading={isHeatmapLoading}
        />
      ) : (
        <ModelsPanel
          t={t}
          locale={locale}
          tokenUnit={tokenUnit}
          modelStats={modelStats}
          modelTrends={modelTrendsRaw ?? []}
          trendData={trendData}
          totalTokens={modelStats.reduce(
            (sum, item) => sum + item.totalTokens,
            0,
          )}
          showCacheCreationPercent={showCacheCreationPercent}
          showCacheReadPercent={showCacheReadPercent}
          onToggleCacheCreation={setShowCacheCreationPercent}
          onToggleCacheRead={setShowCacheReadPercent}
        />
      )}
    </section>
  );
}

function OverviewPanel({
  t,
  locale,
  tokenUnit,
  totals,
  trendData,
  heatmapData,
  maxTrendTokens,
  isHeatmapLoading,
}: {
  t: ReturnType<typeof useTranslation>["t"];
  locale: string;
  tokenUnit: TokenUnit;
  totals: {
    requests: number;
    totalCost: number;
    inputTokens: number;
    outputTokens: number;
    cacheCreationTokens: number;
    cacheReadTokens: number;
    totalTokens: number;
    promptTokens: number;
    activePeriods: number;
    peak: { label: string; tokens: number; requestCount: number };
    favoriteModel: string;
  };
  trendData: Array<{
    rawDate: string;
    label: string;
    requestCount: number;
    tokens: number;
  }>;
  heatmapData: Array<{
    rawDate: string;
    label: string;
    requestCount: number;
    tokens: number;
  }>;
  maxTrendTokens: number;
  isHeatmapLoading: boolean;
}) {
  const cards = [
    {
      label: t("usage.requests", "请求数"),
      value: fmtInt(totals.requests, locale),
      icon: MessageSquareText,
    },
    {
      label: t("usage.totalTokens", "总 Token 数"),
      value: formatToken(totals.totalTokens, tokenUnit, locale),
      icon: Zap,
    },
    {
      label: t("usage.inputTokens", "输入"),
      value: formatToken(totals.inputTokens, tokenUnit, locale),
      icon: Send,
    },
    {
      label: t("usage.outputTokens", "输出"),
      value: formatToken(totals.outputTokens, tokenUnit, locale),
      icon: Download,
    },
    {
      label: t("usage.cacheCreationTokens", "缓存创建"),
      value: formatToken(totals.cacheCreationTokens, tokenUnit, locale),
      icon: Database,
    },
    {
      label: t("usage.cacheReadTokens", "缓存命中"),
      value: formatToken(totals.cacheReadTokens, tokenUnit, locale),
      icon: HardDriveDownload,
    },
    {
      label: t("usage.costOverview", "成本概览"),
      value: fmtUsd(totals.totalCost, 4),
      icon: Coins,
    },
    {
      label: t("usage.favoriteModel", "常用模型"),
      value: totals.favoriteModel,
      icon: Bot,
      title: totals.favoriteModel,
    },
  ];

  return (
    <div className="space-y-5">
      <div className="grid grid-cols-4 gap-2">
        {cards.map((card) => (
          <div key={card.label} className="min-w-0 rounded-lg bg-muted/55 p-4">
            <div className="mb-2 flex items-center justify-between gap-2 text-muted-foreground">
              <span className="truncate text-[15px]">{card.label}</span>
              <card.icon className="h-4 w-4 shrink-0" />
            </div>
            <div
              className="truncate text-2xl font-semibold tracking-normal text-foreground"
              title={card.title ?? card.value}
            >
              {card.value}
            </div>
          </div>
        ))}
      </div>

      <div className="rounded-lg bg-muted/35 p-4">
        <div className="mb-4 flex items-center justify-between gap-3">
          <div>
            <h3 className="text-sm font-semibold">
              {t("usage.trends", "使用趋势")}
            </h3>
            <p className="mt-1 text-xs text-muted-foreground">
              {t("usage.peakUsage", "峰值")} {totals.peak.label} ·{" "}
              {formatToken(totals.peak.tokens, tokenUnit, locale)}
            </p>
          </div>
          <span className="text-sm font-semibold tabular-nums">
            {fmtUsd(totals.totalCost, 4)}
          </span>
        </div>

        <div className="flex h-56 gap-0.5">
          {/* Y-axis labels */}
          <div className="flex w-12 shrink-0 flex-col justify-between py-0.5 text-right">
            {[1, 0.75, 0.5, 0.25, 0].map((ratio) => (
              <span
                key={ratio}
                className="pr-0.5 text-[10px] leading-none text-muted-foreground/50"
              >
                {formatToken(maxTrendTokens * ratio, tokenUnit, locale)}
              </span>
            ))}
          </div>
          {/* Bars */}
          <div className="relative flex flex-1 items-end gap-1 overflow-hidden pb-0.5">
            {/* Horizontal grid lines */}
            <div className="pointer-events-none absolute inset-0 flex flex-col justify-between pb-0.5">
              {[0.25, 0.5, 0.75, 1].map((ratio) => (
                <div
                  key={ratio}
                  className="border-t border-border/30"
                  style={{ height: 0 }}
                />
              ))}
            </div>
            {trendData.length === 0 ? (
              <div className="flex h-full w-full items-center justify-center text-sm text-muted-foreground">
                {t("usage.noData", "暂无数据")}
              </div>
            ) : (
              trendData.map((item) => {
                const height = Math.max(
                  2,
                  (item.tokens / maxTrendTokens) * 100,
                );
                return (
                  <div
                    key={`${item.rawDate}-${item.label}`}
                    className="group relative flex h-full min-w-4 flex-1 items-end justify-center"
                  >
                    {/* Hover tooltip */}
                    <div className="pointer-events-none absolute -top-7 left-1/2 z-10 -translate-x-1/2 whitespace-nowrap rounded-md bg-foreground px-1.5 py-0.5 text-[10px] font-medium text-background opacity-0 shadow transition-opacity group-hover:opacity-100">
                      {item.label}:{" "}
                      {formatToken(item.tokens, tokenUnit, locale)}
                    </div>
                    <div
                      className="relative z-[1] w-full max-w-8 rounded-t-sm bg-blue-500 transition-all group-hover:bg-blue-600"
                      style={{ height: `${height}%` }}
                    />
                  </div>
                );
              })
            )}
          </div>
        </div>

        {trendData.length > 0 && (
          <div className="mt-2 flex justify-between gap-2 text-xs text-muted-foreground">
            <span>{trendData[0]?.label}</span>
            <span>{trendData[Math.floor(trendData.length / 2)]?.label}</span>
            <span>{trendData[trendData.length - 1]?.label}</span>
          </div>
        )}
      </div>

      <Heatmap
        t={t}
        locale={locale}
        tokenUnit={tokenUnit}
        trendData={heatmapData}
        maxTrendTokens={Math.max(...heatmapData.map((item) => item.tokens), 1)}
        isLoading={isHeatmapLoading}
      />
    </div>
  );
}

function ModelsPanel({
  t,
  locale,
  tokenUnit,
  modelStats,
  modelTrends,
  trendData,
  totalTokens,
  showCacheCreationPercent,
  showCacheReadPercent,
  onToggleCacheCreation,
  onToggleCacheRead,
}: {
  t: ReturnType<typeof useTranslation>["t"];
  locale: string;
  tokenUnit: TokenUnit;
  modelStats: ReturnType<typeof normalizeModel>[];
  modelTrends: Array<{
    date: string;
    model: string;
    requestCount: number;
    totalTokens: number;
    totalInputTokens: number;
    totalOutputTokens: number;
    totalCacheCreationTokens: number;
    totalCacheReadTokens: number;
  }>;
  trendData: Array<{
    rawDate: string;
    label: string;
    requestCount: number;
    tokens: number;
  }>;
  totalTokens: number;
  showCacheCreationPercent: boolean;
  showCacheReadPercent: boolean;
  onToggleCacheCreation: (checked: boolean) => void;
  onToggleCacheRead: (checked: boolean) => void;
}) {
  const topModels = modelStats.slice(0, 8);
  const visibleModelNames = new Set(topModels.map((item) => item.model));
  const modelColorByName = new Map(
    topModels.map((item, index) => [
      item.model,
      MODEL_COLORS[index % MODEL_COLORS.length],
    ]),
  );
  const bucketRows = trendData.map((bucket) => {
    const rows = modelTrends.filter((item) => item.date === bucket.rawDate);
    const segmentMap = new Map<string, number>();
    let otherTokens = 0;

    for (const row of rows) {
      if (visibleModelNames.has(row.model)) {
        segmentMap.set(
          row.model,
          (segmentMap.get(row.model) ?? 0) + row.totalTokens,
        );
      } else {
        otherTokens += row.totalTokens;
      }
    }

    const segments = topModels
      .map((model) => ({
        model: model.model,
        tokens: segmentMap.get(model.model) ?? 0,
        color: modelColorByName.get(model.model) ?? MODEL_COLORS[0],
      }))
      .filter((item) => item.tokens > 0);

    if (otherTokens > 0) {
      segments.push({
        model: t("usage.otherModels", "其他模型"),
        tokens: otherTokens,
        color: { bar: "bg-slate-300", text: "text-slate-300" },
      });
    }

    return {
      ...bucket,
      segments,
      tokens: rows.reduce((sum, item) => sum + item.totalTokens, 0),
    };
  });
  const maxBucketTokens = Math.max(...bucketRows.map((item) => item.tokens), 1);

  return (
    <div className="space-y-5">
      <div className="flex flex-wrap items-center justify-end gap-2 text-xs text-muted-foreground">
        <TogglePill
          label={t("usage.cacheCreationPercent", "缓存创建百分比")}
          checked={showCacheCreationPercent}
          onCheckedChange={onToggleCacheCreation}
        />
        <TogglePill
          label={t("usage.cacheHitPercent", "缓存命中百分比")}
          checked={showCacheReadPercent}
          onCheckedChange={onToggleCacheRead}
        />
      </div>

      <div className="rounded-lg bg-muted/35 p-4">
        <div className="mb-4 flex items-center justify-between">
          <h3 className="text-sm font-semibold">
            {t("usage.modelStats", "模型统计")}
          </h3>
          <span className="text-xs text-muted-foreground">
            {formatToken(totalTokens, tokenUnit, locale)}
          </span>
        </div>

        <div className="flex h-64 gap-0.5">
          {/* Y-axis labels */}
          <div className="flex w-12 shrink-0 flex-col justify-between py-0.5 text-right">
            {[1, 0.75, 0.5, 0.25, 0].map((ratio) => (
              <span
                key={ratio}
                className="pr-0.5 text-[10px] leading-none text-muted-foreground/50"
              >
                {formatToken(maxBucketTokens * ratio, tokenUnit, locale)}
              </span>
            ))}
          </div>
          {/* Bars */}
          <div className="relative flex flex-1 items-end gap-1 overflow-hidden pb-0.5">
            {/* Horizontal grid lines */}
            <div className="pointer-events-none absolute inset-0 flex flex-col justify-between pb-0.5">
              {[0.25, 0.5, 0.75, 1].map((ratio) => (
                <div
                  key={ratio}
                  className="border-t border-border/30"
                  style={{ height: 0 }}
                />
              ))}
            </div>
            {bucketRows.length === 0 ? (
              <div className="flex h-full w-full items-center justify-center text-sm text-muted-foreground">
                {t("usage.noData", "暂无数据")}
              </div>
            ) : (
              bucketRows.map((bucket) => {
                const height = Math.max(
                  3,
                  (bucket.tokens / maxBucketTokens) * 100,
                );
                return (
                  <div
                    key={bucket.rawDate}
                    className="group relative flex h-full min-w-4 flex-1 items-end justify-center"
                  >
                    {/* Hover tooltip */}
                    <div className="pointer-events-none absolute -top-7 left-1/2 z-10 -translate-x-1/2 whitespace-nowrap rounded-md bg-foreground px-1.5 py-0.5 text-[10px] font-medium text-background opacity-0 shadow transition-opacity group-hover:opacity-100">
                      {bucket.label}:{" "}
                      {formatToken(bucket.tokens, tokenUnit, locale)}
                    </div>
                    <div
                      className="relative z-[1] flex w-full max-w-8 flex-col-reverse overflow-hidden rounded-t-sm"
                      style={{ height: `${height}%` }}
                    >
                      {bucket.segments.map((segment) => (
                        <div
                          key={segment.model}
                          className={cn(
                            "w-full transition-all group-hover:brightness-95",
                            segment.color.bar,
                          )}
                          style={{
                            height: `${(segment.tokens / Math.max(bucket.tokens, 1)) * 100}%`,
                          }}
                          title={`${segment.model}: ${formatToken(
                            segment.tokens,
                            tokenUnit,
                            locale,
                          )}`}
                        />
                      ))}
                    </div>
                  </div>
                );
              })
            )}
          </div>
        </div>
        {bucketRows.length > 0 && (
          <div className="mt-2 flex justify-between gap-2 text-xs text-muted-foreground">
            <span>{bucketRows[0]?.label}</span>
            <span>{bucketRows[Math.floor(bucketRows.length / 2)]?.label}</span>
            <span>{bucketRows[bucketRows.length - 1]?.label}</span>
          </div>
        )}
      </div>

      <div className="space-y-1">
        {topModels.map((stat, index) => (
          <div
            key={stat.model}
            className="grid items-center gap-x-3 gap-y-1 rounded-md px-1 py-0.5 text-sm sm:grid-cols-[minmax(0,1fr)_auto_auto]"
          >
            <div className="flex min-w-0 items-center gap-2">
              <span
                className={cn(
                  "h-2.5 w-2.5 shrink-0 rounded-sm",
                  MODEL_COLORS[index % MODEL_COLORS.length].bar,
                )}
              />
              <span
                className="truncate text-base font-medium text-foreground"
                title={stat.model}
              >
                {stat.model}
              </span>
            </div>
            <div className="text-right text-sm tabular-nums text-muted-foreground">
              {formatToken(stat.inputTokens, tokenUnit, locale)} in ·{" "}
              {formatToken(stat.outputTokens, tokenUnit, locale)} out
            </div>
            <div className="text-right text-base font-semibold tabular-nums">
              {pct(stat.totalTokens, totalTokens)}
            </div>
            <div className="grid gap-x-3 gap-y-0.5 text-[11px] leading-tight text-muted-foreground sm:col-span-2 sm:col-start-2 sm:grid-cols-2">
              <span>
                {t("usage.cacheCreationTokens", "缓存创建")}:{" "}
                {formatCacheValue(
                  stat.cacheCreationTokens,
                  stat.promptTokens,
                  showCacheCreationPercent,
                  tokenUnit,
                  locale,
                )}
              </span>
              <span>
                {t("usage.cacheReadTokens", "缓存命中")}:{" "}
                {formatCacheValue(
                  stat.cacheReadTokens,
                  stat.promptTokens,
                  showCacheReadPercent,
                  tokenUnit,
                  locale,
                )}
              </span>
            </div>
          </div>
        ))}
        {modelStats.length > topModels.length && (
          <div className="pt-0.5 text-sm text-muted-foreground">
            {t("usage.showMoreModels", "Show {{count}} more", {
              count: modelStats.length - topModels.length,
            })}
          </div>
        )}
      </div>
    </div>
  );
}

function Heatmap({
  t,
  locale,
  tokenUnit,
  trendData,
  maxTrendTokens,
  isLoading,
}: {
  t: ReturnType<typeof useTranslation>["t"];
  locale: string;
  tokenUnit: TokenUnit;
  trendData: Array<{
    rawDate: string;
    label: string;
    requestCount: number;
    tokens: number;
  }>;
  maxTrendTokens: number;
  isLoading: boolean;
}) {
  const dataByDate = useMemo(() => {
    const map = new Map<string, (typeof trendData)[number]>();
    for (const item of trendData) {
      const date = new Date(item.rawDate);
      const key = dateKey(date);
      map.set(key, item);
    }
    return map;
  }, [trendData]);

  const { cells, monthLabels, weekDayLabels, weekCount } = useMemo(() => {
    const now = new Date();
    const year = now.getFullYear();
    const yearStart = new Date(year, 0, 1);
    const today = new Date(year, now.getMonth(), now.getDate());
    const gridStart = new Date(yearStart);
    gridStart.setDate(yearStart.getDate() - yearStart.getDay());
    const gridEnd = new Date(today);
    gridEnd.setDate(today.getDate() + (6 - today.getDay()));
    const weekCount =
      Math.floor(
        (gridEnd.getTime() - gridStart.getTime()) /
          (HEATMAP_DAYS * 24 * 60 * 60 * 1000),
      ) + 1;
    const monthFormatter = new Intl.DateTimeFormat(locale, {
      month: "numeric",
    });
    const weekDayFormatter = new Intl.DateTimeFormat(locale, {
      weekday: "short",
    });
    const labels = Array.from({ length: weekCount }, () => "");
    const dayLabels = Array.from({ length: HEATMAP_DAYS }, (_, dayIndex) => {
      const date = new Date(2024, 0, 7 + dayIndex);
      return weekDayFormatter.format(date);
    });
    const yearCells: Array<{
      key: string;
      label: string;
      tokens: number;
      requestCount: number;
      isOutsideYear: boolean;
    }> = [];

    for (let weekIndex = 0; weekIndex < weekCount; weekIndex += 1) {
      for (let dayIndex = 0; dayIndex < HEATMAP_DAYS; dayIndex += 1) {
        const index = weekIndex * HEATMAP_DAYS + dayIndex;
        const date = new Date(gridStart);
        date.setDate(gridStart.getDate() + index);
        const key = dateKey(date);
        const isOutsideYear = date < yearStart || date > today;
        const isMonthStart =
          date.getDate() === 1 && date.getFullYear() === year;

        if (weekIndex === 0 || (isMonthStart && labels[weekIndex] === "")) {
          labels[weekIndex] = isOutsideYear ? "" : monthFormatter.format(date);
        }

        const item = dataByDate.get(key);
        yearCells.push({
          key,
          label: date.toLocaleDateString(locale, {
            year: "numeric",
            month: "2-digit",
            day: "2-digit",
          }),
          tokens: isOutsideYear ? 0 : (item?.tokens ?? 0),
          requestCount: isOutsideYear ? 0 : (item?.requestCount ?? 0),
          isOutsideYear,
        });
      }
    }

    return {
      cells: yearCells,
      monthLabels: labels,
      weekDayLabels: dayLabels,
      weekCount,
    };
  }, [dataByDate, locale]);
  const totalTokens = cells.reduce((sum, item) => sum + item.tokens, 0);
  const activeCount = cells.filter((item) => item.tokens > 0).length;
  const monthGridStyle = {
    gridTemplateColumns: `repeat(${weekCount}, minmax(0, 1fr))`,
  };
  const heatmapGridStyle = {
    gridTemplateColumns: `repeat(${weekCount}, minmax(0, 1fr))`,
    gridTemplateRows: `repeat(${HEATMAP_DAYS}, auto)`,
  };

  return (
    <div className="rounded-lg bg-muted/35 p-4">
      <div className="mb-3 flex items-center justify-between gap-3">
        <h3 className="text-sm font-semibold">
          {t("usage.activityHeatmap", "时间热力图")}
        </h3>
        <span className="text-xs text-muted-foreground">
          {formatToken(totalTokens, tokenUnit, locale)}
        </span>
      </div>

      <div className="rounded-md bg-background/35 p-3">
        <div className="grid grid-cols-[2rem_minmax(0,1fr)] gap-x-2">
          <div />
          <div
            className="grid gap-[3px] pb-2 text-xs font-medium leading-none text-muted-foreground"
            style={monthGridStyle}
          >
            {monthLabels.map((label, index) => (
              <span key={`${label}-${index}`} className="truncate">
                {label}
              </span>
            ))}
          </div>

          <div className="grid grid-rows-7 gap-[3px] text-xs leading-none text-muted-foreground">
            {weekDayLabels.map((label, index) => (
              <span key={`${label}-${index}`} className="flex items-center">
                {label}
              </span>
            ))}
          </div>

          <div className="grid gap-[3px]" style={heatmapGridStyle}>
            {isLoading
              ? Array.from({ length: weekCount * HEATMAP_DAYS }).map(
                  (_, index) => (
                    <span
                      key={index}
                      className="aspect-square w-full animate-pulse rounded-[3px] bg-muted"
                    />
                  ),
                )
              : cells.map((item) => (
                  <span
                    key={item.key}
                    className={cn(
                      "aspect-square w-full rounded-[3px] ring-1 ring-inset ring-black/5 transition-colors",
                      item.isOutsideYear
                        ? "bg-transparent ring-transparent"
                        : heatClass(item.tokens, maxTrendTokens),
                    )}
                    title={`${item.label}: ${formatToken(
                      item.tokens,
                      tokenUnit,
                      locale,
                    )}`}
                  />
                ))}
          </div>
        </div>
      </div>

      <p className="mt-4 text-sm text-muted-foreground">
        {t("usage.heatmapSummary", "已记录 {{count}} 个活跃时段", {
          count: activeCount,
        })}
      </p>
    </div>
  );
}

function heatClass(value: number, max: number): string {
  if (value <= 0) return "bg-muted";
  const ratio = value / max;
  if (ratio > 0.78) return "bg-blue-700";
  if (ratio > 0.5) return "bg-blue-500";
  if (ratio > 0.25) return "bg-blue-400";
  return "bg-blue-200 dark:bg-blue-900/60";
}

function SegmentedControl({
  value,
  options,
  onChange,
}: {
  value: string;
  options: Array<{ value: string; label: string }>;
  onChange: (value: string) => void;
}) {
  return (
    <div className="inline-flex items-center gap-1 rounded-lg bg-muted/70 p-1">
      {options.map((option) => (
        <button
          key={option.value}
          type="button"
          onClick={() => onChange(option.value)}
          className={cn(
            "min-w-9 rounded-md px-2.5 py-1.5 text-xs font-medium transition-all",
            value === option.value
              ? "bg-background text-foreground shadow-sm"
              : "text-muted-foreground hover:text-foreground",
          )}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}

function TogglePill({
  label,
  checked,
  onCheckedChange,
}: {
  label: string;
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
}) {
  return (
    <button
      type="button"
      onClick={() => onCheckedChange(!checked)}
      className={cn(
        "rounded-full px-3 py-1.5 transition-colors",
        checked
          ? "bg-blue-500 text-white"
          : "bg-muted/60 text-muted-foreground hover:text-foreground",
      )}
    >
      {label}
    </button>
  );
}
