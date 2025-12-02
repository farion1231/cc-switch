import { useTranslation } from "react-i18next";
import {
  LineChart,
  Line,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ResponsiveContainer,
  Legend,
} from "recharts";
import { useUsageTrends } from "@/lib/query/usage";

interface UsageTrendChartProps {
  days: number;
}

export function UsageTrendChart({ days }: UsageTrendChartProps) {
  const { t } = useTranslation();
  const { data: trends, isLoading } = useUsageTrends(days);

  if (isLoading) {
    return <div className="h-[320px] animate-pulse rounded bg-gray-100" />;
  }

  const isToday = days === 1;
  const chartData =
    trends?.map((stat) => {
      const pointDate = new Date(stat.date);
      return {
        rawDate: stat.date,
        label: isToday
          ? pointDate.toLocaleTimeString("zh-CN", { hour: "2-digit" })
          : pointDate.toLocaleDateString("zh-CN", {
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

  const hourlyData = (() => {
    if (!isToday) return chartData;
    const map = new Map<number, (typeof chartData)[number]>();
    chartData.forEach((point) => {
      map.set(point.hour ?? 0, point);
    });
    return Array.from({ length: 24 }, (_, hour) => {
      const bucket = map.get(hour);
      return {
        label: `${hour.toString().padStart(2, "0")}:00`,
        inputTokens: bucket?.inputTokens ?? 0,
        outputTokens: bucket?.outputTokens ?? 0,
        cacheCreationTokens: bucket?.cacheCreationTokens ?? 0,
        cacheReadTokens: bucket?.cacheReadTokens ?? 0,
        cost: bucket?.cost ?? 0,
      };
    });
  })();

  const displayData = isToday ? hourlyData : chartData;

  const rangeLabel = isToday
    ? t("usage.rangeToday", "今天 (按小时)")
    : days === 7
      ? t("usage.rangeLast7Days", "过去 7 天")
      : t("usage.rangeLast30Days", "过去 30 天");

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <h3 className="text-lg font-semibold">
          {t("usage.trends", "使用趋势")}
        </h3>
        <p className="text-sm text-muted-foreground">{rangeLabel}</p>
      </div>

      <ResponsiveContainer width="100%" height={320}>
        <LineChart data={displayData}>
          <CartesianGrid strokeDasharray="3 3" />
          <XAxis dataKey="label" />
          <YAxis
            yAxisId="tokens"
            label={{
              value: t("usage.tokensAxis", "Tokens"),
              angle: -90,
              position: "insideLeft",
            }}
          />
          <YAxis
            yAxisId="cost"
            orientation="right"
            label={{
              value: t("usage.costAxis", "成本 (USD)"),
              angle: 90,
              position: "insideRight",
            }}
          />
          <Tooltip />
          <Legend />
          <Line
            yAxisId="tokens"
            type="monotone"
            dataKey="inputTokens"
            name={t("usage.inputTokens", "输入 Tokens")}
            stroke="#2563eb"
            strokeWidth={2}
            dot={false}
            isAnimationActive
          />
          <Line
            yAxisId="tokens"
            type="monotone"
            dataKey="outputTokens"
            name={t("usage.outputTokens", "输出 Tokens")}
            stroke="#16a34a"
            strokeWidth={2}
            dot={false}
            isAnimationActive
          />
          <Line
            yAxisId="tokens"
            type="monotone"
            dataKey="cacheCreationTokens"
            name={t("usage.cacheCreationTokens", "缓存写入")}
            stroke="#f97316"
            strokeWidth={2}
            dot={false}
            isAnimationActive
          />
          <Line
            yAxisId="tokens"
            type="monotone"
            dataKey="cacheReadTokens"
            name={t("usage.cacheReadTokens", "缓存读取")}
            stroke="#a855f7"
            strokeWidth={2}
            dot={false}
            isAnimationActive
          />
          <Line
            yAxisId="cost"
            type="monotone"
            dataKey="cost"
            name={t("usage.cost", "成本")}
            stroke="#dc2626"
            strokeWidth={2}
            dot={false}
            isAnimationActive
          />
        </LineChart>
      </ResponsiveContainer>
    </div>
  );
}
