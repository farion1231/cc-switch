import { Fragment, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ChevronDown, Loader2, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Notice } from "@/components/ui/notice";
import { Switch } from "@/components/ui/switch";
import { useUsageFirstDate, useUsageTrends } from "@/lib/query/usage";
import { cn } from "@/lib/utils";
import type { UsageRangeSelection } from "@/types/usage";
import {
  fmtInt,
  formatHitRatePercent,
  formatTokensCompact,
  formatTokensShort,
  getLocaleFromLanguage,
  parseFiniteNumber,
} from "./format";
import {
  UsageTrendChart,
  hitRateOf,
  type UsageTrendStatLike,
} from "./UsageTrendChart";
import { UsageTooltipCard } from "./UsageTooltipCard";
import { useMoneyFormat } from "@/hooks/useMoneyFormat";
import type { UsageMetric } from "./rememberedChoice";

const WEEKS = 53;
const DAY_MS = 24 * 60 * 60 * 1000;
/** 后端 `rollup_and_prune(30)` 会在 30 天后合并原始请求明细并仅保留每日汇总 */
export const HEATMAP_RAW_RETENTION_DAYS = 30;
/** 0 档用底色，1–4 档是 --heat-1…4（与趋势图同一色相，色阶在 index.css 按深浅色分别定） */
const LEVELS = [0, 1, 2, 3, 4];

