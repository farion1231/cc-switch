import { Fragment, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ChevronDown, Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { SegmentedControl } from "@/components/ui/segmented-control";
import { useUsageFirstDate, useUsageTrends } from "@/lib/query/usage";
import { cn } from "@/lib/utils";
import type { UsageRangeSelection } from "@/types/usage";
import {
  fmtUsd,
  formatTokensShort,
  getLocaleFromLanguage,
  parseFiniteNumber,
} from "./format";
import type { UsageTrendStatLike } from "./UsageTrendChart";
import { UsageTooltipCard } from "./UsageTooltipCard";

const WEEKS = 53;
const DAY_MS = 24 * 60 * 60 * 1000;
/** 0 档用底色，1–4 档是 --heat-1…4（与趋势图同一色相，色阶在 index.css 按深浅色分别定） */
const LEVELS = [0, 1, 2, 3, 4];

type HeatMetric = "tokens" | "requests" | "cost";

interface UsageHeatmapProps {
  appType?: string;
  providerName?: string;
  model?: string;
  refreshIntervalMs: number;
}

interface DayCell {
  key: string;
  date: Date;
  value: number;
  tokens: number;
  requests: number;
  cost: number;
  future: boolean;
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
  cell: DayCell;
  /** 相对热力图卡片的格子顶部中点 */
  x: number;
  y: number;
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

function metricValue(stat: UsageTrendStatLike, metric: HeatMetric): number {
  if (metric === "requests") return stat.requestCount ?? 0;
  if (metric === "cost") return parseFiniteNumber(stat.totalCost) ?? 0;
  return (
    stat.totalInputTokens +
    stat.totalOutputTokens +
    stat.totalCacheCreationTokens +
    stat.totalCacheReadTokens
  );
}

/** 按非零天的四分位分档，避免个别高峰日把其余格子都压成最浅 */
function buildThresholds(values: number[]): number[] {
  const sorted = values.filter((v) => v > 0).sort((a, b) => a - b);
  if (sorted.length === 0) return [Infinity, Infinity, Infinity];
  const at = (q: number) =>
    sorted[Math.min(sorted.length - 1, Math.floor(sorted.length * q))];
  return [at(0.25), at(0.5), at(0.75)];
}

function levelOf(value: number, thresholds: number[]): number {
  if (value <= 0) return 0;
  if (value <= thresholds[0]) return 1;
  if (value <= thresholds[1]) return 2;
  if (value <= thresholds[2]) return 3;
  return 4;
}

function cellStyle(level: number) {
  return level === 0
    ? undefined
    : {
        backgroundColor: `var(--heat-${level})`,
      };
}

/**
 * 从最早一段的起点到现在的按天数据。先查最早日期算出要几段，再查趋势；
 * 起点按周取整，同一周内查询键不变。
 */
function useAllSpansDailyTrends(
  filters: { appType?: string; providerName?: string; model?: string },
  refreshIntervalMs: number,
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
    refetchInterval,
  });
  return {
    spanCount: spanCount ?? 1,
    trends: trendsQuery.data,
    isLoading: firstDateQuery.isPending || trendsQuery.isLoading,
  };
}

