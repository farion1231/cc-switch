import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Bar,
  BarChart,
  CartesianGrid,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";
import { BarChart3, Loader2 } from "lucide-react";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useUsageTrendSeries } from "@/lib/query/usage";
import { resolveUsageRange } from "@/lib/usageRange";
import {
  fmtInt,
  fmtUsd,
  getLocaleFromLanguage,
  parseFiniteNumber,
} from "./format";
import type {
  ResolvedTrendGranularity,
  TrendGranularityOption,
  TrendGroupBy,
  TrendMetric,
  UsageRangeSelection,
  UsageTrendSeriesPoint,
  UsageTrendSeriesResponse,
} from "@/types/usage";

/** 未进 Top-N 的系列聚合到该 key 下展示。 */
export const OTHER_SERIES_KEY = "__other__";

/** 与后端 TREND_MAX_BUCKETS 护栏一致：超出即禁用档位。 */
export const TREND_BUCKET_LIMIT = 1000;

/** 模型/Provider 维度的 Top-N 展示上限。 */
export const TREND_TOP_N = 8;

/** token_type 维度的四个固定系列（顺序即堆叠顺序）。 */
export const TOKEN_TYPE_SERIES = [
  "input",
  "output",
  "cacheCreation",
  "cacheRead",
] as const;

/** token_type 沿用现有趋势面积图四色。 */
const TOKEN_TYPE_COLORS: Record<string, string> = {
  input: "#3b82f6",
  output: "#22c55e",
  cacheCreation: "#f97316",
  cacheRead: "#a855f7",
};

/** 扩展色板：按系列名稳定映射（同名同色，不随排名漂移）。 */
export const SERIES_PALETTE = [
  "#2563eb",
  "#16a34a",
  "#ea580c",
  "#9333ea",
  "#dc2626",
  "#0d9488",
  "#ca8a04",
  "#4f46e5",
  "#db2777",
  "#65a30d",
  "#0891b2",
  "#7c3aed",
] as const;

const MINUTE_AND_HOUR_GRANULARITIES = new Set([
  "1min",
  "5min",
  "15min",
  "30min",
  "hour",
]);

/** djb2 哈希 → 色板下标。 */
export function seriesColorIndex(name: string): number {
  let hash = 5381;
  for (let i = 0; i < name.length; i += 1) {
    hash = ((hash << 5) + hash + name.charCodeAt(i)) >>> 0;
  }
  return hash % SERIES_PALETTE.length;
}

export function seriesColor(key: string, groupBy: TrendGroupBy): string {
  if (groupBy === "token_type") {
    return TOKEN_TYPE_COLORS[key] ?? "#94a3b8";
  }
  if (key === OTHER_SERIES_KEY) {
    return "#94a3b8";
  }
  return SERIES_PALETTE[seriesColorIndex(key)];
}

/** 镜像后端 estimate_trend_bucket_count：前端档位禁用预算，允许 ±1 误差。 */
export function estimateTrendBucketCount(
  startDate: number,
  endDate: number,
  granularity: TrendGranularityOption,
): number {
  const duration = Math.max(endDate - startDate, 0);
  switch (granularity) {
    case "1min":
      return Math.floor(duration / 60) + 1;
    case "5min":
      return Math.floor(duration / 300) + 1;
    case "15min":
      return Math.floor(duration / 900) + 1;
    case "30min":
      return Math.floor(duration / 1800) + 1;
    case "hour":
      return Math.floor(duration / 3600) + 1;
    case "day":
      return Math.floor(duration / 86400) + 1;
    case "week":
      return Math.floor(duration / 604800) + 1;
    case "month":
      return Math.floor(duration / 2592000) + 1;
    case "auto":
      return 0;
  }
}

/** 与后端 DETAIL_RETENTION_DAYS 一致：子天档只读明细表，越界老数据只剩日汇总。 */
export const TREND_DETAIL_RETENTION_SECONDS = 30 * 86_400;