interface UsageHeatmapProps {
  /** 格子深浅按哪个指标分档：由上方指标条选 */
  metric: UsageMetric;
  appType?: string;
  providerName?: string;
  model?: string;
  refreshIntervalMs: number;
  /** 可选：把热力图当前选中的单日或连续区间同步应用到主仪表盘筛选器 */
  onApplyRange?: (selection: UsageRangeSelection) => void;
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

function metricValue(
  stat: UsageTrendStatLike,
  metric: Exclude<UsageMetric, "hitRate">,
): number {
  if (metric === "requests") return stat.requestCount ?? 0;
  if (metric === "cost") return parseFiniteNumber(stat.totalCost) ?? 0;
  return (
    stat.totalInputTokens +
    stat.totalOutputTokens +
    stat.totalCacheCreationTokens +
    stat.totalCacheReadTokens
  );
}

/** 热力图选中的连续本地日期闭区间 `[start, end]`（均为本地零点） */
export interface HeatmapDayRange {
  start: Date;
  end: Date;
}

export interface HeatmapRangeSummary {
  days: number;
  activeDays: number;
  tokens: number;
  requests: number;
  cost: number;
  hitRate: number | null;
}

/** 把任意顺序的两个日期归一化为闭区间 `[较早, 较晚]`（本地日历零点） */
export function normalizeHeatmapRange(a: Date, b: Date): HeatmapDayRange {
  const start = startOfDay(a);
  const end = startOfDay(b);
  return start.getTime() <= end.getTime()
    ? { start, end }
    : { start: end, end: start };
}

export function isDayInHeatmapRange(
  date: Date,
  range: HeatmapDayRange | null,
): boolean {
  if (!range) return false;
  const target = startOfDay(date).getTime();
  return (
    target >= startOfDay(range.start).getTime() &&
    target <= startOfDay(range.end).getTime()
  );
}

export function isHeatmapRangeEndpoint(
  date: Date,
  range: HeatmapDayRange | null,
): boolean {
  if (!range) return false;
  const target = startOfDay(date).getTime();
  return (
    target === startOfDay(range.start).getTime() ||
    target === startOfDay(range.end).getTime()
  );
}

/**
 * 转为后端 `get_usage_trends` / 主筛选器识别的闭区间秒级时间戳：
 * 起点必须严格对齐本地 00:00:00、终点对齐 23:59:59，后端 `compute_rollup_date_bounds`
 * 才会将超过 30 天的每日汇总行（usage_daily_rollups）纳入统计。
 */
export function buildHeatmapUsageSelection(
  range: HeatmapDayRange,
): UsageRangeSelection {
  const { start, end } = normalizeHeatmapRange(range.start, range.end);
  const customStartDate = Math.floor(
    new Date(
      start.getFullYear(),
      start.getMonth(),
      start.getDate(),
      0,
      0,
      0,
    ).getTime() / 1000,
  );
  const customEndDate = Math.floor(
    new Date(
      end.getFullYear(),
      end.getMonth(),
      end.getDate(),
      23,
      59,
      59,
    ).getTime() / 1000,
  );
  return {
    preset: "custom",
    customStartDate,
    customEndDate,
    liveEndTime: false,
  };
}

/**
 * 判断选中的单日是否已跨过 30 天原始明细保留期。
 * 单日区间时长 <= 24h，后端走按小时分支且仅查 proxy_request_logs；30 天及更早的日期
 * 原始请求已被清理并归档至每日汇总，需要提示用户改看汇总或开启多日趋势。
 */
export function isSingleDayArchived(
  range: HeatmapDayRange,
  today: Date = startOfDay(new Date()),
): boolean {
  const normalized = normalizeHeatmapRange(range.start, range.end);
  if (daysBetween(normalized.start, normalized.end) !== 0) return false;
  return (
    daysBetween(normalized.start, startOfDay(today)) >=
    HEATMAP_RAW_RETENTION_DAYS
  );
}

/** 按热力图已加载的按天趋势汇总选中区间的核心指标，与卡片上方 KPI 和工具提示同口径 */
export function summarizeHeatmapRange(
  range: HeatmapDayRange,
  byDay: ReadonlyMap<string, UsageTrendStatLike>,
  today: Date = startOfDay(new Date()),
): HeatmapRangeSummary {
  const normalized = normalizeHeatmapRange(range.start, range.end);
  const normalizedToday = startOfDay(today);
  const limit =
    normalized.end.getTime() <= normalizedToday.getTime()
      ? normalized.end
      : normalizedToday;

  let days = 0;
  let activeDays = 0;
  let tokens = 0;
  let requests = 0;
  let cost = 0;
  let cacheRead = 0;
  let cacheable = 0;

  for (
    let cursor = normalized.start;
    cursor.getTime() <= limit.getTime();
    cursor = addDays(cursor, 1)
  ) {
    days += 1;
    const stat = byDay.get(dayKey(cursor));
    if (!stat) continue;
    const dayTokens = metricValue(stat, "tokens");
    const dayRequests = stat.requestCount ?? 0;
    tokens += dayTokens;
    requests += dayRequests;
    cost += metricValue(stat, "cost");
    cacheRead += stat.totalCacheReadTokens;
    cacheable +=
      stat.totalInputTokens +
      stat.totalCacheCreationTokens +
      stat.totalCacheReadTokens;
    if (dayRequests > 0 || dayTokens > 0) {
      activeDays += 1;
    }
  }

  return {
    days: Math.max(days, 1),
    activeDays,
    tokens,
    requests,
    cost,
    hitRate: cacheable > 0 ? cacheRead / cacheable : null,
  };
}

/** 格子分档用的值：用量类指标 0 就是没用；命中率 0% 是真实数据，只有没有输入时才是空 */
function heatValue(
  stat: UsageTrendStatLike | undefined,
  metric: UsageMetric,
): number | null {
  if (!stat) return null;
  if (metric === "hitRate") return hitRateOf(stat);
  const value = metricValue(stat, metric);
  return value > 0 ? value : null;
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
  metric: UsageMetric,
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
        value: heatValue(stat, metric),
        hitRate: stat ? hitRateOf(stat) : null,
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

  return { start, end, weeks, monthLabels: monthLabelsOf(weeks), totals };
}

/** 第一列的月份标签离下一个月份标签至少要隔这么多列，否则两个标签会叠在一起 */
const FIRST_LABEL_MIN_COLUMNS = 3;

/**
 * 月份标签：某一列包含当月 1 号时标出月份。第一列也标它所在的月份，
 * 但那个月在开头只剩一两列时不标，否则会和紧跟着的下个月标签重叠（#9917）。
 */
export function monthLabelsOf(weeks: { date: Date }[][]): (number | null)[] {
  const labels = weeks.map((column) => {
    const first = column.find((cell) => cell.date.getDate() === 1);
    return first ? first.date.getMonth() + 1 : null;
  });
  if (labels.length > 0 && labels[0] == null) {
    const next = labels.findIndex((label) => label != null);
    if (next === -1 || next >= FIRST_LABEL_MIN_COLUMNS) {
      labels[0] = weeks[0][0].date.getMonth() + 1;
    }
  }
  return labels;
}

export function UsageHeatmap({
  metric,
  appType,
  providerName,
  model,
  refreshIntervalMs,
  onApplyRange,
}: UsageHeatmapProps) {
  const { t, i18n } = useTranslation();
  const { money } = useMoneyFormat();
  const sectionRef = useRef<HTMLElement>(null);
  const [hover, setHover] = useState<HoverState | null>(null);
  // 默认只画最近 53 周，点「显示更多」才把更早的段全部展开；不记住，每次进来都是收起
  const [showEarlier, setShowEarlier] = useState(false);
  // 行内详情与连续区间多选：默认点击单日行内展开，开启开关后可连续点选起止日期
  const [multiSelect, setMultiSelect] = useState(false);
  const [rangeAnchor, setRangeAnchor] = useState<Date | null>(null);
  const [selectedRange, setSelectedRange] = useState<HeatmapDayRange | null>(
    null,
  );
  // 收起抽屉时短暂保留上一次内容直到 CSS 高度动画（1fr -> 0fr）完成，避免文字/图表瞬间跳没
  const [closingRange, setClosingRange] = useState<HeatmapDayRange | null>(
    null,
  );

  useEffect(() => {
    if (selectedRange != null || closingRange == null) return;
    const timer = window.setTimeout(() => setClosingRange(null), 240);
    return () => window.clearTimeout(timer);
  }, [selectedRange, closingRange]);

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

    return { spans, thresholds, byDay, today };
  }, [trends, metric, spanCount]);

  const shortDayFormatter = useMemo(
    () =>
      new Intl.DateTimeFormat(locale, {
        year: "numeric",
        month: "short",
        day: "numeric",
      }),
    [locale],
  );
  const fullDayFormatter = useMemo(
    () =>
      new Intl.DateTimeFormat(locale, {
        year: "numeric",
        month: "long",
        day: "numeric",
        weekday: "short",
      }),
    [locale],
  );
  const monthFormatter = useMemo(
    () => new Intl.DateTimeFormat(locale, { month: "short" }),
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

  const clearSelection = () => {
    if (selectedRange) {
      setClosingRange(selectedRange);
    }
    setSelectedRange(null);
    setRangeAnchor(null);
  };

  const handleMultiSelectChange = (checked: boolean) => {
    setMultiSelect(checked);
    if (checked) {
      // 如果当前已展开某一天，切换为多选时自动把它延续为区间起点，用户再点一天就能完成区间
      setRangeAnchor(
        selectedRange &&
          daysBetween(selectedRange.start, selectedRange.end) === 0
          ? selectedRange.start
          : null,
      );
      return;
    }
    setRangeAnchor(null);
  };

  const handleDayClick = (rawDate: Date) => {
    const clicked = startOfDay(rawDate);
    setClosingRange(null);
    if (!multiSelect) {
      if (
        selectedRange &&
        daysBetween(selectedRange.start, selectedRange.end) === 0 &&
        dayKey(selectedRange.start) === dayKey(clicked)
      ) {
        clearSelection();
        return;
      }
      setRangeAnchor(null);
      setSelectedRange({ start: clicked, end: clicked });
      return;
    }

    if (!rangeAnchor) {
      setRangeAnchor(clicked);
      setSelectedRange({ start: clicked, end: clicked });
      return;
    }

    if (dayKey(clicked) === dayKey(rangeAnchor)) {
      clearSelection();
      return;
    }

    setSelectedRange(normalizeHeatmapRange(rangeAnchor, clicked));
    setRangeAnchor(null);
  };

  const earlierCount = heat.spans.length - 1;
  const visibleSpans = showEarlier ? heat.spans : heat.spans.slice(0, 1);

  const isDrawerOpen = selectedRange != null;
  const displayedRange = selectedRange ?? closingRange;
  const previewRange =
    multiSelect && rangeAnchor && hover && !hover.cell.future
      ? normalizeHeatmapRange(rangeAnchor, hover.cell.date)
      : null;

  const drawerSelection = useMemo(
    () => (displayedRange ? buildHeatmapUsageSelection(displayedRange) : null),
    [displayedRange],
  );
  const drawerSummary = useMemo(
    () =>
      displayedRange
        ? summarizeHeatmapRange(displayedRange, heat.byDay, heat.today)
        : null,
    [displayedRange, heat.byDay, heat.today],
  );
  const drawerArchivedSingleDay = displayedRange
    ? isSingleDayArchived(displayedRange, heat.today)
    : false;
  const drawerHeading =
    displayedRange && drawerSummary
      ? drawerSummary.days === 1
        ? fullDayFormatter.format(displayedRange.start)
        : `${shortDayFormatter.format(displayedRange.start)} – ${shortDayFormatter.format(displayedRange.end)}`
      : "";
  const drawerCostDigits =
    drawerSummary && drawerSummary.cost > 0 && drawerSummary.cost < 0.01
      ? 4
      : 2;

  return (
    <section
      ref={sectionRef}
      aria-labelledby="usage-heatmap-title"
      onKeyDown={(event) => {
        if (event.key === "Escape" && selectedRange != null) {
          event.preventDefault();
          event.stopPropagation();
          clearSelection();
        }
      }}
      className="relative px-4 pb-3 pt-5"
    >
      <div className="flex flex-wrap items-start justify-between gap-2">
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
        <label className="inline-flex cursor-pointer select-none items-center gap-2 text-caption text-fg-2">
          <span>{t("usage.heatmap.rangeSelect")}</span>
          <Switch
            size="sm"
            checked={multiSelect}
            onCheckedChange={handleMultiSelectChange}
            aria-label={t("usage.heatmap.rangeSelect")}
          />
        </label>
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
                {`${shortDayFormatter.format(span.start)} – ${shortDayFormatter.format(span.end)}`}
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
                    ? monthFormatter.format(
                        new Date(2024, span.monthLabels[w]! - 1, 1),
                      )
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
                    if (cell.future) {
                      return (
                        <div
                          key={cell.key}
                          aria-hidden="true"
                          className="aspect-square w-full rounded-[3px]"
                        />
                      );
                    }
                    const level = levelOf(cell.value, heat.thresholds);
                    const isSelected = isDayInHeatmapRange(
                      cell.date,
                      selectedRange,
                    );
                    const isEndpoint = isHeatmapRangeEndpoint(
                      cell.date,
                      selectedRange,
                    );
                    const isPreview =
                      previewRange != null &&
                      isDayInHeatmapRange(cell.date, previewRange) &&
                      !isSelected;
                    const cellCostDigits =
                      cell.cost > 0 && cell.cost < 0.01 ? 4 : 2;
                    return (
                      <button
                        key={cell.key}
                        type="button"
                        data-day-key={cell.key}
                        aria-pressed={isSelected}
                        aria-label={t("usage.heatmap.cellAriaSingle", {
                          date: fullDayFormatter.format(cell.date),
                          tokens: formatTokensShort(cell.tokens, language),
                          requests: fmtInt(cell.requests, locale),
                          cost: money(cell.cost, cellCostDigits),
                        })}
                        onClick={() => handleDayClick(cell.date)}
                        onMouseEnter={(event) =>
                          showTooltip(cell, event.currentTarget)
                        }
                        onMouseLeave={() => setHover(null)}
                        onFocus={(event) =>
                          showTooltip(cell, event.currentTarget)
                        }
                        onBlur={() => setHover(null)}
                        className={cn(
                          "relative block aspect-square w-full cursor-pointer appearance-none rounded-[3px] border-0 p-0 transition-transform duration-100 hover:scale-110 focus-visible:z-10 motion-reduce:transition-none",
                          level === 0 && "bg-subtle",
                          isEndpoint &&
                            "z-10 after:pointer-events-none after:absolute after:-inset-[1px] after:rounded-[4px] after:border-2 after:border-action",
                          isSelected &&
                            !isEndpoint &&
                            "after:pointer-events-none after:absolute after:inset-0 after:rounded-[3px] after:border after:border-action",
                          isPreview &&
                            "after:pointer-events-none after:absolute after:inset-0 after:rounded-[3px] after:border after:border-dashed after:border-action",
                        )}
                        style={cellStyle(level)}
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
            heading={fullDayFormatter.format(hover.cell.date)}
            tokens={hover.cell.tokens}
            requests={hover.cell.requests}
            cost={hover.cell.cost}
            hitRate={metric === "hitRate" ? hover.cell.hitRate : undefined}
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

      {/* 行内折叠抽屉：复用 grid-template-rows (0fr ↔ 1fr) 动画在热力图原地展开单日或连续区间详情 */}
      <div
        data-testid="usage-heatmap-drawer"
        data-state={isDrawerOpen ? "open" : "closed"}
        aria-hidden={!isDrawerOpen}
        className={cn(
          "grid transition-[grid-template-rows] duration-200 ease-out motion-reduce:transition-none",
          isDrawerOpen ? "grid-rows-[1fr]" : "grid-rows-[0fr]",
        )}
        onTransitionEnd={(event) => {
          if (event.target === event.currentTarget && !isDrawerOpen) {
            setClosingRange(null);
          }
        }}
        {...(isDrawerOpen ? {} : { inert: "" })}
      >
        <div className="min-h-0 overflow-hidden">
          {displayedRange && drawerSelection && drawerSummary && (
            <div className="mt-3 border-t border-border pt-3">
              <div className="flex flex-wrap items-center justify-between gap-2">
                <div className="flex min-w-0 flex-wrap items-center gap-2">
                  <h3 className="m-0 text-body font-semibold tabular-nums text-fg-1">
                    {drawerHeading}
                  </h3>
                  {drawerSummary.days > 1 && (
                    <span className="rounded-control bg-subtle px-2 py-0.5 text-badge font-medium tabular-nums text-fg-2">
                      {t("usage.heatmap.dayCount", {
                        count: drawerSummary.days,
                      })}
                    </span>
                  )}
                  {multiSelect && rangeAnchor && (
                    <span className="rounded-control bg-action-soft px-2 py-0.5 text-badge font-medium text-action-text">
                      {t("usage.heatmap.pickRangeEnd")}
                    </span>
                  )}
                </div>
                <div className="flex shrink-0 items-center gap-1.5">
                  {onApplyRange && (
                    <Button
                      type="button"
                      variant="neutral"
                      size="compact"
                      onClick={() => onApplyRange(drawerSelection)}
                    >
                      {t("usage.heatmap.applyRange")}
                    </Button>
                  )}
                  <Button
                    type="button"
                    variant="quiet"
                    size="icon-compact"
                    aria-label={t("usage.heatmap.closeDetail")}
                    onClick={clearSelection}
                  >
                    <X className="h-3.5 w-3.5" />
                  </Button>
                </div>
              </div>

              <dl className="mt-2.5 grid grid-cols-2 gap-2 rounded-control bg-subtle px-3 py-2 sm:grid-cols-4">
                <div className="min-w-0">
                  <dt className="text-badge text-fg-3">
                    {t("usage.trend.tokens")}
                  </dt>
                  <dd
                    className={cn(
                      "m-0 truncate text-body font-semibold tabular-nums",
                      metric === "tokens" ? "text-action-text" : "text-fg-1",
                    )}
                  >
                    {formatTokensCompact(drawerSummary.tokens, locale)}
                  </dd>
                </div>
                <div className="min-w-0">
                  <dt className="text-badge text-fg-3">
                    {t("usage.trend.requestsLegend")}
                  </dt>
                  <dd
                    className={cn(
                      "m-0 truncate text-body font-semibold tabular-nums",
                      metric === "requests" ? "text-action-text" : "text-fg-1",
                    )}
                  >
                    {fmtInt(drawerSummary.requests, locale)}
                  </dd>
                </div>
                <div className="min-w-0">
                  <dt className="text-badge text-fg-3">
                    {t("usage.trend.cost")}
                  </dt>
                  <dd
                    className={cn(
                      "m-0 truncate text-body font-semibold tabular-nums",
                      metric === "cost" ? "text-action-text" : "text-fg-1",
                    )}
                  >
                    {money(drawerSummary.cost, drawerCostDigits)}
                  </dd>
                </div>
                <div className="min-w-0">
                  <dt className="text-badge text-fg-3">
                    {t("usage.cacheHitRate")}
                  </dt>
                  <dd
                    className={cn(
                      "m-0 truncate text-body font-semibold tabular-nums",
                      metric === "hitRate" ? "text-action-text" : "text-fg-1",
                    )}
                  >
                    {drawerSummary.hitRate == null
                      ? "—"
                      : formatHitRatePercent(drawerSummary.hitRate)}
                  </dd>
                </div>
              </dl>

              {drawerArchivedSingleDay ? (
                <Notice
                  tone="neutral"
                  title={t("usage.heatmap.archivedTitle")}
                  className="mt-2.5"
                >
                  {t("usage.heatmap.archivedBody", {
                    days: HEATMAP_RAW_RETENTION_DAYS,
                  })}
                </Notice>
              ) : (
                <UsageTrendChart
                  embedded
                  range={drawerSelection}
                  metric={metric}
                  appType={appType}
                  providerName={providerName}
                  model={model}
                  refreshIntervalMs={refreshIntervalMs}
                />
              )}
            </div>
          )}
        </div>
      </div>
    </section>
  );
}