/** 把后端按天的数据铺成一段 53 周 × 7 天的格子（周一开头，未来的日子标记出来）。 */
function buildSpan(
  index: number,
  byDay: Map<string, UsageTrendStatLike>,
  metric: HeatMetric,
  today: Date,
): SpanGrid {
  const start = spanStart(today, index);
  const end = index === 0 ? today : addDays(spanStart(today, index - 1), -1);
  const weeks: DayCell[][] = [];
  const totals = { tokens: 0, requests: 0, cost: 0 };
  for (let w = 0; w < WEEKS; w++) {
    const column: DayCell[] = [];
    for (let d = 0; d < 7; d++) {
      const date = addDays(start, w * 7 + d);
      const stat = byDay.get(dayKey(date));
      const cell: DayCell = {
        key: dayKey(date),
        date,
        value: stat ? metricValue(stat, metric) : 0,
        tokens: stat ? metricValue(stat, "tokens") : 0,
        requests: stat?.requestCount ?? 0,
        cost: stat ? metricValue(stat, "cost") : 0,
        future: date.getTime() > today.getTime(),
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
  appType,
  providerName,
  model,
  refreshIntervalMs,
}: UsageHeatmapProps) {
  const { t, i18n } = useTranslation();
  const [metric, setMetric] = useState<HeatMetric>("tokens");
  const sectionRef = useRef<HTMLElement>(null);
  const [hover, setHover] = useState<HoverState | null>(null);
  // 默认只画最近 53 周，点「显示更多」才把更早的段全部展开；不记住，每次进来都是收起
  const [showEarlier, setShowEarlier] = useState(false);

  const showTooltip = (cell: DayCell, target: HTMLElement) => {
    const section = sectionRef.current;
    if (!section) return;
    const box = section.getBoundingClientRect();
    const rect = target.getBoundingClientRect();
    setHover({
      cell,
      x: rect.left + rect.width / 2 - box.left,
      y: rect.top - box.top,
    });
  };
  const language = i18n.resolvedLanguage || i18n.language || "en";
  const locale = getLocaleFromLanguage(language);

  const { spanCount, trends, isLoading } = useAllSpansDailyTrends(
    { appType, providerName, model },
    refreshIntervalMs,
  );

  const heat = useMemo(() => {
    const byDay = new Map<string, UsageTrendStatLike>();
    for (const stat of trends ?? []) {
      byDay.set(dayKey(new Date(stat.date)), stat);
    }
    const today = startOfDay(new Date());
    // 最新的一段在上
    const spans = Array.from({ length: spanCount }, (_, index) =>
      buildSpan(index, byDay, metric, today),
    );
    // 深浅按所有段一起分档，上下几段才能直接比
    const days = spans
      .flatMap((span) => span.weeks.flat())
      .filter((cell) => !cell.future);
    const thresholds = buildThresholds(days.map((cell) => cell.value));

    return { spans, thresholds };
  }, [trends, metric, spanCount]);

  const earlierCount = heat.spans.length - 1;
  const visibleSpans = showEarlier ? heat.spans : heat.spans.slice(0, 1);
  const formatDay = (date: Date) =>
    date.toLocaleDateString(locale, {
      year: "numeric",
      month: "short",
      day: "numeric",
    });

  const weekdayLabels = useMemo(() => {
    // 2024-01-01 是周一
    return Array.from({ length: 7 }, (_, d) =>
      new Date(2024, 0, 1 + d).toLocaleDateString(locale, {
        weekday: "short",
      }),
    );
  }, [locale]);

  return (
    <section
      ref={sectionRef}
      aria-labelledby="usage-heatmap-title"
      className="relative shrink-0 rounded-panel border border-border bg-surface px-4 py-3"
    >
      <div className="flex flex-wrap items-start gap-x-3.5 gap-y-1">
        <div>
          <h2
            id="usage-heatmap-title"
            className="m-0 text-body font-semibold text-fg-1"
          >
            {t("usage.heatmap.title")}
          </h2>
          <p className="m-0 text-caption text-fg-3">
            {t("usage.heatmap.subtitle")}
          </p>
        </div>
        <div className="flex-1" />
        <SegmentedControl<HeatMetric>
          size="sm"
          aria-label={t("usage.trend.metricLabel")}
          value={metric}
          onValueChange={setMetric}
          items={[
            { value: "tokens", label: t("usage.trend.tokens") },
            { value: "requests", label: t("usage.trend.requests") },
            { value: "cost", label: t("usage.trend.cost") },
          ]}
        />
      </div>

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
                  cost: fmtUsd(span.totals.cost, 2),
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
                    return (
                      <div
                        key={cell.key}
                        onMouseEnter={
                          cell.future
                            ? undefined
                            : (event) => showTooltip(cell, event.currentTarget)
                        }
                        onMouseLeave={() => setHover(null)}
                        className={cn(
                          "aspect-square w-full rounded-[3px]",
                          !cell.future && level === 0 && "bg-subtle",
                        )}
                        style={cell.future ? undefined : cellStyle(level)}
                      />
                    );
                  })}
                </Fragment>
              ))}
            </div>
          </div>
        ))
      )}

      {hover && (
        <div
          role="tooltip"
          className="pointer-events-none absolute z-20"
          style={{
            left: hover.x,
            top: hover.y - 8,
            // 靠近左右边缘时往里收，别被卡片裁掉
            transform: `translate(${
              hover.x < 110
                ? "-15%"
                : hover.x > (sectionRef.current?.clientWidth ?? 0) - 110
                  ? "-85%"
                  : "-50%"
            }, -100%)`,
          }}
        >
          <UsageTooltipCard
            heading={hover.cell.date.toLocaleDateString(locale, {
              year: "numeric",
              month: "long",
              day: "numeric",
              weekday: "short",
            })}
            tokens={hover.cell.tokens}
            requests={hover.cell.requests}
            cost={hover.cell.cost}
          />
        </div>
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
              level === 0 && "bg-subtle",
            )}
            style={cellStyle(level)}
          />
        ))}
        {t("usage.heatmap.more")}
      </div>
    </section>
  );
}