/** 该档位在当前范围下是否可选择（auto 恒可选，后端负责解析+护栏）。 */
export function isGranularitySelectable(
  startDate: number,
  endDate: number,
  granularity: TrendGranularityOption,
  nowTs: number = Math.floor(Date.now() / 1000),
): boolean {
  if (granularity === "auto") return true;
  // 子天档位要求范围起点落在明细保留期内（镜像后端保留期护栏）
  if (
    MINUTE_AND_HOUR_GRANULARITIES.has(granularity) &&
    startDate < nowTs - TREND_DETAIL_RETENTION_SECONDS
  ) {
    return false;
  }
  return (
    estimateTrendBucketCount(startDate, endDate, granularity) <=
    TREND_BUCKET_LIMIT
  );
}

/** 短横轴标签：分钟/小时含时刻；月带年份；天/周跨年时带年份。 */
export function formatTrendBucketLabel(
  isoDate: string,
  granularity: ResolvedTrendGranularity,
  dateLocale: string,
  includeYear: boolean,
): string {
  const d = new Date(isoDate);
  if (MINUTE_AND_HOUR_GRANULARITIES.has(granularity)) {
    return d.toLocaleString(dateLocale, {
      month: "2-digit",
      day: "2-digit",
      hour: "2-digit",
      minute: "2-digit",
    });
  }
  if (granularity === "month") {
    return d.toLocaleDateString(dateLocale, {
      year: "numeric",
      month: "2-digit",
    });
  }
  // day / week：周桶 bucketStart 已是周一零点，与天桶同为日期标签
  return includeYear
    ? d.toLocaleDateString(dateLocale, {
        year: "2-digit",
        month: "2-digit",
        day: "2-digit",
      })
    : d.toLocaleDateString(dateLocale, { month: "2-digit", day: "2-digit" });
}

/** Tooltip 用的完整标签（始终带年份）。 */
export function formatTrendBucketTooltipLabel(
  isoDate: string,
  granularity: ResolvedTrendGranularity,
  dateLocale: string,
): string {
  const d = new Date(isoDate);
  if (MINUTE_AND_HOUR_GRANULARITIES.has(granularity)) {
    return d.toLocaleString(dateLocale, {
      year: "numeric",
      month: "2-digit",
      day: "2-digit",
      hour: "2-digit",
      minute: "2-digit",
    });
  }
  return d.toLocaleDateString(dateLocale, {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
  });
}

/** Recharts 宽表行：元数据字段 + 每系列一个 value{i} 数值列。 */
export interface TrendSeriesChartRow {
  xKey: string;
  label: string;
  tooltipLabel: string;
  [column: string]: string | number;
}

export interface TrendSeriesChartModel {
  rows: TrendSeriesChartRow[];
  /** 原始系列名（图例/颜色/标签用）。 */
  seriesKeys: string[];
  /** 与 seriesKeys 平行的行内数值列名（value0…），
   *  与用户可见的系列名彻底隔离，杜绝真实模型/Provider 名撞上元数据字段。 */
  columns: string[];
}

function pointValue(point: UsageTrendSeriesPoint, metric: TrendMetric): number {
  if (metric === "cost") {
    return parseFiniteNumber(point.cost) ?? 0;
  }
  return (
    point.inputTokens +
    point.outputTokens +
    point.cacheCreationTokens +
    point.cacheReadTokens
  );
}

/**
 * 后端 buckets → Recharts 行（宽表化、缺系列补零、Top-N + “其他”聚合、
 * 指标推导）。纯函数，导出供单测。
 */
