import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { useModelStats } from "@/lib/query/usage";
import { TablePagination, useClientPagination } from "./TablePagination";
import { cn } from "@/lib/utils";
import {
  fmtInt,
  fmtUsd,
  formatTokensCompact,
  getLocaleFromLanguage,
  getResolvedLang,
} from "./format";
import { usageTable } from "./usageTable";
import { PerformanceCells, PerformanceHeaders } from "./statsColumns";
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
  const { t, i18n } = useTranslation();
  const locale = getLocaleFromLanguage(getResolvedLang(i18n));
  const { data: stats, isLoading } = useModelStats(
    range,
    { appType, providerName, model },
    {
      refetchInterval: refreshIntervalMs > 0 ? refreshIntervalMs : false,
    },
  );

  const rows = useMemo(
    () => [...(stats ?? [])].sort((a, b) => b.requestCount - a.requestCount),
    [stats],
  );
  const pagination = useClientPagination(
    rows,
    JSON.stringify([range, appType, providerName, model]),
  );

  if (isLoading) {
    return <div className={usageTable.skeleton} />;
  }

  return (
    <div className="flex flex-col">
      <div className={usageTable.scroller}>
        <table
          className={cn(usageTable.table, "min-w-[620px]")}
          aria-label={t("usage.modelStats")}
        >
          <thead>
            <tr className={usageTable.headRow}>
              <th className={usageTable.th}>{t("usage.model")}</th>
              <th className={usageTable.th}>{t("usage.requests")}</th>
              <th className={usageTable.th}>{t("usage.tokens")}</th>
              <th className={usageTable.th}>{t("usage.cost")}</th>
              <PerformanceHeaders />
            </tr>
          </thead>
          <tbody>
            {rows.length === 0 ? (
              <tr>
                <td colSpan={7} className={usageTable.empty}>
                  {t("usage.noData")}
                </td>
              </tr>
            ) : (
              pagination.pageRows.map((stat) => (
                <tr key={stat.model} className={usageTable.row}>
                  <td className={cn(usageTable.td, usageTable.mono)}>
                    <span
                      className="block max-w-[320px] truncate"
                      title={stat.model}
                    >
                      {stat.model}
                    </span>
                  </td>
                  <td className={usageTable.td}>
                    {fmtInt(stat.requestCount, locale)}
                  </td>
                  <td
                    className={usageTable.td}
                    title={fmtInt(stat.totalTokens, locale)}
                  >
                    {formatTokensCompact(stat.totalTokens, locale)}
                  </td>
                  <td
                    className={cn(usageTable.td, "font-medium")}
                    title={`${fmtUsd(stat.totalCost, 6)} · ${t("usage.avgCost")} ${fmtUsd(stat.avgCostPerRequest, 4)}`}
                  >
                    {fmtUsd(stat.totalCost, 2)}
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
  );
}
