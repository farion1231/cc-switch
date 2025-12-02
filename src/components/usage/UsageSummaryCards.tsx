import { useTranslation } from "react-i18next";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { useUsageSummary } from "@/lib/query/usage";

interface UsageSummaryCardsProps {
  days: number;
}

export function UsageSummaryCards({ days }: UsageSummaryCardsProps) {
  const { t } = useTranslation();
  const endDate = Math.floor(Date.now() / 1000);
  const startDate = endDate - days * 24 * 60 * 60;

  const { data: summary, isLoading } = useUsageSummary(startDate, endDate);
  const totalRequests = summary?.totalRequests ?? 0;
  const totalCost = parseFloat(summary?.totalCost || "0").toFixed(4);
  const totalInputTokens = summary?.totalInputTokens ?? 0;
  const totalOutputTokens = summary?.totalOutputTokens ?? 0;
  const totalTokens = totalInputTokens + totalOutputTokens;
  const cacheWriteTokens = summary?.totalCacheCreationTokens ?? 0;
  const cacheReadTokens = summary?.totalCacheReadTokens ?? 0;
  const totalCacheTokens = cacheWriteTokens + cacheReadTokens;

  if (isLoading) {
    return (
      <div className="grid gap-4 md:grid-cols-4">
        {[...Array(4)].map((_, i) => (
          <Card key={i}>
            <CardHeader className="pb-2">
              <div className="h-4 w-24 animate-pulse rounded bg-gray-200" />
            </CardHeader>
            <CardContent>
              <div className="h-8 w-32 animate-pulse rounded bg-gray-200" />
            </CardContent>
          </Card>
        ))}
      </div>
    );
  }

  return (
    <div className="grid gap-4 md:grid-cols-4">
      <Card>
        <CardHeader className="pb-2">
          <CardTitle className="text-sm font-medium text-muted-foreground">
            {t("usage.totalRequests", "总请求数")}
          </CardTitle>
        </CardHeader>
        <CardContent>
          <div className="text-2xl font-bold">
            {totalRequests.toLocaleString()}
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader className="pb-2">
          <CardTitle className="text-sm font-medium text-muted-foreground">
            {t("usage.totalCost", "总成本")}
          </CardTitle>
        </CardHeader>
        <CardContent>
          <div className="text-2xl font-bold">${totalCost}</div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader className="pb-2">
          <CardTitle className="text-sm font-medium text-muted-foreground">
            {t("usage.totalTokens", "总 Token 数")}
          </CardTitle>
        </CardHeader>
        <CardContent>
          <div className="text-2xl font-bold">
            {totalTokens.toLocaleString()}
          </div>
          <div className="mt-2 space-y-1 text-sm text-muted-foreground">
            <div>
              {t("usage.inputTokens", "输入")}:{" "}
              {totalInputTokens.toLocaleString()}
            </div>
            <div>
              {t("usage.outputTokens", "输出")}:{" "}
              {totalOutputTokens.toLocaleString()}
            </div>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader className="pb-2">
          <CardTitle className="text-sm font-medium text-muted-foreground">
            {t("usage.cacheTokens", "缓存 Token")}
          </CardTitle>
        </CardHeader>
        <CardContent>
          <div className="text-2xl font-bold">
            {totalCacheTokens.toLocaleString()}
          </div>
          <div className="mt-2 space-y-1 text-sm text-muted-foreground">
            <div>
              {t("usage.cacheWrite", "写入")}:{" "}
              {cacheWriteTokens.toLocaleString()}
            </div>
            <div>
              {t("usage.cacheRead", "读取")}: {cacheReadTokens.toLocaleString()}
            </div>
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
