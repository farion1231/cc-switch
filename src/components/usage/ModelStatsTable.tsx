import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { ArrowDown } from "lucide-react";
import {
  Accordion,
  AccordionContent,
  AccordionItem,
  AccordionTrigger,
} from "@/components/ui/accordion";
import { useModelStats } from "@/lib/query/usage";
import { TablePagination, useClientPagination } from "./TablePagination";
import { cn } from "@/lib/utils";
import {
  fmtInt,
  formatTokensCompact,
  getLocaleFromLanguage,
  getResolvedLang,
} from "./format";
import { usageTable } from "./usageTable";
import { PerformanceCells, PerformanceHeaders } from "./statsColumns";
import { ModelUsageDistribution } from "./ModelUsageDistribution";
import { buildModelDistribution, formatModelShare } from "./modelDistribution";
import type { UsageRangeSelection } from "@/types/usage";
import { useMoneyFormat } from "@/hooks/useMoneyFormat";

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
  const { t, i18n } = useTranslation();
  const { money, moneyTitle } = useMoneyFormat();
  const locale = getLocaleFromLanguage(getResolvedLang(i18n));
  const [chartsOpen, setChartsOpen] = useState("");
  const [hiddenModels, setHiddenModels] = useState<Set<string>>(new Set());
  const {
    data: stats,
    isLoading,
    isPlaceholderData,
    isError,
    refetch,
  } = useModelStats(
    range,
    { appType, providerName, model },
    {
      refetchInterval: refreshIntervalMs > 0 ? refreshIntervalMs : false,
    },
  );

  // Both charts and the table use the full filter scope, before pagination.
  const distribution = useMemo(
    () => buildModelDistribution(stats ?? []),
    [stats],
  );
  const rows = useMemo(
    () =>
      [...distribution.rows].sort((a, b) => b.requestCount - a.requestCount),
    [distribution.rows],
  );
  const pagination = useClientPagination(
    rows,
    JSON.stringify([range, appType, providerName, model]),
  );

  // A legend selection belongs to one filter scope; refreshing keeps it.
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

  // keepPreviousData can belong to a different filter scope.
  if (isLoading || isPlaceholderData) {
    return <div className={usageTable.skeleton} />;
  }

  if (isError && !stats) {
    return (
      <div className="rounded-panel border border-border bg-surface p-8 text-center">
        <p role="status" className="text-body text-fg-2">
          {t("usage.modelDistribution.loadError")}
        </p>
        <button
          type="button"
          className="mt-3 rounded-control text-body text-action-text hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
          onClick={() => void refetch()}
        >
          {t("usage.modelDistribution.retry")}
        </button>
      </div>
    );
  }

  return (
    <div className="flex min-w-0 flex-col gap-3.5">
      {isError && (
        <p role="status" className="text-body text-fg-2">
          {t("usage.modelDistribution.refreshError")}
        </p>
      )}
      {distribution.rows.length > 0 && (
        <Accordion
          type="single"
          collapsible
          value={chartsOpen}
          onValueChange={setChartsOpen}
        >
          <AccordionItem
            value="distribution"
            className="overflow-hidden rounded-panel border border-border bg-surface"
          >
            <AccordionTrigger className="px-4 py-3 text-left text-strong text-fg-1 hover:no-underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring">
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
      <div className="flex min-w-0 flex-col">
        <div className={usageTable.scroller}>
          {/* 和请求日志同一套列组与取宽：按内容取宽、至少铺满面板，放不下横向滚动 */}
          <table
            className={cn(usageTable.table, "w-max min-w-full")}
            aria-label={t("usage.modelStats")}
          >
            <thead>
              <tr className={usageTable.headRow}>
                <th className={usageTable.th}>{t("usage.model")}</th>
                {/* 按请求数从多到少排：箭头就是排序说明，不另写文字 */}
                <th className={usageTable.th} aria-sort="descending">
                  <span className="inline-flex items-center gap-1">
                    {t("usage.requests")}
                    <ArrowDown
                      aria-hidden="true"
                      className="h-3 w-3 text-fg-3"
                      strokeWidth={2}
                    />
                  </span>
                </th>
                <th className={cn(usageTable.th, usageTable.groupGap)}>
                  {t("usage.cost")}
                </th>
                <th className={usageTable.th}>
                  {t("usage.modelDistribution.costShare")}
                </th>
                <th className={cn(usageTable.th, usageTable.groupGap)}>
                  {t("usage.tokens")}
                </th>
                <th className={usageTable.th}>
                  {t("usage.modelDistribution.tokenShare")}
                </th>
                <PerformanceHeaders />
              </tr>
            </thead>
            <tbody>
              {rows.length === 0 ? (
                <tr>
                  <td colSpan={9} className={usageTable.empty}>
                    {t("usage.noData")}
                  </td>
                </tr>
              ) : (
                pagination.pageRows.map((stat) => (
                  <tr key={stat.model} className={usageTable.row}>
                    <td className={cn(usageTable.td, usageTable.mono)}>
                      <span className="flex items-center gap-2">
                        <span
                          aria-hidden="true"
                          className="h-2.5 w-2.5 shrink-0 rounded-full"
                          style={{ backgroundColor: stat.color }}
                        />
                        <span title={stat.model}>{stat.model}</span>
                      </span>
                    </td>
                    <td className={usageTable.td}>
                      {fmtInt(stat.requestCount, locale)}
                    </td>
                    <td
                      className={cn(
                        usageTable.td,
                        usageTable.groupGap,
                        "font-medium",
                      )}
                      title={`${moneyTitle(stat.totalCost, 6)} · ${t("usage.avgCost")} ${money(stat.avgCostPerRequest, 4)}`}
                    >
                      {money(stat.totalCost, 2)}
                    </td>
                    <td className={usageTable.td}>
                      {formatModelShare(stat.costShare)}
                    </td>
                    <td
                      className={cn(usageTable.td, usageTable.groupGap)}
                      title={fmtInt(stat.totalTokens, locale)}
                    >
                      {formatTokensCompact(stat.totalTokens, locale)}
                    </td>
                    <td className={usageTable.td}>
                      {formatModelShare(stat.tokenShare)}
                    </td>
                    <PerformanceCells stat={stat} />
                  </tr>
                ))
              )}
            </tbody>
          </table>
        </div>
        <TablePagination
          page={pagination.page}
          totalPages={pagination.totalPages}
          total={pagination.total}
          onPageChange={pagination.setPage}
        />
      </div>
    </div>
  );
}
