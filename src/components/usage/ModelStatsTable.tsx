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
import { SuccessSpeedCells, SuccessSpeedHeaders } from "./statsColumns";
import {
  getColumnLayout,
  MODEL_STATS_COLUMNS,
  type ModelStatsColumnId,
} from "./tableColumns";
import type { ColumnVisibility } from "@/types/table";
import type { UsageRangeSelection } from "@/types/usage";

interface ModelStatsTableProps {
  range: UsageRangeSelection;
  appType?: string;
  providerName?: string;
  model?: string;
  refreshIntervalMs: number;
  columnVisibility?: ColumnVisibility<ModelStatsColumnId>;
}

export function ModelStatsTable({
  range,
  appType,
  providerName,
  model,
  refreshIntervalMs,
  columnVisibility,
}: ModelStatsTableProps) {
  const { t, i18n } = useTranslation();
  const locale = getLocaleFromLanguage(getResolvedLang(i18n));
  const columns = getColumnLayout(MODEL_STATS_COLUMNS, columnVisibility);
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
          className={usageTable.table}
          style={{ minWidth: columns.minWidth }}
          aria-label={t("usage.modelStats")}
        >
          <thead>
            <tr className={usageTable.headRow}>
              <th className={usageTable.th}>{t("usage.model")}</th>
              {columns.isVisible("requests") && (
                <th className={usageTable.thEnd}>{t("usage.requests")}</th>
              )}
              {columns.isVisible("tokens") && (
                <th className={usageTable.thEnd}>{t("usage.tokens")}</th>
              )}
              {columns.isVisible("cost") && (
                <th className={usageTable.thEnd}>{t("usage.cost")}</th>
              )}
              <SuccessSpeedHeaders
                showSuccessRate={columns.isVisible("successRate")}
                showSpeed={columns.isVisible("speed")}
              />
            </tr>
          </thead>
          <tbody>
            {rows.length === 0 ? (
              <tr>
                <td colSpan={columns.visibleCount} className={usageTable.empty}>
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
                  {columns.isVisible("requests") && (
                    <td className={usageTable.tdEnd}>
                      {fmtInt(stat.requestCount, locale)}
                    </td>
                  )}
                  {columns.isVisible("tokens") && (
                    <td
                      className={usageTable.tdEnd}
                      title={fmtInt(stat.totalTokens, locale)}
                    >
                      {formatTokensCompact(stat.totalTokens, locale)}
                    </td>
                  )}
                  {columns.isVisible("cost") && (
                    <td
                      className={cn(usageTable.tdEnd, "font-medium")}
                      title={`${fmtUsd(stat.totalCost, 6)} · ${t("usage.avgCost")} ${fmtUsd(stat.avgCostPerRequest, 4)}`}
                    >
                      {fmtUsd(stat.totalCost, 2)}
                    </td>
                  )}
                  <SuccessSpeedCells
                    stat={stat}
                    showSuccessRate={columns.isVisible("successRate")}
                    showSpeed={columns.isVisible("speed")}
                  />
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