export function buildTrendSeriesChart(
  response: UsageTrendSeriesResponse | undefined,
  options: {
    groupBy: TrendGroupBy;
    metric: TrendMetric;
    dateLocale: string;
    topN?: number;
  },
): TrendSeriesChartModel {
  const { groupBy, metric, dateLocale, topN = TREND_TOP_N } = options;
  const buckets = response?.buckets ?? [];
  if (buckets.length === 0) {
    return { rows: [], seriesKeys: [], columns: [] };
  }
  const granularity = response?.granularity ?? "day";

  // 维度决定系列集合：token_type 固定四系列；其余取全范围 key 并集。
  const unionKeys = new Set<string>();
  const totals = new Map<string, number>();
  for (const bucket of buckets) {
    for (const point of bucket.series) {
      unionKeys.add(point.key);
      totals.set(
        point.key,
        (totals.get(point.key) ?? 0) + pointValue(point, metric),
      );
    }
  }
  // 防御：真实系列名与合成 key 撞名时把该系列折进“其他”，避免同名双系列
  const ranked = [...unionKeys]
    .filter((key) => key !== OTHER_SERIES_KEY)
    .sort(
      (a, b) =>
        (totals.get(b) ?? 0) - (totals.get(a) ?? 0) || a.localeCompare(b),
    );
  const topKeys =
    groupBy === "token_type" ? [...TOKEN_TYPE_SERIES] : ranked.slice(0, topN);
  // 并集大小（含被防御折叠的撞名系列）决定是否需要“其他”桶
  const hasOther = groupBy !== "token_type" && unionKeys.size > topKeys.length;
  const seriesKeys = hasOther ? [...topKeys, OTHER_SERIES_KEY] : [...topKeys];
  const columns = seriesKeys.map((_, index) => `value${index}`);

  // 天/周标签：范围跨年才带年份
  const firstYear = new Date(buckets[0].bucketStart).getFullYear();
  const lastYear = new Date(
    buckets[buckets.length - 1].bucketStart,
  ).getFullYear();
  const includeYear = firstYear !== lastYear;

  const rows = buckets.map((bucket) => {
    // 非头部系列全部折入“其他”
    const acc = new Map<string, number>();
    for (const point of bucket.series) {
      const target = topKeys.includes(point.key) ? point.key : OTHER_SERIES_KEY;
      acc.set(target, (acc.get(target) ?? 0) + pointValue(point, metric));
    }
    const row: TrendSeriesChartRow = {
      xKey: bucket.bucketStart,
      label: formatTrendBucketLabel(
        bucket.bucketStart,
        granularity,
        dateLocale,
        includeYear,
      ),
      tooltipLabel: formatTrendBucketTooltipLabel(
        bucket.bucketStart,
        granularity,
        dateLocale,
      ),
    };
    for (const [index, key] of seriesKeys.entries()) {
      row[columns[index]] = acc.get(key) ?? 0;
    }
    return row;
  });

  return { rows, seriesKeys, columns };
}

interface UsageStackedBarChartProps {
  range: UsageRangeSelection;
  appType?: string;
  providerName?: string;
  model?: string;
  refreshIntervalMs: number;
}

const GRANULARITY_OPTIONS: TrendGranularityOption[] = [
  "auto",
  "1min",
  "5min",
  "15min",
  "30min",
  "hour",
  "day",
  "week",
  "month",
];

const GRANULARITY_LABEL_KEYS: Record<TrendGranularityOption, string> = {
  auto: "usage.trendSeries.granularityAuto",
  "1min": "usage.trendSeries.granularity1min",
  "5min": "usage.trendSeries.granularity5min",
  "15min": "usage.trendSeries.granularity15min",
  "30min": "usage.trendSeries.granularity30min",
  hour: "usage.trendSeries.granularityHour",
  day: "usage.trendSeries.granularityDay",
  week: "usage.trendSeries.granularityWeek",
  month: "usage.trendSeries.granularityMonth",
};

const GROUP_BY_LABEL_KEYS: Record<TrendGroupBy, string> = {
  token_type: "usage.trendSeries.groupByTokenType",
  model: "usage.trendSeries.groupByModel",
  provider: "usage.trendSeries.groupByProvider",
};

const TOKEN_TYPE_LABEL_KEYS: Record<string, string> = {
  input: "usage.inputTokens",
  output: "usage.outputTokens",
  cacheCreation: "usage.cacheCreationTokens",
  cacheRead: "usage.cacheReadTokens",
};

