import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Accordion,
  AccordionContent,
  AccordionItem,
  AccordionTrigger,
} from "@/components/ui/accordion";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { useModelStats } from "@/lib/query/usage";
import { fmtUsd } from "./format";
import { ModelUsageDistribution } from "./ModelUsageDistribution";
import { buildModelDistribution, formatModelShare } from "./modelDistribution";
import type { UsageRangeSelection } from "@/types/usage";

interface ModelStatsTableProps {
  range: UsageRangeSelection;
  appType?: string;
  providerName?: string;
  model?: string;
  refreshIntervalMs: number;
}

export function ModelStatsTable({
  range,
  appType,
  providerName,
  model,
  refreshIntervalMs,
}: ModelStatsTableProps) {
  const { t } = useTranslation();
  const [hiddenModels, setHiddenModels] = useState<Set<string>>(new Set());
  const {
    data: stats,
    isLoading,
    isError,
    refetch,
  } = useModelStats(
    range,
    { appType, providerName, model },
    {
      refetchInterval: refreshIntervalMs > 0 ? refreshIntervalMs : false,
    },
  );
  const distribution = useMemo(
    () => buildModelDistribution(stats ?? []),
    [stats],
  );

  // Hiding is a view preference for one filter scope, not part of the data query.
  useEffect(() => {
    setHiddenModels(new Set());
  }, [
    range.preset,
    range.customStartDate,
    range.customEndDate,
    range.liveEndTime,
    appType,
    providerName,
    model,
  ]);

  const toggleModel = (name: string) => {
    setHiddenModels((current) => {
      const next = new Set(current);
      if (next.has(name)) next.delete(name);
      else next.add(name);
      return next;
    });
  };

  if (isLoading && !stats) {
    return <div className="h-[400px] animate-pulse rounded bg-muted/50" />;
  }

  if (isError && !stats) {
    return (
      <div className="rounded-lg border border-border/50 bg-card/40 p-8 text-center">
        <p className="text-sm text-muted-foreground">
          {t("usage.modelDistribution.loadError")}
        </p>
        <button
          type="button"
          className="mt-3 text-sm text-primary hover:underline"
          onClick={() => void refetch()}
        >
          {t("usage.modelDistribution.retry")}
        </button>
      </div>
    );
  }

  return (
    <div className="space-y-4">
      {isError && (
        <p role="status" className="text-sm text-muted-foreground">
          {t("usage.modelDistribution.refreshError")}
        </p>
      )}
      {distribution.rows.length > 0 && (
        <Accordion type="single" collapsible>
          <AccordionItem
            value="distribution"
            className="overflow-hidden rounded-xl border border-border/50 bg-card/40"
          >
            <AccordionTrigger className="px-4 py-3 text-left hover:no-underline">
              {t("usage.modelDistribution.tokenShare")} /{" "}
              {t("usage.modelDistribution.costShare")}
            </AccordionTrigger>
            <AccordionContent className="px-4 pt-1">
              <ModelUsageDistribution
                distribution={distribution}
                hiddenModels={hiddenModels}
                onToggleModel={toggleModel}
                onShowAll={() => setHiddenModels(new Set())}
              />
            </AccordionContent>
          </AccordionItem>
        </Accordion>
      )}
      <div className="overflow-x-auto rounded-lg border border-border/50 bg-card/40 backdrop-blur-sm">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>{t("usage.model", "模型")}</TableHead>
              <TableHead className="text-right">
                {t("usage.requests", "请求数")}
              </TableHead>
              <TableHead className="text-right">
                {t("usage.tokens", "Tokens")}
              </TableHead>
              <TableHead className="text-right">
                {t("usage.modelDistribution.tokenShare")}
              </TableHead>
              <TableHead className="text-right">
                {t("usage.totalCost", "总成本")}
              </TableHead>
              <TableHead className="text-right">
                {t("usage.modelDistribution.costShare")}
              </TableHead>
              <TableHead className="text-right">
                {t("usage.avgCost", "平均成本")}
              </TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {distribution.rows.length === 0 ? (
              <TableRow>
                <TableCell
                  colSpan={7}
                  className="text-center text-muted-foreground"
                >
                  {t("usage.noData", "暂无数据")}
                </TableCell>
              </TableRow>
            ) : (
              distribution.rows.map((stat) => (
                <TableRow key={stat.model}>
                  <TableCell className="font-mono text-sm">
                    <span className="inline-flex items-center gap-2">
                      <span
                        aria-hidden="true"
                        className="h-2.5 w-2.5 shrink-0 rounded-full"
                        style={{ backgroundColor: stat.color }}
                      />
                      {stat.model}
                    </span>
                  </TableCell>
                  <TableCell className="text-right">
                    {stat.requestCount.toLocaleString()}
                  </TableCell>
                  <TableCell className="text-right">
                    {stat.totalTokens.toLocaleString()}
                  </TableCell>
                  <TableCell className="text-right">
                    {formatModelShare(stat.tokenShare)}
                  </TableCell>
                  <TableCell className="text-right">
                    {fmtUsd(stat.totalCost, 4)}
                  </TableCell>
                  <TableCell className="text-right">
                    {formatModelShare(stat.costShare)}
                  </TableCell>
                  <TableCell className="text-right">
                    {fmtUsd(stat.avgCostPerRequest, 6)}
                  </TableCell>
                </TableRow>
              ))
            )}
          </TableBody>
        </Table>
      </div>
    </div>
  );
}
