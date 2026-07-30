/**
 * Quota-trend chart (additive — reads only what the hourly probe recorded).
 *
 * Plots the utilization of each subscription window (5h / 7d / ...) over the
 * dashboard's selected date range, so a window that has long since reset can
 * still be reviewed after the fact. Rows come from the `fork_quota_history`
 * table via `useQuotaHistory`; the probe invalidates that query whenever a
 * reading adds something new.
 */
import { useMemo, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import {
  CartesianGrid,
  Legend,
  Line,
  LineChart,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";
import { History, Loader2 } from "lucide-react";
import { resolveUsageRange } from "@/lib/usageRange";
import type { UsageRangeSelection } from "@/types/usage";
import { TIER_I18N_KEYS } from "@/components/SubscriptionQuotaFooter";
import { PROBE_APPS } from "@/hooks/useQuotaHistoryProbe";
import { useQuotaHistory } from "@/lib/query/quotaHistory";
import { getLocaleFromLanguage } from "./format";
import {
  appsWithHistory,
  hourIndex,
  rowsToSeries,
  usdMaxKey,
  usdUsedKey,
} from "./quotaHistory";

interface QuotaTrendChartProps {
  range: UsageRangeSelection;
  rangeLabel: string;
  /** Dashboard app filter; "all" falls back to the first app with history. */
  appType?: string;
  /** Rendered next to the title (hosts the usage/quota switch). */
  titleSlot?: ReactNode;
}

/** Stable per-window colors, aligned with the usage chart's palette. */
const TIER_COLORS: Record<string, string> = {
  five_hour: "#3b82f6",
  seven_day: "#22c55e",
  seven_day_opus: "#f97316",
  seven_day_sonnet: "#a855f7",
  weekly_limit: "#22c55e",
  "30_day": "#eab308",
  monthly: "#eab308",
  premium: "#14b8a6",
};
const FALLBACK_COLORS = ["#f43f5e", "#0ea5e9", "#84cc16", "#d946ef"];

function tierColor(tier: string, index: number): string {
  return TIER_COLORS[tier] ?? FALLBACK_COLORS[index % FALLBACK_COLORS.length];
}

export function QuotaTrendChart({
  range,
  rangeLabel,
  appType,
  titleSlot,
}: QuotaTrendChartProps) {
  const { t, i18n } = useTranslation();

  const { startDate, endDate } = resolveUsageRange(range);
  const language = i18n.resolvedLanguage || i18n.language || "en";
  const locale = getLocaleFromLanguage(language);

  // Quantize to hours: the range end is "now", so a second-resolution query key
  // would change on every render and refetch forever.
  const startHour = hourIndex(startDate * 1000);
  const endHour = hourIndex(endDate * 1000);
  const { data: history, isLoading } = useQuotaHistory(startHour, endHour);

  // "all" has no single subject: show the first quota-capable app that has
  // actually recorded something (claude → codex → gemini).
  const appId = useMemo(() => {
    if (appType && appType !== "all") return appType;
    return appsWithHistory(history ?? [], PROBE_APPS)[0] ?? PROBE_APPS[0];
  }, [appType, history]);

  const { tiers, rows } = useMemo(
    () => rowsToSeries(history ?? [], appId),
    [history, appId],
  );

  const spansOneDay = endDate - startDate <= 36 * 60 * 60;
  const formatTs = (ts: number) =>
    new Date(ts).toLocaleString(
      locale,
      spansOneDay
        ? {
            month: "2-digit",
            day: "2-digit",
            hour: "2-digit",
            minute: "2-digit",
          }
        : { month: "2-digit", day: "2-digit", hour: "2-digit" },
    );

  const tierLabel = (tier: string) =>
    TIER_I18N_KEYS[tier] ? t(TIER_I18N_KEYS[tier]) : tier;

  const CustomTooltip = ({ active, payload, label }: any) => {
    if (!active || !payload?.length) return null;
    const row = payload[0]?.payload ?? {};
    return (
      <div className="rounded-lg border bg-background/95 p-3 shadow-lg backdrop-blur-md">
        <p className="mb-2 font-medium">{formatTs(label)}</p>
        {payload.map((entry: any, index: number) => {
          if (entry.value == null) return null;
          const used = row[usdUsedKey(entry.dataKey)];
          const max = row[usdMaxKey(entry.dataKey)];
          return (
            <div
              key={index}
              className="flex items-center gap-2 text-sm"
              style={{ color: entry.color }}
            >
              <div
                className="h-2 w-2 rounded-full"
                style={{ backgroundColor: entry.color }}
              />
              <span className="font-medium">{entry.name}:</span>
              <span className="tabular-nums">{entry.value.toFixed(1)}%</span>
              {used != null && (
                <span className="text-muted-foreground">
                  (${used.toFixed(2)}
                  {max != null ? ` / $${max.toFixed(2)}` : ""})
                </span>
              )}
            </div>
          );
        })}
      </div>
    );
  };

  return (
    <div className="rounded-xl border border-border/50 bg-card/40 p-6 backdrop-blur-sm">
      <div className="mb-6 flex items-center justify-between">
        <div className="flex items-center gap-4">
          <h3 className="text-lg font-semibold">
            {t("usage.quotaTrends", "额度趋势")}
          </h3>
          {titleSlot}
        </div>
        <div className="flex items-center gap-3 text-sm text-muted-foreground">
          <span className="uppercase tracking-wide text-xs">{appId}</span>
          <span>{rangeLabel}</span>
        </div>
      </div>

      <div className="h-[350px] w-full">
        {isLoading ? (
          <div className="flex h-full items-center justify-center">
            <Loader2 className="h-8 w-8 animate-spin text-muted-foreground/30" />
          </div>
        ) : tiers.length === 0 ? (
          <div className="flex h-full flex-col items-center justify-center gap-2 text-center">
            <History className="h-8 w-8 text-muted-foreground/30" />
            <p className="text-sm text-muted-foreground">
              {t("usage.quotaTrendEmpty", "该区间暂无额度记录")}
            </p>
            <p className="max-w-md text-xs text-muted-foreground/70">
              {t(
                "usage.quotaTrendHint",
                "额度探针每小时记录一次订阅窗口用量，保持应用运行即可逐步累积历史。",
              )}
            </p>
          </div>
        ) : (
          <ResponsiveContainer width="100%" height="100%">
            <LineChart
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
                dataKey="ts"
                type="number"
                scale="time"
                domain={["dataMin", "dataMax"]}
                axisLine={false}
                tickLine={false}
                tick={{ fill: "hsl(var(--muted-foreground))", fontSize: 12 }}
                minTickGap={48}
                tickFormatter={formatTs}
                dy={10}
              />
              <YAxis
                domain={[0, 100]}
                axisLine={false}
                tickLine={false}
                tick={{ fill: "hsl(var(--muted-foreground))", fontSize: 12 }}
                tickFormatter={(value) => `${value}%`}
              />
              <Tooltip content={<CustomTooltip />} />
              <Legend />
              {tiers.map((tier, index) => (
                <Line
                  key={tier}
                  type="monotone"
                  dataKey={tier}
                  name={tierLabel(tier)}
                  stroke={tierColor(tier, index)}
                  strokeWidth={2}
                  dot={false}
                  // Gaps are real (app closed): never bridge them with a
                  // straight line that would read as steady usage.
                  connectNulls={false}
                />
              ))}
            </LineChart>
          </ResponsiveContainer>
        )}
      </div>
    </div>
  );
}
