import { useTranslation } from "react-i18next";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { useProviderStats } from "@/lib/query/usage";
import { fmtUsd } from "./format";
import type { UsageRangeSelection } from "@/types/usage";

interface ProviderStatsTableProps {
  range: UsageRangeSelection;
  appType?: string;
  providerName?: string;
  model?: string;
  profileName?: string;
  task?: string;
  refreshIntervalMs: number;
}

function countLabelKey(appType?: string): string {
  if (appType === "hermes") return "usage.countLabel.hermesApiCalls";
  if (!appType || appType === "all") return "usage.countLabel.mixedActivity";
  return "usage.countLabel.requests";
}

export function ProviderStatsTable({
  range,
  appType,
  providerName,
  model,
  profileName,
  task,
  refreshIntervalMs,
}: ProviderStatsTableProps) {
  const { t } = useTranslation();
  const { data: stats, isLoading } = useProviderStats(
    range,
    { appType, providerName, model, profileName, task },
    {
      refetchInterval: refreshIntervalMs > 0 ? refreshIntervalMs : false,
    },
  );

  if (isLoading) {
    return <div className="h-[400px] animate-pulse rounded bg-gray-100" />;
  }

  const showStatusMetrics = appType !== "hermes";

  return (
    <div className="rounded-lg border border-border/50 bg-card/40 backdrop-blur-sm overflow-hidden">
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>{t("usage.provider", "Provider")}</TableHead>
            <TableHead className="text-right">
              {t(countLabelKey(appType))}
            </TableHead>
            <TableHead className="text-right">
              {t("usage.tokens", "Tokens")}
            </TableHead>
            <TableHead className="text-right">
              {t("usage.cost", "成本")}
            </TableHead>
            {showStatusMetrics && (
              <TableHead className="text-right">
                {t("usage.successRate", "成功率")}
              </TableHead>
            )}
            {showStatusMetrics && (
              <TableHead className="text-right">
                {t("usage.avgLatency", "平均延迟")}
              </TableHead>
            )}
          </TableRow>
        </TableHeader>
        <TableBody>
          {stats?.length === 0 ? (
            <TableRow>
              <TableCell
                colSpan={showStatusMetrics ? 6 : 4}
                className="text-center text-muted-foreground"
              >
                {t("usage.noData", "暂无数据")}
              </TableCell>
            </TableRow>
          ) : (
            stats?.map((stat) => (
              <TableRow key={stat.providerId}>
                <TableCell className="font-medium">
                  {stat.providerName}
                </TableCell>
                <TableCell className="text-right">
                  {stat.requestCount.toLocaleString()}
                </TableCell>
                <TableCell className="text-right">
                  {stat.totalTokens.toLocaleString()}
                </TableCell>
                <TableCell className="text-right">
                  {fmtUsd(stat.totalCost, 4)}
                </TableCell>
                {showStatusMetrics && (
                  <TableCell className="text-right">
                    {stat.statusAvailable === false
                      ? t("usage.hermes.notAvailable")
                      : `${stat.successRate.toFixed(1)}%`}
                  </TableCell>
                )}
                {showStatusMetrics && (
                  <TableCell className="text-right">
                    {stat.latencyAvailable === false
                      ? t("usage.hermes.notAvailable")
                      : `${stat.avgLatencyMs}ms`}
                  </TableCell>
                )}
              </TableRow>
            ))
          )}
        </TableBody>
      </Table>
    </div>
  );
}
