import { useTranslation } from "react-i18next";
import {
  AreaChart,
  Area,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ResponsiveContainer,
  Legend,
} from "recharts";
import { useUsageTrends } from "@/lib/query/usage";
import { Loader2 } from "lucide-react";

interface UsageTrendChartProps {
  days: number;
}

export function UsageTrendChart({ days }: UsageTrendChartProps) {
  const { t, i18n } = useTranslation();
  const { data: trends, isLoading } = useUsageTrends(days);

  if (isLoading) {
    return (
      <div className="flex h-[350px] items-center justify-center rounded-xl bg-card/40 border border-border/50">
        <Loader2 className="h-8 w-8 animate-spin text-muted-foreground/30" />
      </div>
    );
  }

  const isToday = days === 1;
  const dateLocale =
    i18n.language === "zh"
      ? "zh-CN"
      : i18n.language === "ja"
        ? "ja-JP"
        : "en-US";
  const chartData =
    trends?.map((stat) => {
      const pointDate = new Date(stat.date);
      return {
        rawDate: stat.date,
        label: isToday
          ? pointDate.toLocaleString(dateLocale, {
              month: "2-digit",
              day: "2-digit",
              hour: "2-digit",
              minute: "2-digit",
            })
          : pointDate.toLocaleDateString(dateLocale, {
              month: "2-digit",
              day: "2-digit",
            }),
        hour: pointDate.getHours(),
        inputTokens: stat.totalInputTokens,
        outputTokens: stat.totalOutputTokens,
        cacheCreationTokens: stat.totalCacheCreationTokens,
        cacheReadTokens: stat.totalCacheReadTokens,
        cost: parseFloat(stat.totalCost),
      };
    }) || [];

  const displayData = chartData;

  const CustomTooltip = ({ active, payload, label }: any) => {
    if (active && payload && payload.length) {
      return (
        <div className="rounded-lg border bg-background/95 p-3 shadow-lg backdrop-blur-md">
          <p className="mb-2 font-medium">{label}</p>
          {payload.map((entry: any, index: number) => (
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
              <span>
                {entry.name.includes(t("usage.cost", "成本"))
                  ? `$${typeof entry.value === "number" ? entry.value.toFixed(6) : entry.value}`
                  : entry.value.toLocaleString()}
              </span>
            </div>
          ))}
        </div>
      );
    }
    return null;
  };

  return (
    <div className="rounded-xl border border-border/50 bg-card/40 p-6 backdrop-blur-sm">
      <div className="mb-6 flex items-center justify-between">
        <h3 className="text-lg font-semibold">
          {t("usage.trends", "使用趋势")}
        </h3>
        <p className="text-sm text-muted-foreground">
          {isToday
            ? t("usage.rangeToday", "今天 (按小时)")
            : days === 7
              ? t("usage.rangeLast7Days", "过去 7 天")
              : t("usage.rangeLast30Days", "过去 30 天")}
        </p>
      </div>

      <div className="h-[350px] w-full">
        <ResponsiveContainer width="100%" height="100%">
          <AreaChart
            data={displayData}
            margin={{ top: 10, right: 10, left: 0, bottom: 0 }}
          >
            <defs>
              <linearGradient id="colorInput" x1="0" y1="0" x2="0" y2="1">
                <stop offset="5%" stopColor="#3b82f6" stopOpacity={0.2} />
                <stop offset="95%" stopColor="#3b82f6" stopOpacity={0} />
              </linearGradient>
              <linearGradient id="colorOutput" x1="0" y1="0" x2="0" y2="1">
                <stop offset="5%" stopColor="#22c55e" stopOpacity={0.2} />
                <stop offset="95%" stopColor="#22c55e" stopOpacity={0} />
              </linearGradient>
              <linearGradient
                id="colorCacheCreation"
                x1="0"
                y1="0"
                x2="0"
                y2="1"
              >
                <stop offset="5%" stopColor="#f97316" stopOpacity={0.2} />
                <stop offset="95%" stopColor="#f97316" stopOpacity={0} />
              </linearGradient>
              <linearGradient id="colorCacheRead" x1="0" y1="0" x2="0" y2="1">
                <stop offset="5%" stopColor="#a855f7" stopOpacity={0.2} />
                <stop offset="95%" stopColor="#a855f7" stopOpacity={0} />
              </linearGradient>
            </defs>
            <CartesianGrid
              strokeDasharray="3 3"
              vertical={false}
              stroke="hsl(var(--border))"
              opacity={0.4}
            />
            <XAxis
              dataKey="label"
              axisLine={false}
              tickLine={false}
              tick={{ fill: "hsl(var(--muted-foreground))", fontSize: 12 }}
              dy={10}
            />
            <YAxis
              yAxisId="tokens"
              axisLine={false}
              tickLine={false}
              tick={{ fill: "hsl(var(--muted-foreground))", fontSize: 12 }}
              tickFormatter={(value) => `${(value / 1000).toFixed(0)}k`}
            />
            <YAxis
              yAxisId="cost"
              orientation="right"
              axisLine={false}
              tickLine={false}
              tick={{ fill: "hsl(var(--muted-foreground))", fontSize: 12 }}
              tickFormatter={(value) => `$${value}`}
            />
            <Tooltip content={<CustomTooltip />} />
            <Legend />
            <Area
              yAxisId="tokens"
              type="monotone"
              dataKey="inputTokens"
              name={t("usage.inputTokens", "输入 Tokens")}
              stroke="#3b82f6"
              fillOpacity={1}
              fill="url(#colorInput)"
              strokeWidth={2}
            />
            <Area
              yAxisId="tokens"
              type="monotone"
              dataKey="outputTokens"
              name={t("usage.outputTokens", "输出 Tokens")}
              stroke="#22c55e"
              fillOpacity={1}
              fill="url(#colorOutput)"
              strokeWidth={2}
            />
            <Area
              yAxisId="tokens"
              type="monotone"
              dataKey="cacheCreationTokens"
              name={t("usage.cacheCreationTokens", "缓存创建")}
              stroke="#f97316"
              fillOpacity={1}
              fill="url(#colorCacheCreation)"
              strokeWidth={2}
            />
            <Area
              yAxisId="tokens"
              type="monotone"
              dataKey="cacheReadTokens"
              name={t("usage.cacheReadTokens", "缓存命中")}
              stroke="#a855f7"
              fillOpacity={1}
              fill="url(#colorCacheRead)"
              strokeWidth={2}
            />
            <Area
              yAxisId="cost"
              type="monotone"
              dataKey="cost"
              name={t("usage.cost", "成本")}
              stroke="#f43f5e"
              fill="none"
              strokeWidth={2}
              strokeDasharray="4 4"
            />
          </AreaChart>
        </ResponsiveContainer>
      </div>
    </div>
  );
}
