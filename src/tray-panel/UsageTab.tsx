import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import {
  Area,
  AreaChart,
  Bar,
  BarChart,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";
import { Loader2 } from "lucide-react";
import { usageApi } from "@/lib/api";
import { useMoneyFormat } from "@/hooks/useMoneyFormat";
import { useSettingsQuery } from "@/lib/query";
import { resolveUsageRange, getUsageRangePresetLabel } from "@/lib/usageRange";
import {
  fmtInt,
  formatHitRatePercent,
  formatTokensCompact,
  getLocaleFromLanguage,
  resolveTrendChartType,
} from "@/components/usage/format";
import {
  buildUsageTrendChartData,
  createUsageTrendTokenTickFormatter,
  formatUsageTrendTokenTickLabel,
  type UsageTrendChartPoint,
} from "@/components/usage/UsageTrendChart";
import type { DailyStats, UsageSummary } from "@/types/usage";
import { cn } from "@/lib/utils";

const RANGES = ["today", "7d", "30d", "all"] as const;
type Range = (typeof RANGES)[number];
const METRICS = ["tokens", "requests", "cost"] as const;
type Metric = (typeof METRICS)[number];

const RANGE_KEY = "cc-switch-tray-usage-range";
const METRIC_KEY = "cc-switch-tray-usage-metric";
/** 柱子最多这么多根，再多就把相邻的几天并成一根 */
const MAX_BARS = 31;
const DAY_SECONDS = 24 * 60 * 60;

function remembered<T extends string>(
  key: string,
  options: readonly T[],
  fallback: T,
): T {
  try {
    const value = window.localStorage.getItem(key);
    if (value && (options as readonly string[]).includes(value)) {
      return value as T;
    }
  } catch {
    // 读不到用默认值
  }
  return fallback;
}

function remember(key: string, value: string) {
  try {
    window.localStorage.setItem(key, value);
  } catch {
    // 记不住也不影响使用
  }
}

interface UsageData {
  summary: UsageSummary | null;
  trends: DailyStats[];
  startDate: number;
  endDate: number;
}

/** 「全部」从最早有记录的那天算起（按 0 起算后端会按天铺满几十年）。 */
async function loadUsage(range: Range): Promise<UsageData> {
  const resolved = resolveUsageRange({ preset: range });
  let { startDate } = resolved;
  const { endDate } = resolved;
  if (range === "all") {
    const first = await usageApi.getUsageFirstDate();
    if (!first) return { summary: null, trends: [], startDate, endDate };
    const [y, m, d] = first.split("-").map(Number);
    startDate = Math.floor(new Date(y, m - 1, d).getTime() / 1000);
    // 不到一天时后端会按小时分桶，至少给满一天按天显示
    startDate = Math.min(startDate, endDate - DAY_SECONDS - 1);
  }
  const [summary, trends] = await Promise.all([
    usageApi.getUsageSummary(startDate, endDate),
    usageApi.getUsageTrends(startDate, endDate),
  ]);
  return { summary, trends, startDate, endDate };
}

/** 相邻几根并成一根，标签用第一根的。 */
function bucketize(
  points: UsageTrendChartPoint[],
  maxBars: number,
): UsageTrendChartPoint[] {
  if (points.length <= maxBars) return points;
  const size = Math.ceil(points.length / maxBars);
  const merged: UsageTrendChartPoint[] = [];
  for (let i = 0; i < points.length; i += size) {
    const chunk = points.slice(i, i + size);
    const first = chunk[0];
    const last = chunk[chunk.length - 1];
    merged.push({
      ...first,
      tooltipLabel: `${first.label} – ${last.label}`,
      inputTokens: chunk.reduce((sum, p) => sum + p.inputTokens, 0),
      outputTokens: chunk.reduce((sum, p) => sum + p.outputTokens, 0),
      cacheCreationTokens: chunk.reduce(
        (sum, p) => sum + p.cacheCreationTokens,
        0,
      ),
      cacheReadTokens: chunk.reduce((sum, p) => sum + p.cacheReadTokens, 0),
      requests: chunk.reduce((sum, p) => sum + p.requests, 0),
      tokens: chunk.reduce((sum, p) => sum + p.tokens, 0),
      cost: chunk.some((p) => p.cost != null)
        ? chunk.reduce((sum, p) => sum + (p.cost ?? 0), 0)
        : null,
    });
  }
  return merged;
}

function Segmented<T extends string>({
  value,
  options,
  onChange,
  label,
  size = "md",
}: {
  value: T;
  options: { value: T; label: string }[];
  onChange: (value: T) => void;
  label: string;
  size?: "sm" | "md";
}) {
  return (
    <div
      role="tablist"
      aria-label={label}
      className="flex rounded-lg bg-black/[0.06] p-0.5 dark:bg-white/[0.08]"
    >
      {options.map((option) => (
        <button
          key={option.value}
          type="button"
          role="tab"
          aria-selected={option.value === value}
          onClick={() => onChange(option.value)}
          className={cn(
            "flex-1 whitespace-nowrap rounded-md font-medium transition-colors",
            size === "sm" ? "px-2 py-0.5 text-[11px]" : "px-2 py-1 text-xs",
            option.value === value
              ? "bg-white text-foreground shadow-sm dark:bg-white/[0.16]"
              : "text-muted-foreground hover:text-foreground",
          )}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}

function Stat({
  label,
  value,
  detail,
}: {
  label: string;
  value: string;
  /** 数值下面的一行小字（细分） */
  detail?: string;
}) {
  return (
    <div className="min-w-0 rounded-lg bg-black/[0.04] px-2.5 py-1.5 dark:bg-white/[0.06]">
      <div className="truncate text-[11px] text-muted-foreground">{label}</div>
      <div className="truncate text-[17px] font-semibold leading-6 tabular-nums">
        {value}
      </div>
      {detail && (
        <div className="truncate text-[10.5px] tabular-nums text-muted-foreground">
          {detail}
        </div>
      )}
    </div>
  );
}

export function UsageTab({
  refreshKey,
  visible,
}: {
  /** 每次弹出变一次：重新取数据 */
  refreshKey: number;
  visible: boolean;
}) {
  const { t, i18n } = useTranslation();
  const { money, moneyTick, rate } = useMoneyFormat();
  // 柱形图 / 曲线面积图跟主窗口用量「设置」里选的一样
  const { data: settings } = useSettingsQuery();
  const chartType = resolveTrendChartType(settings?.usageDisplay);
  const Chart = chartType === "bar" ? BarChart : AreaChart;
  const [range, setRange] = useState<Range>(() =>
    remembered(RANGE_KEY, RANGES, "today"),
  );
  const [metric, setMetric] = useState<Metric>(() =>
    remembered(METRIC_KEY, METRICS, "tokens"),
  );
  const locale = getLocaleFromLanguage(
    i18n.resolvedLanguage || i18n.language || "en",
  );

  const { data, isLoading } = useQuery({
    queryKey: ["tray-panel-usage", range, refreshKey],
    queryFn: () => loadUsage(range),
    enabled: visible,
    refetchInterval: visible ? 60_000 : false,
    placeholderData: (previous) => previous,
  });

  const tickFormatter = useMemo(
    () => createUsageTrendTokenTickFormatter(locale),
    [locale],
  );
  const chartData = useMemo(() => {
    if (!data) return [];
    const points = buildUsageTrendChartData(data.trends, {
      isHourly: data.endDate - data.startDate <= DAY_SECONDS,
      dateLocale: locale,
      startDate: data.startDate,
      endDate: data.endDate,
    });
    return bucketize(points, MAX_BARS).map((point) => ({
      ...point,
      // 金额先换算成显示币种，刻度才整齐
      costShown: point.cost == null ? null : point.cost * rate,
    }));
  }, [data, locale, rate]);

  const summary = data?.summary;
  // 显示币种的符号（从 0 的刻度里取，跟着设置里的币种）
  const currencySymbol = moneyTick(0).replace(/[\d.,\s-]/g, "");
  const rangeLabel = getUsageRangePresetLabel(range, t);
  const dataKey =
    metric === "cost"
      ? "costShown"
      : metric === "requests"
        ? "requests"
        : "tokens";
  const formatValue = (point: UsageTrendChartPoint) =>
    metric === "cost"
      ? money(point.cost, 4)
      : metric === "requests"
        ? fmtInt(point.requests, locale)
        : formatTokensCompact(point.tokens, locale);

  return (
    <div className="flex flex-col gap-2.5 px-3 pb-3">
      <Segmented
        label={t("usage.timeRange")}
        value={range}
        onChange={(next) => {
          setRange(next);
          remember(RANGE_KEY, next);
        }}
        options={RANGES.map((value) => ({
          value,
          label: getUsageRangePresetLabel(value, t),
        }))}
      />

      {!summary && isLoading ? (
        <div className="flex h-40 items-center justify-center text-muted-foreground">
          <Loader2 className="h-4 w-4 animate-spin" />
        </div>
      ) : !summary || summary.totalRequests === 0 ? (
        <div className="flex h-24 items-center justify-center text-xs text-muted-foreground">
          {t("usage.noData")}
        </div>
      ) : (
        <>
          {/* 上半：汇总四格，Tokens 的细分用小字写在格子里 */}
          <div className="grid grid-cols-2 gap-1.5">
            <Stat
              label={t("usage.totalCost")}
              value={money(summary.totalCost, 2)}
            />
            <Stat
              label={t("usage.totalRequests")}
              value={fmtInt(summary.totalRequests, locale)}
            />
            <Stat
              label={t("usage.realTotal")}
              value={formatTokensCompact(summary.realTotalTokens, locale)}
              detail={`${t("usage.inputTokens")} ${formatTokensCompact(summary.totalInputTokens, locale)} · ${t("usage.outputTokens")} ${formatTokensCompact(summary.totalOutputTokens, locale)}`}
            />
            <Stat
              label={t("usage.cacheHitRate")}
              value={formatHitRatePercent(summary.cacheHitRate)}
              detail={`${t("usage.cacheWrite")} ${formatTokensCompact(summary.totalCacheCreationTokens, locale)} · ${t("usage.cacheRead")} ${formatTokensCompact(summary.totalCacheReadTokens, locale)}`}
            />
          </div>
          {/* 下半：按时间的明细 */}
          <div className="rounded-lg bg-black/[0.04] px-2 pb-1 pt-2 dark:bg-white/[0.06]">
            <div className="mb-1 flex items-center gap-2 px-1">
              <span className="truncate text-[11px] font-medium text-muted-foreground">
                {t("usage.trend.title", { range: rangeLabel })}
              </span>
              <div className="ms-auto">
                <Segmented
                  size="sm"
                  label={t("usage.trend.metricLabel")}
                  value={metric}
                  onChange={(next) => {
                    setMetric(next);
                    remember(METRIC_KEY, next);
                  }}
                  options={[
                    { value: "tokens", label: t("usage.trend.tokens") },
                    { value: "requests", label: t("usage.trend.requests") },
                    { value: "cost", label: t("usage.trend.cost") },
                  ]}
                />
              </div>
            </div>
            <div className="h-[128px]">
              <ResponsiveContainer width="100%" height="100%">
                <Chart
                  data={chartData}
                  margin={{ top: 4, right: 4, bottom: 0, left: 0 }}
                >
                  <XAxis
                    dataKey="xKey"
                    tickFormatter={(key: string) =>
                      chartData.find((p) => p.xKey === key)?.label ?? key
                    }
                    tick={{ fill: "currentColor", fontSize: 10, opacity: 0.55 }}
                    tickLine={false}
                    axisLine={false}
                    interval="preserveStartEnd"
                    minTickGap={24}
                  />
                  <YAxis
                    width="auto"
                    tick={{ fill: "currentColor", fontSize: 10, opacity: 0.55 }}
                    tickLine={false}
                    axisLine={false}
                    tickCount={3}
                    tickFormatter={(value: unknown) =>
                      metric === "cost"
                        ? // 金额刻度也写成紧凑形式（$8K），窄面板里放得下
                          `${currencySymbol}${formatUsageTrendTokenTickLabel(value, tickFormatter)}`
                        : formatUsageTrendTokenTickLabel(value, tickFormatter)
                    }
                  />
                  <Tooltip
                    cursor={
                      chartType === "bar"
                        ? { fill: "rgba(127,127,127,0.12)" }
                        : {
                            stroke: "rgba(127,127,127,0.5)",
                            strokeDasharray: "3 3",
                          }
                    }
                    content={({ active, payload }) => {
                      const point = payload?.[0]?.payload as
                        | UsageTrendChartPoint
                        | undefined;
                      if (!active || !point) return null;
                      return (
                        <div className="rounded-md bg-popover px-2 py-1 text-[11px] text-popover-foreground shadow-md">
                          <div className="font-medium">
                            {point.tooltipLabel ?? point.label}
                          </div>
                          <div className="tabular-nums">
                            {formatValue(point)}
                          </div>
                        </div>
                      );
                    }}
                  />
                  {chartType === "bar" ? (
                    <Bar
                      dataKey={dataKey}
                      fill="#38bdf8"
                      radius={[3, 3, 0, 0]}
                      maxBarSize={18}
                      isAnimationActive={false}
                    />
                  ) : (
                    <Area
                      type="monotone"
                      dataKey={dataKey}
                      stroke="#38bdf8"
                      strokeWidth={2}
                      fill="#38bdf8"
                      fillOpacity={0.16}
                      dot={chartData.length === 1 ? { r: 3 } : false}
                      activeDot={{ r: 3 }}
                      isAnimationActive={false}
                    />
                  )}
                </Chart>
              </ResponsiveContainer>
            </div>
          </div>
        </>
      )}
    </div>
  );
}
