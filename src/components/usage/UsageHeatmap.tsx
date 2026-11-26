import {
  Fragment,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
} from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { ChevronDown, Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { useUsageFirstDate, useUsageTrends } from "@/lib/query/usage";
import { cn } from "@/lib/utils";
import type {
  DailyStats,
  RuntimeTrendStats,
  UsageRangeSelection,
} from "@/types/usage";
import {
  formatHitRatePercent,
  formatTokensShort,
  getLocaleFromLanguage,
} from "./format";
import { trendMetricsOf } from "./UsageTrendChart";
import { UsageTooltipCard } from "./UsageTooltipCard";
import { useMoneyFormat } from "@/hooks/useMoneyFormat";
import type { UsageMetric } from "./rememberedChoice";
import { runtimeColor, runtimeName } from "./runtimePalette";
import { buildModelTrendSeries, modelTrendMetric } from "./modelTrendSeries";
import {
  UsageBreakdownControl,
  type UsageBreakdown,
} from "./UsageBreakdownControl";

type HeatModel = ReturnType<typeof buildModelTrendSeries>[number] & {
  label: string;
};
type BreakdownRow = { label: string; color: string; value: number | null };

const WEEKS = 53;
const DAY_MS = 24 * 60 * 60 * 1000;
/** 0 档用底色；1–4 档由主题色和深浅共同决定。 */
const LEVELS = [0, 1, 2, 3, 4];

interface UsageHeatmapProps {
  /** 格子深浅按哪个指标分档：由上方指标条选 */
  metric: UsageMetric;
  appType?: string;
  providerName?: string;
  model?: string;
  refreshIntervalMs: number;
}

interface DayCell {
  key: string;
  date: Date;
  /** 按当前指标分档的值；null = 这天没有可画的数据（画成底色） */
  value: number | null;
  /** 命中率指标时悬停卡片里显示 */
  hitRate: number | null;
  tokens: number;
  requests: number;
  cost: number;
  future: boolean;
  color?: string;
  runtimeStats: RuntimeTrendStats[];
  models: BreakdownRow[];
}

/** 一段 53 周：最新一段右端是今天，更早的段首尾相接往前排 */
interface SpanGrid {
  start: Date;
  /** 这段的最后一天（最新一段是今天） */
  end: Date;
  weeks: DayCell[][];
  monthLabels: (number | null)[];
  totals: { tokens: number; requests: number; cost: number };
}

interface HoverState {
  day: string;
  /** Viewport coordinates of the cell. */
  x: number;
  top: number;
  bottom: number;
}

function startOfDay(date: Date): Date {
  return new Date(date.getFullYear(), date.getMonth(), date.getDate());
}

function dayKey(date: Date): string {
  return `${date.getFullYear()}-${date.getMonth() + 1}-${date.getDate()}`;
}

/** 周一为一周第一天：0 = 周一 … 6 = 周日 */
function weekdayIndex(date: Date): number {
  return (date.getDay() + 6) % 7;
}

/** 两个本地日期相差的天数（按 UTC 日历算，不受夏令时影响） */
function daysBetween(from: Date, to: Date): number {
  return Math.round(
    (Date.UTC(to.getFullYear(), to.getMonth(), to.getDate()) -
      Date.UTC(from.getFullYear(), from.getMonth(), from.getDate())) /
      DAY_MS,
  );
}

function addDays(date: Date, days: number): Date {
  return new Date(date.getFullYear(), date.getMonth(), date.getDate() + days);
}

/**
 * 第 index 段的起点（周一）：第 0 段是往前 52 周那一周的周一，凑满 53 列、右端是本周；
 * 再往前每段 53 周，首尾相接。
 */
function spanStart(today: Date, index: number): Date {
  const thisMonday = addDays(today, -weekdayIndex(today));
  return addDays(thisMonday, -((WEEKS - 1) * 7 + index * WEEKS * 7));
}

/** 一共几段：最早有记录的那天落在哪一段；没有记录或在第 0 段里时只有 1 段 */
function spanCountFor(
  firstDate: string | null | undefined,
  today: Date,
): number {
  const match = firstDate ? /^(\d{4})-(\d{2})-(\d{2})$/.exec(firstDate) : null;
  if (!match) return 1;
  const first = new Date(
    Number(match[1]),
    Number(match[2]) - 1,
    Number(match[3]),
  );
  const before = daysBetween(first, spanStart(today, 0));
  return before > 0 ? 1 + Math.ceil(before / (WEEKS * 7)) : 1;
}

function dominantRuntime(
  runtimes: RuntimeTrendStats[],
  metric: UsageMetric,
): string | undefined {
  let app: string | undefined;
  let largest = 0;
  for (const runtime of runtimes) {
    // For cache hit rate, choose the app with the most cacheable input.
    const value =
      metric === "hitRate"
        ? runtime.totalInputTokens +
          runtime.totalCacheCreationTokens +
          runtime.totalCacheReadTokens
        : (trendMetricsOf(runtime)[metric] ?? 0);
    if (
      value > largest ||
      (value === largest && app && runtime.appType < app)
    ) {
      app = runtime.appType;
      largest = value;
    }
  }
  return app;
}

/** 按有数据的天的四分位分档，避免个别高峰日把其余格子都压成最浅 */
function buildThresholds(values: (number | null)[]): number[] {
  const sorted = values
    .filter((v): v is number => v != null)
    .sort((a, b) => a - b);
  if (sorted.length === 0) return [Infinity, Infinity, Infinity];
  const at = (q: number) =>
    sorted[Math.min(sorted.length - 1, Math.floor(sorted.length * q))];
  return [at(0.25), at(0.5), at(0.75)];
}

function levelOf(value: number | null, thresholds: number[]): number {
  if (value == null) return 0;
  if (value <= thresholds[0]) return 1;
  if (value <= thresholds[1]) return 2;
  if (value <= thresholds[2]) return 3;
  return 4;
}

function heatFillStyle(level: number, color?: string): CSSProperties {
  return {
    "--heat-fill": color
      ? `color-mix(in srgb, ${color} var(--heat-strength-${level}), transparent)`
      : `var(--heat-${level})`,
  } as CSSProperties;
}

/**
 * 从最早一段的起点到现在的按天数据。先查最早日期算出要几段，再查趋势；
 * 起点按周取整，同一周内查询键不变。
 */
function useAllSpansDailyTrends(
  filters: { appType?: string; providerName?: string; model?: string },
  refreshIntervalMs: number,
  includeModels: boolean,
) {
  const refetchInterval = refreshIntervalMs > 0 ? refreshIntervalMs : false;
  const firstDateQuery = useUsageFirstDate(filters, { refetchInterval });
  const today = startOfDay(new Date());
  const spanCount = firstDateQuery.isSuccess
    ? spanCountFor(firstDateQuery.data, today)
    : null;
  const startKey = dayKey(spanStart(today, (spanCount ?? 1) - 1));
  const selection = useMemo<UsageRangeSelection>(
    () => ({
      preset: "custom",
      customStartDate: Math.floor(
        spanStart(startOfDay(new Date()), (spanCount ?? 1) - 1).getTime() /
          1000,
      ),
      liveEndTime: true,
    }),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [startKey],
  );
  const trendsQuery = useUsageTrends(selection, filters, {
    enabled: spanCount != null,
    includeModels,
    refetchInterval,
  });
  return {
    spanCount: spanCount ?? 1,
    trends: trendsQuery.isPlaceholderData ? undefined : trendsQuery.data,
    isLoading:
      firstDateQuery.isPending ||
      trendsQuery.isLoading ||
      trendsQuery.isPlaceholderData,
  };
}

/** 把后端按天的数据铺成一段 53 周 × 7 天的格子（周一开头，未来的日子标记出来）。 */
function buildSpan(
  index: number,
  byDay: Map<string, DailyStats>,
  metric: UsageMetric,
  today: Date,
  mode: UsageBreakdown,
  models: HeatModel[],
): SpanGrid {
  const start = spanStart(today, index);
  const end = index === 0 ? today : addDays(spanStart(today, index - 1), -1);
  const weeks: DayCell[][] = [];
  const totals = { tokens: 0, requests: 0, cost: 0 };
  for (let w = 0; w < WEEKS; w++) {
    const column: DayCell[] = [];
    for (let d = 0; d < 7; d++) {
      const date = addDays(start, w * 7 + d);
      const key = dayKey(date);
      const stat = byDay.get(key);
      const metrics = stat ? trendMetricsOf(stat) : undefined;
      const value = metrics?.[metric] ?? null;
      const runtimeStats = mode === "app" ? (stat?.runtimeStats ?? []) : [];
      const app = dominantRuntime(runtimeStats, metric);
      let color = app ? runtimeColor(app) : undefined;
      const modelRows: BreakdownRow[] = [];
      let largestModelValue = 0;
      if (stat?.modelStats?.length) {
        const point = { xKey: key, modelStats: stat.modelStats };
        for (const model of models) {
          const amount = model.value(point);
          if (amount === 0) continue;
          modelRows.push({
            label: model.label,
            color: model.color,
            value: amount,
          });
          if (amount > largestModelValue) {
            largestModelValue = amount;
            color = model.color;
          }
        }
      }
      const cell: DayCell = {
        key,
        date,
        // Zero is a real cache hit rate, but an empty usage bucket.
        value:
          metric === "hitRate" || (value != null && value > 0) ? value : null,
        hitRate: metrics?.hitRate ?? null,
        tokens: metrics?.tokens ?? 0,
        requests: metrics?.requests ?? 0,
        cost: metrics?.cost ?? 0,
        future: date.getTime() > today.getTime(),
        color,
        runtimeStats,
        models: modelRows,
      };
      totals.tokens += cell.tokens;
      totals.requests += cell.requests;
      totals.cost += cell.cost;
      column.push(cell);
    }
    weeks.push(column);
  }

  // 月份标签：某一列包含当月 1 号（或第一列）时标出月份
  const monthLabels = weeks.map((column, w) => {
    if (w === 0) return column[0].date.getMonth() + 1;
    const first = column.find((cell) => cell.date.getDate() === 1);
    return first ? first.date.getMonth() + 1 : null;
  });

  return { start, end, weeks, monthLabels, totals };
}

export function UsageHeatmap({
  metric,
  appType,
  providerName,
  model,
  refreshIntervalMs,
}: UsageHeatmapProps) {
  const { t, i18n } = useTranslation();
  const { money } = useMoneyFormat();
  const tooltipRef = useRef<HTMLDivElement>(null);
  const [hover, setHover] = useState<HoverState | null>(null);
  const [breakdownMode, setBreakdownMode] = useState<UsageBreakdown>("app");
  const selectedModelMetric = modelTrendMetric[metric];
  const mode =
    breakdownMode === "model" && !selectedModelMetric ? "total" : breakdownMode;
  // 默认只画最近 53 周，点「显示更多」才把更早的段全部展开；不记住，每次进来都是收起
  const [showEarlier, setShowEarlier] = useState(false);

  const showTooltip = (cell: DayCell, target: HTMLElement) => {
    const rect = target.getBoundingClientRect();
    setHover({
      day: cell.key,
      x: rect.left + rect.width / 2,
      top: rect.top,
      bottom: rect.bottom,
    });
  };
  useLayoutEffect(() => {
    const tooltip = tooltipRef.current;
    if (!hover || !tooltip) return;
    const margin = 8;
    const { width, height } = tooltip.getBoundingClientRect();
    const left = Math.min(
      hover.x - width / 2,
      window.innerWidth - width - margin,
    );
    const above = hover.top - height - margin;
    const below = Math.min(
      hover.bottom + margin,
      window.innerHeight - height - margin,
    );
    tooltip.style.left = `${Math.max(margin, left)}px`;
    tooltip.style.top = `${Math.max(margin, above >= margin ? above : below)}px`;

    const close = () => setHover(null);
    window.addEventListener("scroll", close, true);
    window.addEventListener("resize", close);
    return () => {
      window.removeEventListener("scroll", close, true);
      window.removeEventListener("resize", close);
    };
  }, [hover, metric]);
  const language = i18n.resolvedLanguage || i18n.language || "en";
  const locale = getLocaleFromLanguage(language);

  const { spanCount, trends, isLoading } = useAllSpansDailyTrends(
    { appType, providerName, model },
    refreshIntervalMs,
    mode === "model",
  );

  const heat = useMemo(() => {
    const byDay = new Map<string, DailyStats>();
    for (const stat of trends ?? []) {
      byDay.set(dayKey(new Date(stat.date)), stat);
    }
    const models: HeatModel[] =
      mode === "model" && selectedModelMetric
        ? buildModelTrendSeries(
            Array.from(byDay, ([xKey, { modelStats }]) => ({
              xKey,
              modelStats,
            })),
            selectedModelMetric,
          ).map((series) => ({
            ...series,
            label:
              series.model === null
                ? t("usage.trend.otherModels")
                : series.model || t("usage.trend.unknownModel"),
          }))
        : [];
    const today = startOfDay(new Date());
    // 最新的一段在上
    const spans = Array.from({ length: spanCount }, (_, index) =>
      buildSpan(index, byDay, metric, today, mode, models),
    );
    // 深浅按所有段一起分档，上下几段才能直接比
    const days = spans
      .flatMap((span) => span.weeks.flat())
      .filter((cell) => !cell.future);
    const thresholds = buildThresholds(days.map((cell) => cell.value));

    return { spans, thresholds, models };
  }, [trends, metric, spanCount, mode, selectedModelMetric, t]);

  const earlierCount = heat.spans.length - 1;
  const visibleSpans = showEarlier ? heat.spans : heat.spans.slice(0, 1);
  const visibleApps = Array.from(
    new Set(
      visibleSpans.flatMap((span) =>
        span.weeks.flatMap((week) =>
          week.flatMap((cell) =>
            cell.future
              ? []
              : cell.runtimeStats.map((runtime) => runtime.appType),
          ),
        ),
      ),
    ),
  ).sort();
  const legend =
    mode === "model"
      ? heat.models
      : visibleApps.map((app) => ({
          key: app,
          label: runtimeName(app),
          color: runtimeColor(app),
        }));
  const { format: formatDay } = useMemo(
    () =>
      new Intl.DateTimeFormat(locale, {
        year: "numeric",
        month: "short",
        day: "numeric",
      }),
    [locale],
  );

  const weekdayLabels = useMemo(() => {
    // 2024-01-01 是周一
    return Array.from({ length: 7 }, (_, d) =>
      new Date(2024, 0, 1 + d).toLocaleDateString(locale, {
        weekday: "short",
      }),
    );
  }, [locale]);

  const activeCell =
    !isLoading && hover
      ? visibleSpans
          .flatMap((span) => span.weeks)
          .flat()
          .find((cell) => cell.key === hover.day)
      : undefined;
  const rows =
    mode === "model"
      ? activeCell?.models
      : activeCell?.runtimeStats.map((runtime) => ({
          label: runtimeName(runtime.appType),
          color: runtimeColor(runtime.appType),
          value: trendMetricsOf(runtime)[metric],
        }));
  const breakdown = rows?.map(({ label, color, value }) => ({
    label,
    color,
    value:
      value == null
        ? "—"
        : metric === "cost"
          ? money(value, 4)
          : metric === "hitRate"
            ? formatHitRatePercent(value)
            : formatTokensShort(value, language),
  }));

  return (
    <section
      aria-labelledby="usage-heatmap-title"
      className="relative px-4 pb-3 pt-5"
    >
      <div className="flex flex-wrap items-center justify-between gap-x-4 gap-y-1">
        <h2
          id="usage-heatmap-title"
          className="m-0 text-body font-semibold text-fg-1"
        >
          {t("usage.heatmap.title")}
        </h2>
        <UsageBreakdownControl
          value={mode}
          onValueChange={(next) => {
            setHover(null);
            setBreakdownMode(next);
          }}
          canBreakDownByModel={selectedModelMetric != null}
        />
      </div>
      {!isLoading && legend.length > 0 && (
        <ul className="mb-0 mt-1 flex flex-wrap gap-x-3 gap-y-1 p-0 text-badge text-fg-3">
          {legend.map(({ key, label, color }) => (
            <li key={key} className="flex min-w-0 items-center gap-1.5">
              <span
                aria-hidden="true"
                className="usage-heat-fill h-2.5 w-2.5 shrink-0 rounded-[2px]"
                style={heatFillStyle(4, color)}
              />
              <span className="min-w-0 break-words">{label}</span>
            </li>
          ))}
        </ul>
      )}

      {isLoading ? (
        <div className="flex h-[150px] items-center justify-center">
          <Loader2 className="h-5 w-5 animate-spin text-fg-3" />
        </div>
      ) : (
        visibleSpans.map((span, index) => (
          <div
            key={dayKey(span.start)}
            className={index === 0 ? "mt-3" : "mt-4"}
          >
            <div className="flex flex-wrap items-baseline gap-x-2">
              <h3 className="m-0 text-body font-semibold tabular-nums text-fg-1">
                {`${formatDay(span.start)} – ${formatDay(span.end)}`}
              </h3>
              <span className="text-caption tabular-nums text-fg-3">
                {t("usage.heatmap.spanSummary", {
                  tokens: formatTokensShort(span.totals.tokens, language),
                  requests: formatTokensShort(span.totals.requests, language),
                  cost: money(span.totals.cost, 2),
                })}
              </span>
            </div>
            <div
              className="mt-1.5 grid items-center gap-[3px]"
              style={{
                gridTemplateColumns: `max-content repeat(${WEEKS}, minmax(0, 1fr))`,
              }}
            >
              <span />
              {span.weeks.map((_, w) => (
                <span
                  key={`m${w}`}
                  className="h-4 overflow-visible whitespace-nowrap text-badge leading-4 text-fg-3"
                >
                  {span.monthLabels[w] != null
                    ? new Date(
                        2024,
                        span.monthLabels[w]! - 1,
                        1,
                      ).toLocaleDateString(locale, { month: "short" })
                    : ""}
                </span>
              ))}
              {weekdayLabels.map((label, d) => (
                <Fragment key={`r${d}`}>
                  <span className="pe-1.5 text-badge leading-none text-fg-3">
                    {d % 2 === 0 ? label : ""}
                  </span>
                  {span.weeks.map((column) => {
                    const cell = column[d];
                    const level = levelOf(cell.value, heat.thresholds);
                    const labels =
                      mode === "model"
                        ? cell.models.map(({ label }) => label)
                        : cell.runtimeStats.map(({ appType }) =>
                            runtimeName(appType),
                          );
                    const hasUsage =
                      !cell.future && (level > 0 || labels.length > 0);
                    return (
                      <div
                        key={cell.key}
                        role={hasUsage ? "img" : undefined}
                        aria-label={
                          hasUsage
                            ? [formatDay(cell.date), ...labels].join(" · ")
                            : undefined
                        }
                        onMouseEnter={
                          cell.future
                            ? undefined
                            : (event) => showTooltip(cell, event.currentTarget)
                        }
                        onMouseLeave={() => setHover(null)}
                        className={cn(
                          "flex aspect-square w-full overflow-hidden rounded-[3px]",
                          !cell.future && level === 0 && "bg-subtle",
                        )}
                      >
                        {!cell.future && level > 0 && (
                          <span
                            aria-hidden="true"
                            className="usage-heat-fill flex-1"
                            style={heatFillStyle(level, cell.color)}
                          />
                        )}
                      </div>
                    );
                  })}
                </Fragment>
              ))}
            </div>
          </div>
        ))
      )}

      {hover &&
        activeCell &&
        createPortal(
          <div
            ref={tooltipRef}
            role="tooltip"
            className="pointer-events-none fixed z-20"
          >
            <UsageTooltipCard
              heading={activeCell.date.toLocaleDateString(locale, {
                year: "numeric",
                month: "long",
                day: "numeric",
                weekday: "short",
              })}
              tokens={activeCell.tokens}
              requests={activeCell.requests}
              cost={activeCell.cost}
              hitRate={metric === "hitRate" ? activeCell.hitRate : undefined}
              breakdown={breakdown}
            />
          </div>,
          document.body,
        )}

      <div className="mt-2 flex items-center gap-1.5 text-badge text-fg-3">
        {!isLoading && earlierCount > 0 && (
          <Button
            type="button"
            variant="quiet"
            size="compact"
            aria-expanded={showEarlier}
            onClick={() => setShowEarlier((open) => !open)}
            className="-ms-2 gap-1 pe-2 ps-2 text-caption text-fg-2"
          >
            {showEarlier
              ? t("usage.heatmap.showLess")
              : t("usage.heatmap.showMore")}
            <ChevronDown
              className={cn(
                "h-3.5 w-3.5 transition-transform duration-150",
                showEarlier && "rotate-180",
              )}
            />
          </Button>
        )}
        <div className="flex-1" />
        {t("usage.heatmap.less")}
        {LEVELS.map((level) => (
          <span
            key={level}
            className={cn(
              "h-2.5 w-2.5 rounded-[2px]",
              level === 0 ? "bg-subtle" : "usage-heat-fill",
            )}
            aria-hidden="true"
            style={
              level === 0 ? undefined : heatFillStyle(level, "var(--text-2)")
            }
          />
        ))}
        {t("usage.heatmap.more")}
      </div>
    </section>
  );
}
