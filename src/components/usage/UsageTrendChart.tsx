import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import {
  Area,
  AreaChart,
  Bar,
  BarChart,
  CartesianGrid,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";
import { useUsageTrends } from "@/lib/query/usage";
import { Loader2 } from "lucide-react";
import {
  fmtInt,
  formatHitRatePercent,
  getLocaleFromLanguage,
  parseFiniteNumber,
  resolveTrendChartType,
} from "./format";
import { resolveUsageRange } from "@/lib/usageRange";
import type { UsageRangeSelection } from "@/types/usage";
import { useMoneyFormat } from "@/hooks/useMoneyFormat";
import { useSettingsQuery } from "@/lib/query";
import type { UsageMetric } from "./rememberedChoice";

interface UsageTrendChartProps {
  range: UsageRangeSelection;
  /** 画哪个指标：由上方指标条选 */
  metric: UsageMetric;
  appType?: string;
  providerName?: string;
  model?: string;
  refreshIntervalMs: number;
}

export interface UsageTrendStatLike {
  date: string;
  requestCount?: number;
  totalInputTokens: number;
  totalOutputTokens: number;
  totalCacheCreationTokens: number;
  totalCacheReadTokens: number;
  totalCost: string | number;
}

export interface UsageTrendChartPoint {
  /** Unique category key for Recharts — must not collide across years. */
  xKey: string;
  rawDate: string;
  /** Short tick label shown on the X axis. */
  label: string;
  /** Fuller label used by the tooltip. */
  tooltipLabel: string;
  hour: number;
  inputTokens: number;
  outputTokens: number;
  cacheCreationTokens: number;
  cacheReadTokens: number;
  /** 请求数（「请求」指标） */
  requests: number;
  /** 真实消耗 Tokens = 新增输入 + 输出 + 缓存写入 + 缓存命中（和指标卡同口径） */
  tokens: number;
  cost: number | null;
  /** 缓存命中率（0–1）= 缓存命中 ÷（新增输入 + 缓存写入 + 缓存命中），和指标条同口径；没有输入时为 null */
  hitRate: number | null;
}

/** 缓存命中率（0–1）；分母（新增输入 + 缓存写入 + 缓存命中）为 0 时没有意义，返回 null */
export function hitRateOf(stat: UsageTrendStatLike): number | null {
  const cacheable =
    stat.totalInputTokens +
    stat.totalCacheCreationTokens +
    stat.totalCacheReadTokens;
  return cacheable > 0 ? stat.totalCacheReadTokens / cacheable : null;
}

/**
 * 命中率 Y 轴的下限：命中率通常都在 90% 以上，从 0 画起柱子几乎一样高。
 * 取这批数据的最低值，往下再留半个「最低值到 100%」的距离（至少 2 个百分点）当余量，
 * 再向下取整到 5% 的整数档；上限固定 100%。没有数据、或算出来低于 50% 时从 0 画。Exported for unit tests.
 */
export function hitRateAxisFloor(rates: (number | null)[]): number {
  const known = rates.filter((rate): rate is number => rate != null);
  if (known.length === 0) return 0;
  const min = Math.min(...known);
  const padded = min - Math.max((1 - min) / 2, 0.02);
  // 加一点容差：0.95 * 20 在浮点下是 18.999…，直接取整会多掉一档
  const floor = Math.floor(padded * 20 + 1e-9) / 20;
  // 数据本身跨度很大（最低值远低于 50%）时截断已无意义，照常从 0 画
  return floor < 0.5 ? 0 : floor;
}

/** Build chart rows from backend trend stats. Exported for unit tests. */
export function buildUsageTrendChartData(
  trends: UsageTrendStatLike[] | undefined,
  options: {
    isHourly: boolean;
    dateLocale: string;
    /** Inclusive range endpoints (unix seconds). Used to decide year labels. */
    startDate: number;
    endDate: number;
  },
): UsageTrendChartPoint[] {
  const { isHourly, dateLocale, startDate, endDate } = options;
  const startYear = new Date(startDate * 1000).getFullYear();
  const endYear = new Date(endDate * 1000).getFullYear();
  const spansMultipleYears = startYear !== endYear;
  // 按小时时刻度只写钟点（日期在标题和时间范围里已经有）；跨天的范围在 0 点那格写日期，标出换日
  const spansMultipleDays =
    new Date(startDate * 1000).toDateString() !==
    new Date(endDate * 1000).toDateString();

  return (
    trends?.map((stat) => {
      const pointDate = new Date(stat.date);
      const cost = parseFiniteNumber(stat.totalCost);
      // Prefer a stable unique key from the source timestamp / date string.
      // Falling back to ISO keeps categories unique even if the backend
      // returns sparse points that share the same local MM/DD across years.
      const xKey = stat.date;
      const tooltipLabel = isHourly
        ? pointDate.toLocaleString(dateLocale, {
            year: "numeric",
            month: "2-digit",
            day: "2-digit",
            hour: "2-digit",
            minute: "2-digit",
          })
        : pointDate.toLocaleDateString(dateLocale, {
            year: "numeric",
            month: "2-digit",
            day: "2-digit",
          });
      const label = isHourly
        ? spansMultipleDays && pointDate.getHours() === 0
          ? pointDate.toLocaleDateString(dateLocale, {
              month: "2-digit",
              day: "2-digit",
            })
          : pointDate.toLocaleTimeString(dateLocale, {
              hour: "2-digit",
              minute: "2-digit",
              hourCycle: "h23",
            })
        : spansMultipleYears
          ? pointDate.toLocaleDateString(dateLocale, {
              year: "2-digit",
              month: "2-digit",
              day: "2-digit",
            })
          : pointDate.toLocaleDateString(dateLocale, {
              month: "2-digit",
              day: "2-digit",
            });

      return {
        xKey,
        rawDate: stat.date,
        label,
        tooltipLabel,
        hour: pointDate.getHours(),
        inputTokens: stat.totalInputTokens,
        outputTokens: stat.totalOutputTokens,
        cacheCreationTokens: stat.totalCacheCreationTokens,
        cacheReadTokens: stat.totalCacheReadTokens,
        requests: stat.requestCount ?? 0,
        tokens:
          stat.totalInputTokens +
          stat.totalOutputTokens +
          stat.totalCacheCreationTokens +
          stat.totalCacheReadTokens,
        cost: cost ?? null,
        hitRate: hitRateOf(stat),
      };
    }) || []
  );
}

/** Resolve a tick label by the unique category key (not by filtered tick index). */
export function formatUsageTrendTickLabel(
  xKey: string,
  chartData: UsageTrendChartPoint[],
): string {
  const point = chartData.find((row) => row.xKey === xKey);
  return point?.label ?? xKey;
}

export interface UsageTrendTokenTickFormatter {
  format(value: number): string;
}

/**
 * 中日文的「万」「亿」各管四位数，5000万、2000亿 比英文的 50M、200B 宽一截，
 * 会把纵轴撑宽；这两段改写成 5千万、2千亿。
 */
function cjkThousandUnits(locale: string): Array<[number, string]> | null {
  if (locale === "zh-TW") {
    return [
      [1e11, "千億"],
      [1e7, "千萬"],
    ];
  }
  if (locale.startsWith("zh")) {
    return [
      [1e11, "千亿"],
      [1e7, "千万"],
    ];
  }
  if (locale.startsWith("ja")) {
    return [
      [1e11, "千億"],
      [1e7, "千万"],
    ];
  }
  return null;
}

export function createUsageTrendTokenTickFormatter(
  locale: string,
): UsageTrendTokenTickFormatter {
  const compact = new Intl.NumberFormat(locale, {
    notation: "compact",
    compactDisplay: "short",
    maximumFractionDigits: 1,
  });
  const units = cjkThousandUnits(locale);
  if (!units) return compact;
  const plain = new Intl.NumberFormat(locale, { maximumFractionDigits: 1 });
  return {
    format(value) {
      const abs = Math.abs(value);
      for (const [size, unit] of units) {
        if (abs < size || abs >= size * 10) continue;
        const scaled = Number((value / size).toFixed(1));
        // 9.96 千万这类进位到 10 的，交回原格式（9960万）
        if (Math.abs(scaled) < 10) return `${plain.format(scaled)}${unit}`;
      }
      return compact.format(value);
    },
  };
}

export function formatUsageTrendTokenTickLabel(
  value: unknown,
  formatter: UsageTrendTokenTickFormatter,
): string {
  const num = parseFiniteNumber(value);
  if (num == null) return "--";

  return formatter.format(num);
}

const AXIS_TICK = { fill: "var(--text-3)", fontSize: 11 };

export function UsageTrendChart({
  range,
  metric,
  appType,
  providerName,
  model,
  refreshIntervalMs,
}: UsageTrendChartProps) {
  const { t, i18n } = useTranslation();
  const { money, moneyTick, rate } = useMoneyFormat();
  // 柱形图 / 曲线面积图在用量「设置」里选
  const { data: settings } = useSettingsQuery();
  const chartType = resolveTrendChartType(settings?.usageDisplay);
  const { startDate, endDate } = resolveUsageRange(range);
  const { data: trends, isLoading } = useUsageTrends(
    range,
    { appType, providerName, model },
    {
      refetchInterval: refreshIntervalMs > 0 ? refreshIntervalMs : false,
    },
  );

  const durationSeconds = Math.max(endDate - startDate, 0);
  const isHourly = durationSeconds <= 24 * 60 * 60;
  const language = i18n.resolvedLanguage || i18n.language || "en";
  const dateLocale = getLocaleFromLanguage(language);
  const tokenTickFormatter = useMemo(
    () => createUsageTrendTokenTickFormatter(dateLocale),
    [dateLocale],
  );

  const chartData = useMemo(
    () =>
      buildUsageTrendChartData(trends, {
        isHourly,
        dateLocale,
        startDate,
        endDate,
      }),
    [trends, isHourly, dateLocale, startDate, endDate],
  );

  // 单指标：图只画指标条选中的那一个，单 Y 轴，标题和 tooltip 跟着走。
  const metricLabel = {
    cost: t("usage.trend.cost"),
    requests: t("usage.trend.requestsLegend"),
    tokens: t("usage.trend.tokens"),
    hitRate: t("usage.cacheHitRate"),
  }[metric];

  const formatMetric = (value: unknown) => {
    if (metric === "cost") return money(value, 4);
    if (metric === "hitRate") {
      const rate = parseFiniteNumber(value);
      return rate == null ? "—" : formatHitRatePercent(rate);
    }
    return fmtInt(value, dateLocale);
  };
  const formatYTick = (value: unknown) => {
    if (metric === "cost") return moneyTick(value);
    // 下限常是 95% 这类，中间刻度会落在 97.5%，留一位小数
    if (metric === "hitRate")
      return `${+((Number(value) || 0) * 100).toFixed(1)}%`;
    return formatUsageTrendTokenTickLabel(value, tokenTickFormatter);
  };

  const hitRateFloor = useMemo(
    () => hitRateAxisFloor(chartData.map((point) => point.hitRate)),
    [chartData],
  );

  const Chart = chartType === "bar" ? BarChart : AreaChart;
  // 成本先换算成显示币种再画，Y 轴才会按这个币种挑整齐的刻度。
  const dataKey =
    metric === "cost"
      ? (point: UsageTrendChartPoint) =>
          point.cost == null ? null : point.cost * rate
      : metric;

  const renderTooltip = ({ active, payload }: any) => {
    if (!active || !payload || payload.length === 0) return null;
    const point = payload[0]?.payload as UsageTrendChartPoint | undefined;
    const heading = point?.tooltipLabel ?? point?.label ?? "";
    return (
      <div className="rounded-[8px] border border-border bg-surface px-3 py-2 text-caption text-fg-1 shadow-v7-md">
        <p className="mb-1 font-semibold">{heading}</p>
        <div className="flex items-center gap-2 tabular-nums">
          <span
            aria-hidden="true"
            className="h-2 w-2 rounded-[2px] bg-chart-usage"
          />
          <span className="text-fg-2">{metricLabel}</span>
          <span className="ms-auto ps-3">
            {formatMetric(point ? point[metric] : payload[0]?.value)}
          </span>
        </div>
      </div>
    );
  };

  return (
    <section aria-labelledby="usage-trend-title" className="px-4 pb-2.5 pt-5">
      <h2
        id="usage-trend-title"
        className="m-0 min-h-7 whitespace-nowrap text-caption font-semibold leading-7 text-fg-2"
      >
        {t("usage.trend.titleMetric", {
          metric: metricLabel,
          granularity: isHourly
            ? t("usage.trend.hourly")
            : t("usage.trend.daily"),
        })}
      </h2>

      <div className="mt-1 h-[132px] w-full">
        {isLoading ? (
          <div className="flex h-full items-center justify-center">
            <Loader2 className="h-5 w-5 animate-spin text-fg-3" />
          </div>
        ) : (
          <ResponsiveContainer width="100%" height="100%">
            <Chart
              data={chartData}
              margin={{ top: 6, right: 0, left: 0, bottom: 0 }}
            >
              <CartesianGrid
                vertical={false}
                stroke="var(--chart-grid)"
                strokeWidth={1}
              />
              {/* 🔴 分类键必须是后端 RFC3339 桶时间戳（xKey），不能用显示文字，否则跨年错位 */}
              <XAxis
                dataKey="xKey"
                axisLine={false}
                tickLine={false}
                tick={AXIS_TICK}
                tickMargin={6}
                minTickGap={12}
                tickFormatter={(value) =>
                  formatUsageTrendTickLabel(String(value), chartData)
                }
                allowDuplicatedCategory={false}
              />
              <YAxis
                width="auto"
                axisLine={false}
                tickLine={false}
                tick={AXIS_TICK}
                tickCount={3}
                domain={metric === "hitRate" ? [hitRateFloor, 1] : undefined}
                tickFormatter={formatYTick}
              />
              <Tooltip
                content={renderTooltip}
                cursor={
                  chartType === "bar"
                    ? { fill: "var(--bg-subtle)" }
                    : { stroke: "var(--text-3)", strokeDasharray: "3 3" }
                }
              />
              {chartType === "bar" ? (
                <Bar
                  dataKey={dataKey}
                  name={metricLabel}
                  fill="var(--chart-usage)"
                  radius={[3, 3, 0, 0]}
                  maxBarSize={36}
                  isAnimationActive={false}
                />
              ) : (
                <Area
                  type="monotone"
                  dataKey={dataKey}
                  name={metricLabel}
                  stroke="var(--chart-usage)"
                  strokeWidth={2}
                  fill="var(--chart-usage)"
                  fillOpacity={0.16}
                  dot={chartData.length === 1 ? { r: 3 } : false}
                  activeDot={{ r: 4 }}
                  isAnimationActive={false}
                />
              )}
            </Chart>
          </ResponsiveContainer>
        )}
      </div>
    </section>
  );
}
