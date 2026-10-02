import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Bar,
  CartesianGrid,
  ComposedChart,
  Line,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";
import { SegmentedControl } from "@/components/ui/segmented-control";
import { useUsageTrends } from "@/lib/query/usage";
import { Loader2 } from "lucide-react";
import {
  fmtInt,
  fmtUsd,
  getLocaleFromLanguage,
  parseFiniteNumber,
} from "./format";
import { resolveUsageRange } from "@/lib/usageRange";
import type { UsageRangeSelection } from "@/types/usage";

interface UsageTrendChartProps {
  range: UsageRangeSelection;
  rangeLabel: string;
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
  /** 请求数（「请求」指标的柱） */
  requests: number;
  /** 真实消耗 Tokens = 新增输入 + 输出 + 缓存写入 + 缓存命中（和指标卡同口径） */
  tokens: number;
  cost: number | null;
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
        ? pointDate.toLocaleString(dateLocale, {
            month: "2-digit",
            day: "2-digit",
            hour: "2-digit",
            minute: "2-digit",
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

export function createUsageTrendTokenTickFormatter(
  locale: string,
): Intl.NumberFormat {
  return new Intl.NumberFormat(locale, {
    notation: "compact",
    compactDisplay: "short",
    maximumFractionDigits: 1,
  });
}

export function formatUsageTrendTokenTickLabel(
  value: unknown,
  formatter: Intl.NumberFormat,
): string {
  const num = parseFiniteNumber(value);
  if (num == null) return "--";

  return formatter.format(num);
}

type TrendMetric = "requests" | "tokens" | "cost";

const AXIS_TICK = { fill: "var(--text-3)", fontSize: 11 };

export function UsageTrendChart({
  range,
  rangeLabel,
  appType,
  providerName,
  model,
  refreshIntervalMs,
}: UsageTrendChartProps) {
  const { t, i18n } = useTranslation();
  const [metric, setMetric] = useState<TrendMetric>("requests");
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

  const showCostLine = metric !== "cost";
  const barLabel =
    metric === "requests"
      ? t("usage.trend.requestsLegend")
      : metric === "tokens"
        ? t("usage.trend.tokens")
        : t("usage.trend.cost");

  const formatMetric = (value: unknown, key: string) =>
    key === "cost" ? fmtUsd(value, 4) : fmtInt(value, dateLocale);
  const formatLeftTick = (value: unknown) =>
    metric === "cost"
      ? `$${parseFiniteNumber(value) ?? 0}`
      : formatUsageTrendTokenTickLabel(value, tokenTickFormatter);

  const renderTooltip = ({ active, payload }: any) => {
    if (!active || !payload || payload.length === 0) return null;
    const point = payload[0]?.payload as UsageTrendChartPoint | undefined;
    const heading = point?.tooltipLabel ?? point?.label ?? "";
    return (
      <div className="rounded-[8px] border border-border bg-surface px-3 py-2 text-caption text-fg-1 shadow-v7-md">
        <p className="mb-1 font-semibold">{heading}</p>
        {payload.map((entry: any) => (
          <div
            key={String(entry.dataKey)}
            className="flex items-center gap-2 tabular-nums"
          >
            <span
              aria-hidden="true"
              className="h-2 w-2 rounded-[2px]"
              style={{ backgroundColor: entry.color }}
            />
            <span className="text-fg-2">{entry.name}</span>
            <span className="ms-auto ps-3">
              {formatMetric(entry.value, String(entry.dataKey))}
            </span>
          </div>
        ))}
      </div>
    );
  };

  return (
    <section
      aria-labelledby="usage-trend-title"
      className="shrink-0 rounded-panel border border-border bg-surface px-4 py-2.5"
    >
      <div className="flex min-h-7 flex-wrap items-center gap-x-3.5 gap-y-1">
        <h2
          id="usage-trend-title"
          className="m-0 whitespace-nowrap text-caption font-semibold text-fg-2"
        >
          {t("usage.trend.title", { range: rangeLabel })}
        </h2>
        <span className="inline-flex items-center gap-1.5 whitespace-nowrap text-caption text-fg-2">
          <span
            aria-hidden="true"
            className="h-2.5 w-2.5 rounded-[2px] bg-chart-1"
          />
          {barLabel}
        </span>
        {showCostLine && (
          <span className="inline-flex items-center gap-1.5 whitespace-nowrap text-caption text-fg-2">
            <span
              aria-hidden="true"
              className="h-0.5 w-3.5 rounded-full bg-chart-2"
            />
            {t("usage.trend.costLine")}
          </span>
        )}
        <div className="flex-1" />
        <SegmentedControl<TrendMetric>
          size="sm"
          aria-label={t("usage.trend.metricLabel")}
          value={metric}
          onValueChange={setMetric}
          items={[
            { value: "requests", label: t("usage.trend.requests") },
            { value: "tokens", label: t("usage.trend.tokens") },
            { value: "cost", label: t("usage.trend.cost") },
          ]}
        />
      </div>

      <div className="mt-1 h-[132px] w-full">
        {isLoading ? (
          <div className="flex h-full items-center justify-center">
            <Loader2 className="h-5 w-5 animate-spin text-fg-3" />
          </div>
        ) : (
          <ResponsiveContainer width="100%" height="100%">
            <ComposedChart
              data={chartData}
              margin={{ top: 6, right: 0, left: 0, bottom: 0 }}
            >
              <CartesianGrid
                vertical={false}
                stroke="var(--chart-grid)"
                strokeWidth={1}
              />
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
                yAxisId="left"
                width={44}
                axisLine={false}
                tickLine={false}
                tick={AXIS_TICK}
                tickCount={3}
                tickFormatter={formatLeftTick}
              />
              {showCostLine && (
                <YAxis
                  yAxisId="cost"
                  orientation="right"
                  width={44}
                  axisLine={false}
                  tickLine={false}
                  tick={AXIS_TICK}
                  tickCount={3}
                  tickFormatter={(value) => `$${value}`}
                />
              )}
              <Tooltip
                content={renderTooltip}
                cursor={{ fill: "var(--bg-subtle)" }}
              />
              <Bar
                yAxisId="left"
                dataKey={metric}
                name={barLabel}
                fill="var(--chart-1)"
                radius={[3, 3, 0, 0]}
                maxBarSize={36}
                isAnimationActive={false}
              />
              {showCostLine && (
                <Line
                  yAxisId="cost"
                  type="linear"
                  dataKey="cost"
                  name={t("usage.trend.cost")}
                  stroke="var(--chart-2)"
                  strokeWidth={2}
                  dot={{
                    r: 3,
                    stroke: "var(--chart-2)",
                    strokeWidth: 1.5,
                    fill: "var(--bg-card)",
                  }}
                  activeDot={{ r: 4 }}
                  isAnimationActive={false}
                />
              )}
            </ComposedChart>
          </ResponsiveContainer>
        )}
      </div>
    </section>
  );
}