/** 堆叠柱状图：颗粒度 × 堆叠维度 × 指标。渲染在现有趋势面积图下方。 */
export function UsageStackedBarChart({
  range,
  appType,
  providerName,
  model,
  refreshIntervalMs,
}: UsageStackedBarChartProps) {
  const { t, i18n } = useTranslation();
  const { startDate, endDate } = resolveUsageRange(range);
  // 维度/颗粒度/指标选择不持久化（会话内 state，与现有面板控件一致）
  const [granularity, setGranularity] =
    useState<TrendGranularityOption>("auto");
  const [groupBy, setGroupBy] = useState<TrendGroupBy>("token_type");
  const [metric, setMetric] = useState<TrendMetric>("tokens");
  const [hiddenKeys, setHiddenKeys] = useState<ReadonlySet<string>>(new Set());

  const { data: response, isLoading } = useUsageTrendSeries(
    range,
    { appType, providerName, model },
    granularity,
    groupBy,
    {
      refetchInterval: refreshIntervalMs > 0 ? refreshIntervalMs : false,
    },
  );

  const language = i18n.resolvedLanguage || i18n.language || "en";
  const dateLocale = getLocaleFromLanguage(language);

  const { rows, seriesKeys, columns } = useMemo(
    () => buildTrendSeriesChart(response, { groupBy, metric, dateLocale }),
    [response, groupBy, metric, dateLocale],
  );

  const seriesLabels = useMemo(() => {
    const labels = new Map<string, string>();
    for (const key of seriesKeys) {
      if (key === OTHER_SERIES_KEY) {
        labels.set(key, t("usage.trendSeries.other"));
      } else if (groupBy === "token_type") {
        labels.set(key, t(TOKEN_TYPE_LABEL_KEYS[key] ?? key));
      } else {
        labels.set(key, key);
      }
    }
    return labels;
  }, [seriesKeys, groupBy, t]);

  const handleGroupBy = (next: TrendGroupBy) => {
    setGroupBy(next);
    setHiddenKeys(new Set());
    if (next === "token_type") {
      // token_type 维度无分类型成本，回落 Tokens 指标
      setMetric("tokens");
    }
  };

  const toggleKey = (key: string) => {
    setHiddenKeys((prev) => {
      const next = new Set(prev);
      if (next.has(key)) {
        next.delete(key);
      } else {
        next.add(key);
      }
      return next;
    });
  };

  if (isLoading) {
    return (
      <div className="flex h-[350px] items-center justify-center rounded-xl bg-card/40 border border-border/50">
        <Loader2 className="h-8 w-8 animate-spin text-muted-foreground/30" />
      </div>
    );
  }

  const costDisabled = groupBy === "token_type";

  const CustomTooltip = ({ active, payload }: any) => {
    if (!active || !payload?.length) {
      return null;
    }
    const row = payload[0]?.payload as TrendSeriesChartRow | undefined;
    const heading = row?.tooltipLabel ?? "";
    const total = columns.reduce(
      (sum, column) => sum + (Number(row?.[column] ?? 0) || 0),
      0,
    );
    return (
      <div className="rounded-lg border bg-background/95 p-3 shadow-lg backdrop-blur-md">
        <p className="mb-2 font-medium">{heading}</p>
        {payload.map((entry: any) => (
          <div
            key={entry.dataKey}
            className="flex items-center gap-2 text-sm"
            style={{ color: entry.color }}
          >
            <div
              className="h-2 w-2 rounded-full"
              style={{ backgroundColor: entry.color }}
            />
            <span className="font-medium">{entry.name}:</span>
            <span>
              {metric === "cost"
                ? fmtUsd(entry.value, 6)
                : fmtInt(entry.value, dateLocale)}
            </span>
          </div>
        ))}
        <p className="mt-2 border-t pt-2 text-sm text-muted-foreground">
          {t("usage.trendSeries.total")}:{" "}
          {metric === "cost" ? fmtUsd(total, 6) : fmtInt(total, dateLocale)}
        </p>
      </div>
    );
  };

  return (
    <div className="rounded-xl border border-border/50 bg-card/40 p-6 backdrop-blur-sm">
      <div className="mb-4 flex flex-wrap items-center gap-3">
        <h3 className="text-lg font-semibold">
          {t("usage.trendSeries.title")}
        </h3>

        <div className="flex items-center gap-1 rounded-lg bg-muted/50 p-1 text-xs">
          {(["tokens", "cost"] as const).map((m) => (
            <button
              key={m}
              type="button"
              disabled={m === "cost" && costDisabled}
              onClick={() => setMetric(m)}
              className={`rounded-md px-2.5 py-1 transition-colors disabled:cursor-not-allowed disabled:opacity-40 ${
                metric === m
                  ? "bg-background font-medium shadow-sm"
                  : "text-muted-foreground hover:text-foreground"
              }`}
              title={
                m === "cost" && costDisabled
                  ? t("usage.trendSeries.costUnavailableForTokenType")
                  : undefined
              }
            >
              {m === "tokens"
                ? t("usage.trendSeries.metricTokens")
                : t("usage.trendSeries.metricCost")}
            </button>
          ))}
        </div>

        <div className="flex items-center gap-1 rounded-lg bg-muted/50 p-1 text-xs">
          {(["token_type", "model", "provider"] as const).map((g) => (
            <button
              key={g}
              type="button"
              onClick={() => handleGroupBy(g)}
              className={`rounded-md px-2.5 py-1 transition-colors ${
                groupBy === g
                  ? "bg-background font-medium shadow-sm"
                  : "text-muted-foreground hover:text-foreground"
              }`}
            >
              {t(GROUP_BY_LABEL_KEYS[g])}
            </button>
          ))}
        </div>

        <Select
          value={granularity}
          onValueChange={(v) => setGranularity(v as TrendGranularityOption)}
        >
          <SelectTrigger
            className="ml-auto h-8 w-[150px] bg-background text-xs"
            aria-label={t("usage.trendSeries.granularity")}
          >
            <span className="flex items-center gap-1.5">
              <BarChart3 className="h-3.5 w-3.5 shrink-0" />
              <SelectValue />
            </span>
          </SelectTrigger>
          <SelectContent>
            {GRANULARITY_OPTIONS.map((option) => (
              <SelectItem
                key={option}
                value={option}
                disabled={!isGranularitySelectable(startDate, endDate, option)}
                title={
                  isGranularitySelectable(startDate, endDate, option)
                    ? undefined
                    : t("usage.trendSeries.granularityTooFine")
                }
              >
                {option === "auto" && response
                  ? t("usage.trendSeries.autoResolved", {
                      granularity: t(
                        GRANULARITY_LABEL_KEYS[response.granularity],
                      ),
                    })
                  : t(GRANULARITY_LABEL_KEYS[option])}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      {rows.length === 0 ? (
        <div className="flex h-[350px] items-center justify-center text-sm text-muted-foreground">
          {t("usage.noData")}
        </div>
      ) : (
        <>
          <div className="h-[350px] w-full">
            <ResponsiveContainer width="100%" height="100%">
              <BarChart
                data={rows}
                margin={{ top: 10, right: 10, left: 0, bottom: 0 }}
              >
                <CartesianGrid
                  strokeDasharray="3 3"
                  vertical={false}
                  stroke="hsl(var(--border))"
                  opacity={0.4}
                />
                <XAxis
                  dataKey="xKey"
                  axisLine={false}
                  tickLine={false}
                  tick={{ fill: "hsl(var(--muted-foreground))", fontSize: 12 }}
                  dy={10}
                  minTickGap={24}
                  tickFormatter={(value) =>
                    rows.find((row) => row.xKey === value)?.label ??
                    String(value)
                  }
                />
                <YAxis
                  axisLine={false}
                  tickLine={false}
                  tick={{ fill: "hsl(var(--muted-foreground))", fontSize: 12 }}
                  tickFormatter={(value) =>
                    metric === "cost"
                      ? `$${value}`
                      : `${(value / 1000).toFixed(0)}k`
                  }
                />
                <Tooltip content={<CustomTooltip />} />
                {seriesKeys.map((key, index) => (
                  <Bar
                    key={key}
                    dataKey={columns[index]}
                    name={seriesLabels.get(key) ?? key}
                    stackId="usage"
                    fill={seriesColor(key, groupBy)}
                    hide={hiddenKeys.has(key)}
                  />
                ))}
              </BarChart>
            </ResponsiveContainer>
          </div>

          {/* 自定义图例：随维度切换；点击淡化（hide）而非移除，可再点恢复 */}
          <div className="mt-3 flex flex-wrap items-center gap-x-4 gap-y-2 text-xs">
            {seriesKeys.map((key) => {
              const hidden = hiddenKeys.has(key);
              return (
                <button
                  key={key}
                  type="button"
                  onClick={() => toggleKey(key)}
                  className={`flex items-center gap-1.5 transition-opacity ${
                    hidden ? "opacity-40" : "opacity-100"
                  }`}
                >
                  <span
                    className="h-2.5 w-2.5 rounded-sm"
                    style={{ backgroundColor: seriesColor(key, groupBy) }}
                  />
                  <span>{seriesLabels.get(key) ?? key}</span>
                </button>
              );
            })}
          </div>
        </>
      )}
    </div>
  );
}
