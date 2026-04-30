import { useTranslation } from "react-i18next";
import {
  BarChart,
  Bar,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ResponsiveContainer,
} from "recharts";
import { useUsageTrends } from "@/lib/query/usage";
import { Loader2 } from "lucide-react";
import {
  fmtInt,
  getLocaleFromLanguage,
} from "./format";
import { resolveUsageRange } from "@/lib/usageRange";
import type { UsageRangeSelection } from "@/types/usage";

interface UsageTrendChartProps {
  range: UsageRangeSelection;
  rangeLabel: string;
  appType?: string;
  refreshIntervalMs: number;
}

// Blue-purple gradient palette for the stacked bars
const CHART_COLORS = {
  input: "#6366f1",       // indigo-500
  output: "#8b5cf6",      // violet-500
  cacheRead: "#a78bfa",   // violet-400
  cacheCreation: "#c4b5fd", // violet-300
};

export function UsageTrendChart({
  range,
  rangeLabel,
  appType,
  refreshIntervalMs,
}: UsageTrendChartProps) {
  const { t, i18n } = useTranslation();
  const { startDate, endDate } = resolveUsageRange(range);
  const { data: trends, isLoading } = useUsageTrends(range, appType, {
    refetchInterval: refreshIntervalMs > 0 ? refreshIntervalMs : false,
  });

  if (isLoading) {
    return (
      <div className="flex h-[280px] items-center justify-center liquid-glass rounded-2xl">
        <Loader2 className="h-6 w-6 animate-spin text-muted-foreground/30" />
      </div>
    );
  }

  const durationSeconds = Math.max(endDate - startDate, 0);
  const isHourly = durationSeconds <= 24 * 60 * 60;
  const language = i18n.resolvedLanguage || i18n.language || "en";
  const dateLocale = getLocaleFromLanguage(language);

  const chartData =
    trends?.map((stat) => {
      const pointDate = new Date(stat.date);
      return {
        rawDate: stat.date,
        label: isHourly
          ? pointDate.toLocaleString(dateLocale, {
              month: "2-digit",
              day: "2-digit",
              hour: "2-digit",
              minute: "2-digit",
            })
          : pointDate.toLocaleDateString(dateLocale, {
              month: "short",
              day: "numeric",
            }),
        input: stat.totalInputTokens,
        output: stat.totalOutputTokens,
        cacheRead: stat.totalCacheReadTokens,
        cacheCreation: stat.totalCacheCreationTokens,
      };
    }) || [];

  const CustomTooltip = ({ active, payload, label }: any) => {
    if (!active || !payload?.length) return null;

    const total = payload.reduce(
      (sum: number, entry: any) => sum + (entry.value || 0),
      0,
    );

    return (
      <div className="liquid-glass rounded-xl px-3 py-2.5 min-w-[180px]">
        <p className="text-xs font-medium text-foreground mb-2">{label}</p>
        <div className="space-y-1">
          {payload.map((entry: any) => (
            <div
              key={entry.dataKey}
              className="flex items-center justify-between gap-4 text-xs"
            >
              <div className="flex items-center gap-1.5">
                <div
                  className="h-2 w-2 rounded-full"
                  style={{ backgroundColor: entry.fill || entry.color }}
                />
                <span className="text-muted-foreground">{entry.name}</span>
              </div>
              <span className="font-mono font-medium text-foreground">
                {fmtInt(entry.value, dateLocale)}
              </span>
            </div>
          ))}
        </div>
        <div className="mt-1.5 pt-1.5 border-t border-white/10 dark:border-white/5 flex items-center justify-between text-xs">
          <span className="text-muted-foreground">
            {t("usage.totalTokens", "Total")}
          </span>
          <span className="font-mono font-semibold text-foreground">
            {fmtInt(total, dateLocale)}
          </span>
        </div>
      </div>
    );
  };

  return (
    <div className="liquid-glass rounded-2xl p-5">
      <div className="mb-4 flex items-center justify-between">
        <h3 className="text-sm font-semibold text-foreground">
          {t("usage.trends", "Usage Trends")}
        </h3>
        <span className="text-xs text-muted-foreground">{rangeLabel}</span>
      </div>

      {/* Legend */}
      <div className="flex items-center gap-4 mb-4">
        {[
          { key: "input", label: t("usage.inputTokens", "Input"), color: CHART_COLORS.input },
          { key: "output", label: t("usage.outputTokens", "Output"), color: CHART_COLORS.output },
          { key: "cacheRead", label: t("usage.cacheReadTokens", "Cache Read"), color: CHART_COLORS.cacheRead },
          { key: "cacheCreation", label: t("usage.cacheCreationTokens", "Cache Write"), color: CHART_COLORS.cacheCreation },
        ].map((item) => (
          <div key={item.key} className="flex items-center gap-1.5 text-xs text-muted-foreground">
            <div
              className="h-2.5 w-2.5 rounded"
              style={{ backgroundColor: item.color }}
            />
            {item.label}
          </div>
        ))}
      </div>

      <div className="h-[240px] w-full">
        <ResponsiveContainer width="100%" height="100%">
          <BarChart
            data={chartData}
            margin={{ top: 4, right: 4, left: -12, bottom: 0 }}
            barCategoryGap="20%"
          >
            <defs>
              <linearGradient id="gradInput" x1="0" y1="0" x2="0" y2="1">
                <stop offset="0%" stopColor={CHART_COLORS.input} stopOpacity={0.9} />
                <stop offset="100%" stopColor={CHART_COLORS.input} stopOpacity={0.7} />
              </linearGradient>
              <linearGradient id="gradOutput" x1="0" y1="0" x2="0" y2="1">
                <stop offset="0%" stopColor={CHART_COLORS.output} stopOpacity={0.9} />
                <stop offset="100%" stopColor={CHART_COLORS.output} stopOpacity={0.7} />
              </linearGradient>
              <linearGradient id="gradCacheRead" x1="0" y1="0" x2="0" y2="1">
                <stop offset="0%" stopColor={CHART_COLORS.cacheRead} stopOpacity={0.9} />
                <stop offset="100%" stopColor={CHART_COLORS.cacheRead} stopOpacity={0.7} />
              </linearGradient>
              <linearGradient id="gradCacheCreation" x1="0" y1="0" x2="0" y2="1">
                <stop offset="0%" stopColor={CHART_COLORS.cacheCreation} stopOpacity={0.9} />
                <stop offset="100%" stopColor={CHART_COLORS.cacheCreation} stopOpacity={0.7} />
              </linearGradient>
            </defs>
            <CartesianGrid
              strokeDasharray="none"
              vertical={false}
              stroke="currentColor"
              className="text-border/20"
            />
            <XAxis
              dataKey="label"
              axisLine={false}
              tickLine={false}
              tick={{ fill: "hsl(var(--muted-foreground))", fontSize: 11 }}
              dy={8}
            />
            <YAxis
              axisLine={false}
              tickLine={false}
              tick={{ fill: "hsl(var(--muted-foreground))", fontSize: 11 }}
              tickFormatter={(value) =>
                value >= 1_000_000
                  ? `${(value / 1_000_000).toFixed(1)}M`
                  : value >= 1_000
                    ? `${(value / 1_000).toFixed(0)}k`
                    : `${value}`
              }
              dx={-4}
            />
            <Tooltip
              content={<CustomTooltip />}
              cursor={{ fill: "hsl(var(--muted) / 0.3)", radius: 6 }}
            />
            <Bar
              dataKey="input"
              name={t("usage.inputTokens", "Input")}
              stackId="tokens"
              fill="url(#gradInput)"
              radius={[0, 0, 0, 0]}
            />
            <Bar
              dataKey="output"
              name={t("usage.outputTokens", "Output")}
              stackId="tokens"
              fill="url(#gradOutput)"
              radius={[0, 0, 0, 0]}
            />
            <Bar
              dataKey="cacheRead"
              name={t("usage.cacheReadTokens", "Cache Read")}
              stackId="tokens"
              fill="url(#gradCacheRead)"
              radius={[0, 0, 0, 0]}
            />
            <Bar
              dataKey="cacheCreation"
              name={t("usage.cacheCreationTokens", "Cache Write")}
              stackId="tokens"
              fill="url(#gradCacheCreation)"
              radius={[4, 4, 0, 0]}
            />
          </BarChart>
        </ResponsiveContainer>
      </div>
    </div>
  );
}
